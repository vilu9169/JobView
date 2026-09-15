//! Optional remote classification. No work runs on startup and the key never
//! appears in status, SQLite, logs, or the email payload.
use std::sync::{Arc, RwLock};

use serde::Serialize;
use tokio::sync::{watch, Mutex};
use zeroize::Zeroizing;

use crate::{
    domain::{AppSettings, Email, WorkspaceSnapshot},
    error::{AppError, AppResult},
    repository::{AiSettings, AiUsage, Repository},
    services::{
        classification::{ClassificationService, LocalClassificationService},
        credentials::{platform_secure_store, SecretValue, SecureCredentialService},
        gemini::{
            validate_api_key, GeminiClassificationService, RemoteClassificationService,
            PROMPT_VERSION,
        },
        normalization::{normalize_email, NormalizedEmail, RawEmail},
    },
};

// Preserve the existing Windows Credential Manager namespace and OAuth entries.
// A separate key prevents either integration from reading/deleting the other.
const API_KEY: &str = "gemini-api-key-v1";

#[derive(Clone, Default)]
struct Runtime {
    running: bool,
    processed: u32,
    total: u32,
    applied: u32,
    cache_hits: u32,
    requests: u32,
    last_error: Option<String>,
    last_message: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub configured: bool,
    pub credential_store_available: bool,
    pub settings: AiSettings,
    pub usage: AiUsage,
    pub running: bool,
    pub processed: u32,
    pub total: u32,
    pub applied: u32,
    pub cache_hits: u32,
    pub requests: u32,
    pub last_error: Option<String>,
    pub last_message: Option<String>,
}

pub struct AiIntegration {
    repository: Repository,
    credentials: Arc<dyn SecureCredentialService>,
    service: Arc<dyn RemoteClassificationService>,
    operation: Mutex<()>,
    cancellation: watch::Sender<bool>,
    runtime: RwLock<Runtime>,
}

impl AiIntegration {
    pub fn new(repository: Repository) -> AppResult<Self> {
        Ok(Self::with_services(
            repository,
            platform_secure_store(),
            Arc::new(GeminiClassificationService::new()?),
        ))
    }

    fn with_services(
        repository: Repository,
        credentials: Arc<dyn SecureCredentialService>,
        service: Arc<dyn RemoteClassificationService>,
    ) -> Self {
        Self {
            repository,
            credentials,
            service,
            operation: Mutex::new(()),
            cancellation: watch::channel(false).0,
            runtime: RwLock::new(Runtime::default()),
        }
    }

    pub async fn status(&self) -> AppResult<AiStatus> {
        let key = self.credentials.get(API_KEY);
        let runtime = self.runtime.read().map_err(|_| state_error())?.clone();
        Ok(AiStatus {
            configured: matches!(&key, Ok(Some(_))),
            credential_store_available: key.is_ok(),
            settings: self.repository.ai_settings().await?,
            usage: self.repository.ai_usage().await?,
            running: runtime.running,
            processed: runtime.processed,
            total: runtime.total,
            applied: runtime.applied,
            cache_hits: runtime.cache_hits,
            requests: runtime.requests,
            last_error: runtime.last_error,
            last_message: runtime.last_message,
        })
    }

