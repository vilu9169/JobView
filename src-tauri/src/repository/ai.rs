//! AI requests run outside database transactions. Only validated results enter
//! the cache; applying one rechecks the email so concurrent corrections win.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{email_by_id, events, linked_application, validation, Repository};
use crate::{
    domain::{AppSettings, Email},
    error::{AppError, AppResult},
    services::{
        classification::ClassificationResult,
        gemini::{validate_model, validate_result},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiSettings {
    pub model: String,
    pub max_requests_per_run: i64,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            model: "gemini-2.5-flash-lite".into(),
            max_requests_per_run: 50,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AiUsage {
    pub requests: i64,
    pub succeeded: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_hits: i64,
}

#[derive(Debug, Clone)]
pub struct CachedAiClassification {
    pub result: ClassificationResult,
    pub model: String,
    pub prompt_version: String,
}

#[derive(FromRow)]
struct CacheRow {
    result_json: String,
    model: String,
    prompt_version: String,
}

fn validate_classification(result: &ClassificationResult) -> AppResult<()> {
    validate_result(result)
        .map_err(|_| AppError::Validation("The AI classification did not pass validation.".into()))
}

impl Repository {
    pub async fn app_settings(&self) -> AppResult<AppSettings> {
        Ok(sqlx::query_as("SELECT * FROM app_settings WHERE id = 1")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn ai_settings(&self) -> AppResult<AiSettings> {
        Ok(sqlx::query_as("SELECT * FROM ai_settings WHERE id = 1")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn save_ai_settings(&self, settings: AiSettings) -> AppResult<AiSettings> {
        validate_model(&settings.model)
            .map_err(|_| AppError::Validation("Choose a supported Gemini model.".into()))?;
        if !(1..=500).contains(&settings.max_requests_per_run) {
            return Err(AppError::Validation(
                "The AI request limit must be between 1 and 500.".into(),
            ));
        }
        sqlx::query("UPDATE ai_settings SET model = ?, max_requests_per_run = ? WHERE id = 1")
            .bind(&settings.model)
            .bind(settings.max_requests_per_run)
            .execute(&self.pool)
            .await?;
        Ok(settings)
    }

    pub async fn ai_usage(&self) -> AppResult<AiUsage> {
        Ok(sqlx::query_as("SELECT * FROM ai_usage WHERE id = 1")
            .fetch_one(&self.pool)
            .await?)
    }

    /// Cached results remain reusable after changing the selected model. A new
    /// model is used only for uncached content or an explicit forced refresh.
    pub async fn cached_ai_classification(
        &self,
        content_hash: &str,
    ) -> AppResult<Option<CachedAiClassification>> {
        let row = sqlx::query_as::<_, CacheRow>(
            "SELECT result_json, model, prompt_version FROM ai_classification_cache WHERE content_hash = ?",
        )
        .bind(content_hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let Ok(result) = serde_json::from_str::<ClassificationResult>(&row.result_json) else {
            return Ok(None);
        };
        if validate_result(&result).is_err()
            || validate_model(&row.model).is_err()
            || row.prompt_version.is_empty()
        {
            return Ok(None);
        }
        Ok(Some(CachedAiClassification {
            result,
            model: row.model,
            prompt_version: row.prompt_version,
        }))
    }

    /// Count before sending so interruptions and API failures remain visible.
    pub async fn record_ai_attempt(&self, content_hash: &str) -> AppResult<()> {
        if content_hash.is_empty() || content_hash.len() > 256 {
            return Err(AppError::Validation(
                "The AI request metadata did not pass validation.".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE ai_usage SET requests = requests + 1 WHERE id = 1")
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO ai_classification_attempts (content_hash, last_attempt_at) VALUES (?, ?) ON CONFLICT(content_hash) DO UPDATE SET attempts = attempts + 1, last_attempt_at = excluded.last_attempt_at")
            .bind(content_hash).bind(validation::now()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn has_ai_attempt(&self, content_hash: &str) -> AppResult<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM ai_classification_attempts WHERE content_hash = ?)",
        )
        .bind(content_hash)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn store_ai_classification(
        &self,
        content_hash: &str,
        model: &str,
        prompt_version: &str,
        result: &ClassificationResult,
        input_tokens: i64,
        output_tokens: i64,
    ) -> AppResult<()> {
        validate_classification(result)?;
        validate_model(model)
            .map_err(|_| AppError::Validation("Choose a supported Gemini model.".into()))?;
        if content_hash.is_empty()
            || content_hash.len() > 256
            || prompt_version.is_empty()
            || prompt_version.len() > 80
            || !(0..=10_000_000).contains(&input_tokens)
            || !(0..=10_000_000).contains(&output_tokens)
        {
            return Err(AppError::Validation(
                "The AI result metadata did not pass validation.".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO ai_classification_cache (content_hash, model, prompt_version, result_json, created_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(content_hash) DO UPDATE SET model = excluded.model, prompt_version = excluded.prompt_version, result_json = excluded.result_json, created_at = excluded.created_at")
            .bind(content_hash).bind(model).bind(prompt_version)
            .bind(serde_json::to_string(result)?).bind(validation::now())
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE ai_usage SET succeeded = succeeded + 1, input_tokens = input_tokens + ?, output_tokens = output_tokens + ? WHERE id = 1")
            .bind(input_tokens).bind(output_tokens).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn record_ai_cache_hit(&self) -> AppResult<()> {
        sqlx::query("UPDATE ai_usage SET cache_hits = cache_hits + 1 WHERE id = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Content changes, ignored emails and manual classification corrections
    /// invalidate an in-flight result. Links, completed actions and manual job
    /// fields are retained; only inferred timeline evidence is synchronized.
    pub async fn apply_ai_classification(
        &self,
        email_id: &str,
        expected_hash: &str,
        result: &ClassificationResult,
    ) -> AppResult<bool> {
        validate_classification(result)?;
        if result.confidence < 0.5 {
            return Ok(false);
        }
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query_as::<_, Email>("SELECT * FROM emails WHERE id = ?")
            .bind(email_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(current) = current else {
            return Ok(false);
        };
        if current.content_hash != expected_hash
            || current.manual_override
            || current.ignored
            || current.remote_deleted
        {
            return Ok(false);
        }
        if current.classification_source == "gemini"
            && current.category == result.category
            && current.classification_confidence == result.confidence
            && current.requires_action == result.requires_action
            && current.suggested_action == result.suggested_action
            && current.is_job_related == result.is_job_related
            && current.reasoning_code == result.reasoning_code
            && current.extracted_company == result.company
            && current.extracted_role == result.role
            && current.suggested_stage == result.suggested_stage
            && current.event_type == result.event_type
            && current.deadline == result.deadline
        {
            return Ok(false);
        }
        let timestamp = validation::now();
        sqlx::query("UPDATE emails SET category = ?, classification_confidence = ?, classification_source = 'gemini', classified_at = ?, requires_action = ?, suggested_action = ?, is_job_related = ?, reasoning_code = ?, extracted_company = ?, extracted_role = ?, suggested_stage = ?, event_type = ?, deadline = ?, updated_at = ? WHERE id = ?")
            .bind(&result.category).bind(result.confidence).bind(&timestamp).bind(result.requires_action)
            .bind(&result.suggested_action).bind(result.is_job_related).bind(&result.reasoning_code)
            .bind(&result.company).bind(&result.role).bind(&result.suggested_stage).bind(&result.event_type)
            .bind(&result.deadline).bind(&timestamp).bind(email_id).execute(&mut *tx).await?;
        if let Some(application_id) = linked_application(&mut tx, email_id).await? {
            let email = email_by_id(&mut tx, email_id).await?;
            events::sync_email(&mut tx, &application_id, &email, &timestamp).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn ai_emails(&self) -> AppResult<Vec<Email>> {
        Ok(sqlx::query_as("SELECT * FROM emails WHERE manual_override = 0 AND ignored = 0 ORDER BY received_at DESC, id")
            .fetch_all(&self.pool).await?)
    }

    pub async fn ai_email(&self, email_id: &str) -> AppResult<Option<Email>> {
        Ok(sqlx::query_as("SELECT * FROM emails WHERE id = ?")
            .bind(email_id)
            .fetch_optional(&self.pool)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApplicationInput, EmailDisposition};
    use tempfile::TempDir;

    async fn database() -> (Repository, TempDir) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let repository = Repository::open(directory.path().join("workspace.sqlite3"))
            .await
            .expect("migrate database");
        (repository, directory)
    }

    fn classification() -> ClassificationResult {
        ClassificationResult {
            is_job_related: true,
            category: "interview".into(),
            event_type: Some("interview_requested".into()),
            company: Some("Fiction Labs".into()),
            role: Some("Engineer".into()),
            suggested_stage: Some("interview".into()),
            requires_action: true,
            suggested_action: Some("Confirm interview availability".into()),
            deadline: None,
            confidence: 0.88,
            reasoning_code: "personal_interview".into(),
        }
    }

    fn application() -> ApplicationInput {
        ApplicationInput {
            id: None,
            company: "Fiction Labs".into(),
            role: "Engineer".into(),
            location: "Stockholm".into(),
            job_url: String::new(),
            source: "Manual".into(),
            applied_at: Some("2026-09-01".into()),
            current_stage: "applied".into(),
            next_action: Some("My manually planned follow-up".into()),
            next_action_due_at: Some("2026-09-15".into()),
            notes: "My notes remain unchanged".into(),
            archived: false,
            source_email_id: None,
        }
    }

    #[tokio::test]
    async fn ai_migration_preserves_existing_settings_and_starts_disabled() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("upgrade.sqlite3");
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .expect("legacy database");
        let all = sqlx::migrate!("./migrations");
        let mut legacy = sqlx::migrate::Migrator::DEFAULT;
        legacy.migrations = std::borrow::Cow::Owned(
            all.iter()
                .filter(|migration| migration.version < 3)
                .cloned()
                .collect(),
        );
        legacy.run(&pool).await.expect("legacy migrations");
        sqlx::query("UPDATE app_settings SET sync_email_limit = 1250, local_confidence_accept_threshold = 0.85, local_confidence_gemini_threshold = 0.4 WHERE id = 1")
            .execute(&pool).await.expect("user settings");
        pool.close().await;

        let repository = Repository::open(&path).await.expect("upgrade");
        let settings = repository.app_settings().await.expect("settings");
        assert_eq!(settings.sync_email_limit, 1250);
        assert_eq!(settings.local_confidence_accept_threshold, 0.85);
        assert_eq!(settings.local_confidence_gemini_threshold, 0.4);
        assert_eq!(settings.ai_mode, "off");
        assert_eq!(
            repository.ai_settings().await.unwrap().max_requests_per_run,
            50
        );
        for mode in ["uncertain", "candidates", "off"] {
            let updated = repository
                .save_settings(AppSettings {
                    ai_mode: mode.into(),
                    ..settings.clone()
                })
                .await
                .expect("new modes persist");
            assert_eq!(updated.settings.ai_mode, mode);
        }
        assert!(
            sqlx::query("UPDATE app_settings SET ai_mode = 'everything' WHERE id = 1")
                .execute(&repository.pool)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn settings_and_cache_reject_invalid_data_without_writes() {
        let (repository, _directory) = database().await;
        for settings in [
            AiSettings {
                model: "https://untrusted.example/endpoint".into(),
                ..AiSettings::default()
            },
            AiSettings {
                max_requests_per_run: 0,
                ..AiSettings::default()
            },
            AiSettings {
                max_requests_per_run: 501,
                ..AiSettings::default()
            },
        ] {
            assert!(repository.save_ai_settings(settings).await.is_err());
        }
        let mut result = classification();
        result.confidence = 1.01;
        assert!(repository
            .store_ai_classification("hash", "gemini-2.5-flash-lite", "v1", &result, 20, 10)
            .await
            .is_err());
        assert!(repository
            .store_ai_classification(
                "hash",
                "gemini-2.5-flash-lite",
                "v1",
                &classification(),
                -1,
                10
            )
            .await
            .is_err());
        assert!(repository
            .cached_ai_classification("hash")
            .await
            .unwrap()
            .is_none());
        let usage = repository.ai_usage().await.unwrap();
        assert_eq!(usage.requests, 0);
        assert_eq!(usage.succeeded, 0);
        assert_eq!(
            repository.ai_settings().await.unwrap().model,
            "gemini-2.5-flash-lite"
        );
    }

    #[tokio::test]
    async fn cache_usage_and_failed_attempts_survive_restart() {
        let (repository, directory) = database().await;
        repository.record_ai_attempt("failed-hash").await.unwrap();
        repository.record_ai_attempt("valid-hash").await.unwrap();
        repository
            .store_ai_classification(
                "valid-hash",
                "gemini-2.5-flash-lite",
                "prompt-v1",
                &classification(),
                320,
                95,
            )
            .await
            .unwrap();
        repository.record_ai_cache_hit().await.unwrap();
        repository.pool.close().await;
        let repository = Repository::open(directory.path().join("workspace.sqlite3"))
            .await
            .unwrap();
        assert!(repository.has_ai_attempt("failed-hash").await.unwrap());
        assert!(!repository.has_ai_attempt("new-hash").await.unwrap());
        let cached = repository
            .cached_ai_classification("valid-hash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached.result.category, "interview");
        assert_eq!(cached.model, "gemini-2.5-flash-lite");
        assert_eq!(cached.prompt_version, "prompt-v1");
        let usage = repository.ai_usage().await.unwrap();
        assert_eq!(
            (usage.requests, usage.succeeded, usage.cache_hits),
            (2, 1, 1)
        );
        assert_eq!((usage.input_tokens, usage.output_tokens), (320, 95));
        sqlx::query("UPDATE ai_classification_cache SET result_json = '{}' WHERE content_hash = 'valid-hash'")
            .execute(&repository.pool).await.unwrap();
        assert!(repository
            .cached_ai_classification("valid-hash")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn in_flight_result_cannot_override_manual_correction_ignore_or_changed_content() {
        let (repository, _directory) = database().await;
        let imported = repository.load_fixtures().await.unwrap();
        let email = &imported.emails[0];
        let mut uncertain = classification();
        uncertain.confidence = 0.49;
        assert!(!repository
            .apply_ai_classification(&email.id, &email.content_hash, &uncertain)
            .await
            .unwrap());
        assert!(!repository
            .apply_ai_classification(&email.id, "stale-hash", &classification())
            .await
            .unwrap());
        assert!(!repository
            .apply_ai_classification("missing-email", &email.content_hash, &classification())
            .await
            .unwrap());
        repository
            .set_email_disposition(&email.id, EmailDisposition::Ignored)
            .await
            .unwrap();
        assert!(!repository
            .apply_ai_classification(&email.id, &email.content_hash, &classification())
            .await
            .unwrap());
        assert!(!repository
            .ai_emails()
            .await
            .unwrap()
            .iter()
            .any(|candidate| candidate.id == email.id));
        repository
            .set_email_disposition(&email.id, EmailDisposition::NotJob)
            .await
            .unwrap();
        assert!(!repository
            .apply_ai_classification(&email.id, &email.content_hash, &classification())
            .await
            .unwrap());
        let current = repository.ai_email(&email.id).await.unwrap().unwrap();
        assert_eq!(current.classification_source, "manual");
        assert!(!current.is_job_related);
    }

    #[tokio::test]
    async fn ai_reclassification_retains_manual_work_and_audits_changed_inferred_events() {
        let (repository, _directory) = database().await;
        let imported = repository.load_fixtures().await.unwrap();
        let email = imported
            .emails
            .iter()
            .find(|email| {
                email.is_job_related && email.event_type.as_deref() != Some("interview_requested")
            })
            .unwrap();
        let saved = repository.save_application(application()).await.unwrap();
        let application_id = saved.applications[0].id.clone();
        repository
            .link_email(&email.id, &application_id)
            .await
            .unwrap();
        repository
            .resolve_action(None, Some(&email.id))
            .await
            .unwrap();
        let mut manual = application();
        manual.id = Some(application_id.clone());
        manual.current_stage = "technical_test".into();
        repository.save_application(manual).await.unwrap();

        assert!(repository
            .apply_ai_classification(&email.id, &email.content_hash, &classification())
            .await
            .unwrap());
        let snapshot = repository.snapshot().await.unwrap();
        let current = snapshot
            .emails
            .iter()
            .find(|candidate| candidate.id == email.id)
            .unwrap();
        assert_eq!(current.classification_source, "gemini");
        assert!(current.action_completed);
        assert_eq!(snapshot.links[0].association_source, "manual");
        let job = &snapshot.applications[0];
        assert_eq!(job.current_stage, "technical_test");
        assert_eq!(
            job.next_action.as_deref(),
            Some("My manually planned follow-up")
        );
        assert_eq!(job.next_action_due_at.as_deref(), Some("2026-09-15"));
        assert_eq!(job.notes, "My notes remain unchanged");
        let active = snapshot
            .events
            .iter()
            .filter(|event| {
                event.source_email_id.as_deref() == Some(&email.id)
                    && event.event_source == "gemini"
            })
            .count();
        assert_eq!(active, 1);
        let superseded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM application_events WHERE source_email_id = ? AND event_source = 'local_rules' AND superseded_at IS NOT NULL")
            .bind(&email.id).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(superseded, 1);

        // Reapplying a cache hit and rebuilding local rules are idempotent for
        // Gemini evidence and do not manufacture another event.
        let reused = repository
            .apply_ai_classification(&email.id, &email.content_hash, &classification())
            .await
            .unwrap();
        assert!(!reused);
        let rebuilt = repository.rebuild_classifications().await.unwrap();
        let unchanged = rebuilt
            .emails
            .iter()
            .find(|item| item.id == email.id)
            .unwrap();
        assert_eq!(unchanged.classified_at, current.classified_at);
        assert_eq!(unchanged.updated_at, current.updated_at);
        assert_eq!(
            rebuilt
                .emails
                .iter()
                .find(|candidate| candidate.id == email.id)
                .unwrap()
                .classification_source,
            "gemini"
        );
        let inferred_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM application_events WHERE source_email_id = ? AND event_source = 'gemini'")
            .bind(&email.id).fetch_one(&repository.pool).await.unwrap();
        assert_eq!(inferred_count, 1);

        let unrelated = ClassificationResult {
            is_job_related: false,
            category: "promotion".into(),
            event_type: None,
            company: None,
            role: None,
            suggested_stage: None,
            requires_action: false,
            suggested_action: None,
            deadline: None,
            confidence: 0.95,
            reasoning_code: "commercial_promotion".into(),
        };
        repository
            .apply_ai_classification(&email.id, &email.content_hash, &unrelated)
            .await
            .unwrap();
        let final_state = repository.snapshot().await.unwrap();
        assert_eq!(final_state.links.len(), 1, "explicit links are user-owned");
        assert_eq!(final_state.applications[0].current_stage, "technical_test");
        assert!(!final_state
            .events
            .iter()
            .any(|event| event.source_email_id.as_deref() == Some(&email.id)
                && event.event_source == "gemini"));
    }
}