    pub async fn save_key(&self, api_key: Zeroizing<String>) -> AppResult<AiStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        let value = api_key.trim();
        validate_api_key(value.as_bytes())
            .map_err(|error| AppError::Validation(error.to_string()))?;
        self.credentials
            .set(API_KEY, &SecretValue::new(value.as_bytes().to_vec()))
            .map_err(|_| credential_error())?;
        self.update(|state| {
            state.last_error = None;
            state.last_message = Some("API key saved securely. Test the connection, then save an AI mode to enable email classification.".into());
        })?;
        self.status().await
    }

    pub async fn remove_key(&self) -> AppResult<AiStatus> {
        self.cancellation.send_replace(true);
        let _operation = self.operation.lock().await;
        self.credentials
            .delete(API_KEY)
            .map_err(|_| credential_error())?;
        let mut settings = self.repository.app_settings().await?;
        settings.ai_mode = "off".into();
        self.repository.save_settings(settings).await?;
        self.update(|state| {
            state.last_error = None;
            state.last_message = Some("API key removed. AI classification is off; saved classifications remain available.".into());
        })?;
        self.status().await
    }

    pub async fn save_configuration(&self, settings: AiSettings) -> AppResult<AiStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        self.repository.save_ai_settings(settings).await?;
        self.status().await
    }

    /// Wait until any in-flight request has been dropped before acknowledging
    /// the privacy-mode change. No new email can be sent after Off is saved.
    pub async fn save_workspace_settings(
        &self,
        settings: AppSettings,
    ) -> AppResult<WorkspaceSnapshot> {
        self.cancellation.send_replace(true);
        let _operation = self.operation.lock().await;
        self.repository.save_settings(settings).await
    }

    pub async fn cancel(&self) -> AppResult<AiStatus> {
        self.cancellation.send_replace(true);
        self.status().await
    }

    pub async fn test_connection(&self) -> AppResult<AiStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        self.begin(1)?;
        let result = self.test_inner().await;
        self.finish(result)?;
        self.status().await
    }

    async fn test_inner(&self) -> AppResult<String> {
        let key = self.key()?;
        let settings = self.repository.ai_settings().await?;
        let email = normalize_email(&RawEmail {
            gmail_message_id: String::new(), gmail_thread_id: String::new(),
            sender_name: "Fictional Hiring Team".into(),
            sender_email: "hiring@fictional.example".into(),
            recipients: vec![], subject: "Tack för din ansökan".into(),
            received_at: "2026-09-01T12:00:00Z".into(), snippet: String::new(),
            body_text: Some("Detta är ett fiktivt anslutningstest. Vi har tagit emot din ansökan till tjänsten som utvecklare hos Exempelbolaget. Du behöver inte göra något just nu.".into()),
            body_html: None,
        });
        self.repository
            .record_ai_attempt(&email.content_hash)
            .await?;
        self.update(|state| state.requests = 1)?;
        let mut cancellation = self.cancellation.subscribe();
        let response = tokio::select! {
            biased;
            _ = cancelled(&mut cancellation) => return Ok("Connection test cancelled.".into()),
            response = self.service.classify(&email, &settings.model, &key) => response.map_err(|error| AppError::Integration(error.to_string()))?,
        };
        self.repository
            .store_ai_classification(
                &email.content_hash,
                &settings.model,
                PROMPT_VERSION,
                &response.result,
                response.input_tokens as i64,
                response.output_tokens as i64,
            )
            .await?;
        self.update(|state| state.processed = 1)?;
        Ok("Connection successful: Gemini returned a valid classification for fictional Swedish text. No mailbox email was sent.".into())
    }

    pub async fn classify(&self, force: bool) -> AppResult<AiStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        self.begin(0)?;
        let result = self.classify_inner(force).await;
        self.finish(result)?;
        self.status().await
    }

    /// Called only after a user-requested Gmail sync. Failure remains visible in
    /// AI status and never rolls back a successfully cached Gmail message.
    pub async fn after_sync(&self) {
        match self.repository.app_settings().await {
            Ok(settings) if settings.ai_mode != "off" => {
                if let Err(error) = self.classify(false).await {
                    error.log_safe("classify_after_sync");
                }
            }
            Err(error) => error.log_safe("read_ai_mode_after_sync"),
            _ => {}
        }
    }

    async fn classify_inner(&self, force: bool) -> AppResult<String> {
        let workspace = self.repository.snapshot().await?;
        if workspace.settings.ai_mode == "off" {
            return Err(AppError::Validation(
                "Save an AI classification mode in Settings before reviewing emails with Gemini."
                    .into(),
            ));
        }
        let settings = self.repository.ai_settings().await?;
        let key = self.key()?;
        let candidates: Vec<_> = workspace
            .emails
            .into_iter()
            .filter(|email| eligible(email, &workspace.settings))
            .collect();
        self.update(|state| state.total = candidates.len() as u32)?;
        let mut cancellation = self.cancellation.subscribe();
        let mut requests = 0;
        let mut previously_attempted = 0;
        for email in candidates {
            if *cancellation.borrow() {
                return Ok("AI review cancelled. Completed results are saved.".into());
            }
            // Settings changes from another app window/process are rechecked.
            let current = self.repository.app_settings().await?;
            if current.ai_mode == "off" {
                return Ok("AI review stopped because classification is off.".into());
            }
            if !eligible(&email, &current) {
                continue;
            }
            if !force {
                if let Some(cached) = self
                    .repository
                    .cached_ai_classification(&email.content_hash)
                    .await?
                {
                    let applied = self
                        .repository
                        .apply_ai_classification(&email.id, &email.content_hash, &cached.result)
                        .await?;
                    self.repository.record_ai_cache_hit().await?;
                    self.update(|state| {
                        state.processed += 1;
                        state.cache_hits += 1;
                        state.applied += u32::from(applied);
                    })?;
                    continue;
                }
                if self.repository.has_ai_attempt(&email.content_hash).await? {
                    previously_attempted += 1;
                    self.update(|state| state.processed += 1)?;
                    continue;
                }
            }
            if requests >= settings.max_requests_per_run {
                return Ok(if force {
                    "Resend limit reached. Completed results are saved. A forced review starts from the newest candidates each time; increase the limit before resending a larger set.".into()
                } else {
                    "Request limit reached. Completed results are saved; review again to continue with remaining emails.".into()
                });
            }
            // Avoid sending a stale candidate after a user correction during a
            // long run. Persistence also checks the hash/override after response.
            let fresh_settings = self.repository.app_settings().await?;
            let Some(latest) = self.repository.ai_email(&email.id).await? else {
                continue;
            };
            if latest.content_hash != email.content_hash || !eligible(&latest, &fresh_settings) {
                continue;
            }
            self.repository
                .record_ai_attempt(&email.content_hash)
                .await?;
            requests += 1;
            self.update(|state| state.requests += 1)?;
            let normalized = normalized(&email);
            let response = tokio::select! {
                biased;
                _ = cancelled(&mut cancellation) => return Ok("AI review cancelled. Completed results are saved; an already sent request may still be billed.".into()),
                response = self.service.classify(&normalized, &settings.model, &key) => response.map_err(|error| AppError::Integration(error.to_string()))?,
            };
            self.repository
                .store_ai_classification(
                    &email.content_hash,
                    &settings.model,
                    PROMPT_VERSION,
                    &response.result,
                    response.input_tokens as i64,
                    response.output_tokens as i64,
                )
                .await?;
            let applied = self
                .repository
                .apply_ai_classification(&email.id, &email.content_hash, &response.result)
                .await?;
            self.update(|state| {
                state.processed += 1;
                state.applied += u32::from(applied);
            })?;
        }
        Ok(if previously_attempted > 0 {
            format!("Review complete. {previously_attempted} previously unsuccessful requests were skipped. Use the explicit resend option to retry them.")
        } else {
            "Review complete. Saved results are reused when email content is unchanged.".into()
        })
    }

    fn key(&self) -> AppResult<SecretValue> {
        self.credentials
            .get(API_KEY)
            .map_err(|_| credential_error())?
            .ok_or_else(|| AppError::Validation("Add a Gemini API key in Settings first.".into()))
    }

    fn begin(&self, total: u32) -> AppResult<()> {
        self.cancellation.send_replace(false);
        self.update(|state| {
            *state = Runtime {
                running: true,
                total,
                ..Runtime::default()
            }
        })
    }

    fn finish(&self, result: AppResult<String>) -> AppResult<()> {
        self.update(|state| {
            state.running = false;
            match &result {
                Ok(message) => state.last_message = Some(message.clone()),
                Err(error) => state.last_error = Some(error.to_string()),
            }
        })?;
        result.map(|_| ())
    }

    fn update(&self, change: impl FnOnce(&mut Runtime)) -> AppResult<()> {
        let mut state = self.runtime.write().map_err(|_| state_error())?;
        change(&mut state);
        Ok(())
    }
}

fn normalized(email: &Email) -> NormalizedEmail {
    NormalizedEmail {
        sender_name: email.sender_name.clone(),
        sender_email: email.sender_email.clone(),
        recipients: vec![],
        subject: email.subject.clone(),
        received_at: email.received_at.clone(),
        snippet: email.snippet.clone(),
        body_text: email.body_text.clone(),
        content_hash: email.content_hash.clone(),
    }
}

/// Broader than the event rules so less formulaic Swedish replies reach AI.
/// Explicitly excluded mail and user corrections never become bulk candidates.
fn eligible(email: &Email, settings: &AppSettings) -> bool {
    if settings.ai_mode == "off" || email.manual_override || email.ignored || email.remote_deleted {
        return false;
    }
    let local = LocalClassificationService.classify(&normalized(email));
    if matches!(
        local.category.as_str(),
        "account_notification" | "promotion" | "newsletter" | "receipt" | "job_alert"
    ) {
        return false;
    }
    let content = format!("{} {}", email.subject, email.body_text).to_lowercase();
    let signals = [
        "ansök",
        "rekryter",
        "intervju",
        "tjänsten",
        "tjänst som",
        "praktik",
        "trainee",
        "anställ",
        "kandidatur",
        "urvalsprocess",
        "du sökt",
        "job application",
        "recruit",
        "interview",
        "hiring",
        "your candidacy",
        "position you",
        "assessment",
        "arbetsprov",
        "personlighetstest",
    ];
    let candidate = local.is_job_related || signals.iter().any(|signal| content.contains(signal));
    if !candidate {
        return false;
    }
    match settings.ai_mode.as_str() {
        "candidates" => true,
        "uncertain" => {
            let strength = if local.is_job_related {
                local.confidence
            } else {
                local.confidence.max(0.5)
            };
            strength >= settings.local_confidence_gemini_threshold
                && strength < settings.local_confidence_accept_threshold
        }
        _ => false,
    }
}

async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() || receiver.changed().await.is_err() {
            return;
        }
    }
}
fn credential_error() -> AppError {
    AppError::Integration("Windows secure credential storage is unavailable. The Gemini API key was not saved or read.".into())
}
fn state_error() -> AppError {
    AppError::Integration("AI status is unavailable. Restart JobView.".into())
}
fn busy_error() -> AppError {
    AppError::Validation(
        "An AI operation is already running. Wait for it or cancel the review.".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        classification::ClassificationResult,
        credentials::MemoryCredentialService,
        gemini::{AiError, AiResponse},
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    #[derive(Default)]
    struct FakeService {
        calls: AtomicUsize,
        fail_at: Option<usize>,
        block: bool,
        started: Notify,
        release: Notify,
        subjects: std::sync::Mutex<Vec<String>>,
    }
    #[async_trait]
    impl RemoteClassificationService for FakeService {
        async fn classify(
            &self,
            email: &NormalizedEmail,
            _model: &str,
            _key: &SecretValue,
        ) -> Result<AiResponse, AiError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            self.subjects.lock().unwrap().push(email.subject.clone());
            self.started.notify_one();
            if self.block {
                self.release.notified().await;
            }
            if self.fail_at == Some(call) {
                return Err(AiError::RateLimited);
            }
            Ok(AiResponse {
                result: ClassificationResult {
                    is_job_related: true,
                    category: "job_related".into(),
                    event_type: None,
                    company: Some("Exempelbolaget".into()),
                    role: None,
                    suggested_stage: None,
                    requires_action: false,
                    suggested_action: None,
                    deadline: None,
                    confidence: 0.8,
                    reasoning_code: "personal_job_update".into(),
                },
                input_tokens: 120,
                output_tokens: 80,
            })
        }
    }

    fn raw(id: usize, subject: &str, body: &str) -> RawEmail {
        RawEmail {
            gmail_message_id: format!("fixture-ai-{id}"),
            gmail_thread_id: format!("fixture-ai-thread-{id}"),
            sender_name: "Exempelbolaget".into(),
            sender_email: "hiring@fictional.example".into(),
            recipients: vec!["recipient@fictional.example".into()],
            subject: subject.into(),
            received_at: "2026-09-01T12:00:00Z".into(),
            snippet: String::new(),
            body_text: Some(body.into()),
            body_html: None,
        }
    }
    async fn setup(
        service: Arc<FakeService>,
    ) -> (Arc<AiIntegration>, Repository, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::open(dir.path().join("ai.sqlite3"))
            .await
            .unwrap();
        repo.import_emails(&[
            raw(1, "Angående tjänsten", "Vi går igenom urvalet och återkommer inom kort."),
            raw(2, "Din kandidatur", "Vi återkommer om nästa steg."),
            raw(3, "Tack för din ansökan", "Vi har tagit emot din ansökan."),
            raw(4, "Back to school sale", "Career goals and recruiter-ready projects. Get 50% off courses and annual subscription plans."),
            raw(5, "Password reset", "Please change your password. Talent Acquisition."),
            raw(6, "Middag på fredag", "Vi ses klockan sex."),
        ]).await.unwrap();
        let ai = Arc::new(AiIntegration::with_services(
            repo.clone(),
            Arc::new(MemoryCredentialService::default()),
            service,
        ));
        ai.save_key(Zeroizing::new("fictional-test-api-value-only".into()))
            .await
            .unwrap();
        (ai, repo, dir)
    }
    async fn mode(ai: &AiIntegration, value: &str) {
        ai.save_workspace_settings(AppSettings {
            ai_mode: value.into(),
            ..AppSettings::default()
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn off_never_sends_mail_and_connection_test_uses_only_fictional_text() {
        let service = Arc::new(FakeService::default());
        let (ai, repo, _dir) = setup(service.clone()).await;
        assert!(ai.classify(false).await.is_err());
        ai.after_sync().await;
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
        let status = ai.test_connection().await.unwrap();
        assert_eq!(status.usage.requests, 1);
        assert_eq!(status.usage.succeeded, 1);
        assert_eq!(repo.app_settings().await.unwrap().ai_mode, "off");
        assert!(repo
            .snapshot()
            .await
            .unwrap()
            .emails
            .iter()
            .all(|email| email.classification_source == "local_rules"));
        assert_eq!(
            service.subjects.lock().unwrap().as_slice(),
            ["Tack för din ansökan"]
        );
    }

    #[tokio::test]
    async fn saving_long_authorization_key_preserves_bytes_and_rejects_bad_replacements() {
        let service = Arc::new(FakeService::default());
        let (ai, repo, _dir) = setup(service.clone()).await;
        let value = format!("AQ.{}._~+/=", "fictional".repeat(100));
        let status = ai
            .save_key(Zeroizing::new(format!("  {value}\r\n")))
            .await
            .unwrap();
        assert!(status.configured);
        assert_eq!(
            ai.credentials.get(API_KEY).unwrap().unwrap().expose(),
            value.as_bytes()
        );
        assert!(!serde_json::to_string(&status).unwrap().contains(&value));
        assert_eq!(repo.app_settings().await.unwrap().ai_mode, "off");
        for invalid in [format!("{value}\r\nInjected: secret"), "f".repeat(2561)] {
            let error = ai
                .save_key(Zeroizing::new(invalid.clone()))
                .await
                .err()
                .unwrap();
            assert!(!error.to_string().contains(&invalid));
            assert_eq!(
                ai.credentials.get(API_KEY).unwrap().unwrap().expose(),
                value.as_bytes()
            );
        }
        assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn modes_route_broader_swedish_candidates_but_filter_marketing_and_security() {
        let service = Arc::new(FakeService::default());
        let (ai, _repo, _dir) = setup(service.clone()).await;
        mode(&ai, "uncertain").await;
        let first = ai.classify(false).await.unwrap();
        assert_eq!(first.requests, 2);
        mode(&ai, "candidates").await;
        let second = ai.classify(false).await.unwrap();
        assert_eq!(second.requests, 1);
        assert_eq!(second.cache_hits, 2);
        let subjects = service.subjects.lock().unwrap();
        assert_eq!(subjects.len(), 3);
        assert!(!subjects.iter().any(|subject| subject.contains("sale")
            || subject.contains("reset")
            || subject.contains("Middag")));
    }

    #[tokio::test]
    async fn capped_runs_resume_and_reuse_persisted_results_until_explicit_resend() {
        let service = Arc::new(FakeService::default());
        let (ai, repo, dir) = setup(service.clone()).await;
        mode(&ai, "candidates").await;
        ai.save_configuration(AiSettings {
            max_requests_per_run: 1,
            ..AiSettings::default()
        })
        .await
        .unwrap();
        for _ in 0..3 {
            assert_eq!(ai.classify(false).await.unwrap().requests, 1);
        }
        assert_eq!(service.calls.load(Ordering::SeqCst), 3);
        let reopened = Repository::open(dir.path().join("ai.sqlite3"))
            .await
            .unwrap();
        let restarted =
            AiIntegration::with_services(reopened, ai.credentials.clone(), service.clone());
        let cached = restarted.classify(false).await.unwrap();
        assert_eq!(cached.requests, 0);
        assert_eq!(cached.cache_hits, 3);
        assert_eq!(restarted.classify(true).await.unwrap().requests, 1);
        assert_eq!(repo.ai_usage().await.unwrap().requests, 4);
    }

    #[tokio::test]
    async fn failed_request_preserves_mail_and_is_not_automatically_charged_again() {
        let service = Arc::new(FakeService {
            fail_at: Some(2),
            ..FakeService::default()
        });
        let (ai, repo, _dir) = setup(service.clone()).await;
        mode(&ai, "candidates").await;
        assert!(ai.classify(false).await.is_err());
        let status = ai.status().await.unwrap();
        assert!(!status.running);
        assert!(status.last_error.unwrap().contains("quota"));
        assert_eq!(status.applied, 1);
        assert_eq!(
            repo.snapshot()
                .await
                .unwrap()
                .emails
                .iter()
                .filter(|email| email.classification_source == "gemini")
                .count(),
            1
        );
        let resumed = ai.classify(false).await.unwrap();
        assert_eq!(resumed.requests, 1);
        assert!(resumed.last_message.unwrap().contains("unsuccessful"));
        assert_eq!(service.calls.load(Ordering::SeqCst), 3);
        assert_eq!(ai.classify(true).await.unwrap().requests, 3);
    }

    #[tokio::test]
    async fn saving_off_cancels_inflight_call_and_prevents_further_dispatch() {
        let service = Arc::new(FakeService {
            block: true,
            ..FakeService::default()
        });
        let (ai, repo, _dir) = setup(service.clone()).await;
        mode(&ai, "candidates").await;
        let task_ai = ai.clone();
        let task = tokio::spawn(async move { task_ai.classify(false).await });
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            service.started.notified(),
        )
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), mode(&ai, "off"))
            .await
            .unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.ai_usage().await.unwrap().succeeded, 0);
        ai.after_sync().await;
        assert_eq!(service.calls.load(Ordering::SeqCst), 1);
        assert!(!ai.status().await.unwrap().running);
    }

    #[tokio::test]
    async fn key_removal_disables_ai_and_keeps_oauth_and_cached_mail() {
        let service = Arc::new(FakeService::default());
        let (ai, repo, _dir) = setup(service.clone()).await;
        mode(&ai, "candidates").await;
        ai.credentials
            .set(
                "refresh-token-v1",
                &SecretValue::new(b"fictional-oauth".to_vec()),
            )
            .unwrap();
        let status = ai.remove_key().await.unwrap();
        assert!(!status.configured);
        assert_eq!(repo.app_settings().await.unwrap().ai_mode, "off");
        assert!(ai.credentials.get("refresh-token-v1").unwrap().is_some());
        assert_eq!(repo.snapshot().await.unwrap().emails.len(), 6);
    }
}
