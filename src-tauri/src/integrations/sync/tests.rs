use super::*;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use crate::{
    domain::{ApplicationInput, EmailDisposition},
    services::gmail::*,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use tempfile::TempDir;

const ACCOUNT: &str = "candidate@fiction.example";

#[derive(Default)]
struct FakeTokens {
    calls: Mutex<Vec<bool>>,
}
#[async_trait]
impl TokenProvider for FakeTokens {
    async fn token(&self, force_refresh: bool) -> Result<SecretValue, OAuthError> {
        self.calls.lock().expect("tokens lock").push(force_refresh);
        Ok(SecretValue::new(b"fictional-oauth-token".to_vec()))
    }
}

struct FakeGmail {
    profile: GmailProfile,
    pages: Mutex<VecDeque<Result<MessagePage, GmailError>>>,
    history: Mutex<VecDeque<Result<HistoryPage, GmailError>>>,
    messages: Mutex<HashMap<String, VecDeque<Result<GmailMessage, GmailError>>>>,
    calls: Mutex<Vec<String>>,
}
impl Default for FakeGmail {
    fn default() -> Self {
        Self {
            profile: GmailProfile {
                email_address: ACCOUNT.into(),
                history_id: "100".into(),
            },
            pages: Mutex::new(VecDeque::new()),
            history: Mutex::new(VecDeque::new()),
            messages: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
        }
    }
}
impl FakeGmail {
    fn page(&self, ids: &[&str], next: Option<&str>) {
        self.pages
            .lock()
            .expect("page lock")
            .push_back(Ok(MessagePage {
                messages: ids
                    .iter()
                    .map(|id| MessageRef {
                        id: (*id).into(),
                        thread_id: "thread-fiction".into(),
                    })
                    .collect(),
                next_page_token: next.map(str::to_owned),
            }));
    }
    fn message(&self, id: &str, results: Vec<Result<GmailMessage, GmailError>>) {
        self.messages
            .lock()
            .expect("message lock")
            .insert(id.into(), results.into());
    }
    fn history(&self, changes: Vec<HistoryRecord>, next: Option<&str>, checkpoint: &str) {
        self.history
            .lock()
            .expect("history lock")
            .push_back(Ok(HistoryPage {
                history: changes,
                next_page_token: next.map(str::to_owned),
                history_id: Some(checkpoint.into()),
            }));
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls lock").clone()
    }
}
#[async_trait]
impl GmailService for FakeGmail {
    async fn profile(&self, _token: &str) -> Result<GmailProfile, GmailError> {
        Ok(self.profile.clone())
    }
    async fn list_messages(
        &self,
        _token: &str,
        page: Option<&str>,
        max: u32,
    ) -> Result<MessagePage, GmailError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("list:{}:{max}", page.unwrap_or("first")));
        self.pages
            .lock()
            .expect("pages")
            .pop_front()
            .expect("planned message page")
    }
    async fn get_message(
        &self,
        _token: &str,
        id: &str,
        metadata: bool,
    ) -> Result<GmailMessage, GmailError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("get:{id}:{metadata}"));
        let mut messages = self.messages.lock().expect("messages");
        let queue = messages.get_mut(id).expect("planned message response");
        if queue.len() == 1 {
            queue.front().expect("response").clone()
        } else {
            queue.pop_front().expect("response")
        }
    }
    async fn list_history(
        &self,
        _token: &str,
        start: &str,
        page: Option<&str>,
    ) -> Result<HistoryPage, GmailError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("history:{start}:{}", page.unwrap_or("first")));
        self.history
            .lock()
            .expect("history")
            .pop_front()
            .expect("planned history page")
    }
}

async fn database() -> (Repository, TempDir) {
    let directory = tempfile::tempdir().expect("temp database");
    let repository = Repository::open(directory.path().join("sync.sqlite3"))
        .await
        .expect("database migrations");
    (repository, directory)
}
fn mail(id: &str) -> GmailMessage {
    GmailMessage { id: id.into(), thread_id: format!("thread-{id}"), history_id: Some("90".into()),
        internal_date: Some("1788850800000".into()), label_ids: vec!["INBOX".into(), "UNREAD".into()],
        snippet: "Interview invitation for Frontend Engineer at Northstar Labs".into(),
        payload: Some(MessagePart { mime_type: "text/plain".into(),
            headers: vec![
                MessageHeader { name: "From".into(), value: "Mira <hiring@northstar.example>".into() },
                MessageHeader { name: "To".into(), value: ACCOUNT.into() },
                MessageHeader { name: "Subject".into(), value: "Interview invitation — Frontend Engineer at Northstar Labs".into() },
            ],
            body: MessagePartBody { data: Some(URL_SAFE_NO_PAD.encode("Company: Northstar Labs\nRole: Frontend Engineer\nPlease reply with your availability to schedule an interview by 2026-09-15.")), ..Default::default() },
            ..Default::default()
        }),
    }
}
fn reference(id: &str) -> MessageRef {
    MessageRef {
        id: id.into(),
        thread_id: format!("thread-{id}"),
    }
}
fn application() -> ApplicationInput {
    ApplicationInput {
        id: None,
        company: "Northstar Labs".into(),
        role: "Frontend Engineer".into(),
        location: String::new(),
        job_url: String::new(),
        source: "Manual".into(),
        applied_at: Some("2026-09-01".into()),
        current_stage: "applied".into(),
        next_action: None,
        next_action_due_at: None,
        notes: "Fictional sync test".into(),
        archived: false,
        source_email_id: None,
    }
}
async fn seed(repository: &Repository, ids: &[&str]) {
    repository
        .bind_gmail_account(ACCOUNT)
        .await
        .expect("bind account");
    for id in ids {
        repository
            .cache_gmail_message(ACCOUNT, &mail(id))
            .await
            .expect("seed email");
    }
    repository
        .finish_gmail_sync("10", 500)
        .await
        .expect("initial checkpoint");
}
async fn labels(directory: &TempDir, id: &str) -> Vec<String> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(directory.path().join("sync.sqlite3")),
        )
        .await
        .expect("read cache");
    let json: String =
        sqlx::query_scalar("SELECT gmail_labels FROM emails WHERE gmail_message_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("labels");
    serde_json::from_str(&json).expect("labels json")
}

#[tokio::test]
async fn initial_sync_pages_to_limit_and_classifies_locally_without_creating_jobs() {
    let (repository, _directory) = database().await;
    let mut settings = repository.snapshot().await.expect("snapshot").settings;
    settings.sync_email_limit = 2;
    repository.save_settings(settings).await.expect("settings");
    let service = FakeGmail::default();
    service.page(&["a1"], Some("second"));
    service.page(&["a2", "a3"], None);
    service.message("a1", vec![Ok(mail("a1"))]);
    service.message("a2", vec![Ok(mail("a2"))]);
    let result = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("sync");
    assert_eq!(result.imported, 2);
    let snapshot = repository.snapshot().await.expect("snapshot");
    assert!(snapshot.applications.is_empty());
    assert_eq!(snapshot.emails.len(), 2);
    assert!(snapshot.emails.iter().all(|email| email.is_job_related
        && email.requires_action
        && email.gmail_account_id.as_deref() == Some(ACCOUNT)));
    let state = repository
        .gmail_sync_state()
        .await
        .expect("state")
        .expect("bound state");
    assert_eq!(state.gmail_history_id.as_deref(), Some("100"));
    assert_eq!(state.initial_sync_limit, 2);
    assert!(service.calls().contains(&"list:second:1".into()));
    assert!(!service.calls().iter().any(|call| call.contains("a3")));
}

#[tokio::test]
async fn partial_network_failure_keeps_successes_and_retries_without_refetching_their_bodies() {
    let (repository, _directory) = database().await;
    let service = FakeGmail::default();
    service.page(&["a1", "a2"], None);
    service.message("a1", vec![Ok(mail("a1"))]);
    service.message("a2", vec![Err(GmailError::Network)]);
    assert!(matches!(
        synchronize(&repository, &service, &FakeTokens::default(), &|_| {}).await,
        Err(SyncError::Gmail(GmailError::Network))
    ));
    assert_eq!(
        repository.snapshot().await.expect("snapshot").emails.len(),
        1
    );
    assert!(repository
        .gmail_sync_state()
        .await
        .expect("state")
        .expect("bound")
        .gmail_history_id
        .is_none());
    service.page(&["a1", "a2"], None);
    service.message("a2", vec![Ok(mail("a2"))]);
    let result = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("retry");
    assert_eq!(result.imported, 1);
    assert_eq!(
        service
            .calls()
            .iter()
            .filter(|call| *call == "get:a1:false")
            .count(),
        1
    );
    assert!(service.calls().contains(&"get:a1:true".into()));
    assert_eq!(
        repository.snapshot().await.expect("snapshot").emails.len(),
        2
    );
}

#[tokio::test]
async fn incremental_sync_applies_labels_deletions_and_new_mail_without_refetching_cached_bodies() {
    let (repository, directory) = database().await;
    seed(&repository, &["a1", "a2"]).await;
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            labels_added: vec![HistoryLabels {
                message: reference("a1"),
                label_ids: vec!["STARRED".into()],
            }],
            labels_removed: vec![HistoryLabels {
                message: reference("a1"),
                label_ids: vec!["UNREAD".into()],
            }],
            ..Default::default()
        }],
        Some("next"),
        "20",
    );
    service.history(
        vec![HistoryRecord {
            messages_added: vec![HistoryMessage {
                message: reference("a3"),
            }],
            messages_deleted: vec![HistoryMessage {
                message: reference("a2"),
            }],
            ..Default::default()
        }],
        None,
        "30",
    );
    service.message("a3", vec![Ok(mail("a3"))]);
    let result = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("incremental");
    assert_eq!(result.imported, 1);
    assert_eq!(labels(&directory, "a1").await, vec!["INBOX", "STARRED"]);
    assert!(repository
        .snapshot()
        .await
        .expect("snapshot")
        .emails
        .iter()
        .any(|email| email.gmail_message_id == "a2" && email.remote_deleted));
    assert_eq!(
        service
            .calls()
            .iter()
            .filter(|call| call.starts_with("get:"))
            .collect::<Vec<_>>(),
        vec![&"get:a3:false".to_string()]
    );
    assert!(!service.calls().iter().any(|call| call.starts_with("list:")));
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("30")
    );
}

#[tokio::test]
async fn incremental_sync_excludes_new_spam_and_trash_using_current_labels() {
    let (repository, _directory) = database().await;
    seed(&repository, &[]).await;
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            messages_added: ["spam", "trash", "archived"]
                .iter()
                .map(|id| HistoryMessage {
                    message: reference(id),
                })
                .collect(),
            // An older history label must not exclude a message already restored
            // according to the authoritative message snapshot.
            labels_added: vec![HistoryLabels {
                message: reference("archived"),
                label_ids: vec!["SPAM".into()],
            }],
            ..Default::default()
        }],
        None,
        "110",
    );
    for (id, label) in [
        ("spam", "SPAM"),
        ("trash", "TRASH"),
        ("archived", "STARRED"),
    ] {
        let mut message = mail(id);
        message.label_ids = vec![label.into()];
        service.message(id, vec![Ok(message)]);
    }
    let progress = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("incremental");
    assert_eq!(
        progress,
        SyncProgress {
            processed: 3,
            total: 3,
            imported: 1,
            skipped: 2,
            failed: 0
        }
    );
    let snapshot = repository.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.emails.len(), 1);
    assert_eq!(snapshot.emails[0].gmail_message_id, "archived");
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("110")
    );
}

#[tokio::test]
async fn initial_scan_skips_mail_moved_to_spam_or_trash_after_listing() {
    let (repository, _directory) = database().await;
    let service = FakeGmail::default();
    service.page(&["spam", "trash"], None);
    for (id, label) in [("spam", "SPAM"), ("trash", "TRASH")] {
        let mut message = mail(id);
        message.label_ids.push(label.into());
        service.message(id, vec![Ok(message)]);
    }
    let progress = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("initial sync");
    assert_eq!(progress.imported, 0);
    assert_eq!(progress.skipped, 2);
    assert!(repository
        .snapshot()
        .await
        .expect("snapshot")
        .emails
        .is_empty());
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("100")
    );
}

#[tokio::test]
async fn restoring_previously_excluded_mail_imports_it_without_a_message_added_event() {
    let (repository, _directory) = database().await;
    seed(&repository, &[]).await;
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            labels_removed: vec![
                HistoryLabels {
                    message: reference("spam"),
                    label_ids: vec!["SPAM".into()],
                },
                HistoryLabels {
                    message: reference("trash"),
                    label_ids: vec!["TRASH".into()],
                },
                HistoryLabels {
                    message: reference("moved-back"),
                    label_ids: vec!["TRASH".into()],
                },
            ],
            ..Default::default()
        }],
        None,
        "110",
    );
    service.message("spam", vec![Ok(mail("spam"))]);
    service.message("trash", vec![Ok(mail("trash"))]);
    let mut moved_back = mail("moved-back");
    moved_back.label_ids = vec!["SPAM".into()];
    service.message("moved-back", vec![Ok(moved_back)]);
    let progress = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("restored mail");
    assert_eq!(progress.imported, 2);
    assert_eq!(progress.skipped, 1);
    assert_eq!(
        repository.snapshot().await.expect("snapshot").emails.len(),
        2
    );
    assert!(!repository
        .gmail_message_is_cached(ACCOUNT, "moved-back")
        .await
        .expect("excluded cache"));
}

#[tokio::test]
async fn cached_mail_moved_to_spam_or_trash_is_retained_without_refetching_bodies() {
    let (repository, directory) = database().await;
    seed(&repository, &["spam", "trash"]).await;
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            labels_added: vec![
                HistoryLabels {
                    message: reference("spam"),
                    label_ids: vec!["SPAM".into()],
                },
                HistoryLabels {
                    message: reference("trash"),
                    label_ids: vec!["TRASH".into()],
                },
            ],
            labels_removed: ["spam", "trash"]
                .iter()
                .map(|id| HistoryLabels {
                    message: reference(id),
                    label_ids: vec!["INBOX".into()],
                })
                .collect(),
            ..Default::default()
        }],
        None,
        "110",
    );
    synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("moved cached mail");
    assert_eq!(labels(&directory, "spam").await, vec!["SPAM", "UNREAD"]);
    assert_eq!(labels(&directory, "trash").await, vec!["TRASH", "UNREAD"]);
    let snapshot = repository.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.emails.len(), 2);
    assert!(snapshot.emails.iter().all(|email| !email.remote_deleted));
    assert_eq!(service.calls(), vec!["history:10:first"]);
}

#[tokio::test]
async fn history_page_failure_does_not_advance_or_partially_apply_a_collected_plan() {
    let (repository, _directory) = database().await;
    seed(&repository, &["a1"]).await;
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            messages_deleted: vec![HistoryMessage {
                message: reference("a1"),
            }],
            ..Default::default()
        }],
        Some("next"),
        "20",
    );
    service
        .history
        .lock()
        .expect("history")
        .push_back(Err(GmailError::Network));
    assert!(
        synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
            .await
            .is_err()
    );
    assert!(!repository.snapshot().await.expect("snapshot").emails[0].remote_deleted);
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("10")
    );
}

#[tokio::test]
async fn expired_history_recovers_recent_mail_and_reconciles_cached_metadata_including_deletions() {
    let (repository, _directory) = database().await;
    seed(&repository, &["a1", "old"]).await;
    let service = FakeGmail::default();
    service
        .history
        .lock()
        .expect("history")
        .push_back(Err(GmailError::NotFound));
    service.page(&["a2", "a1"], None);
    service.message("a1", vec![Ok(mail("a1"))]);
    service.message("a2", vec![Ok(mail("a2"))]);
    service.message("old", vec![Err(GmailError::NotFound)]);
    synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("recovery");
    assert!(service.calls().contains(&"get:a1:true".into()));
    assert!(service.calls().contains(&"get:old:true".into()));
    assert!(repository
        .snapshot()
        .await
        .expect("snapshot")
        .emails
        .iter()
        .any(|email| email.gmail_message_id == "old" && email.remote_deleted));
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("100")
    );
}

#[tokio::test]
async fn empty_history_is_a_cheap_noop_without_any_message_requests() {
    let (repository, _directory) = database().await;
    seed(&repository, &["a1"]).await;
    let service = FakeGmail::default();
    service.history(vec![], None, "100");
    let result = synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("empty incremental");
    assert_eq!(result.total, 0);
    assert_eq!(service.calls(), vec!["history:10:first"]);
}

#[tokio::test]
async fn duplicate_history_page_cursor_is_rejected_with_checkpoint_intact() {
    let (repository, _directory) = database().await;
    seed(&repository, &[]).await;
    let service = FakeGmail::default();
    service.history(vec![], Some("repeat"), "20");
    service.history(vec![], Some("repeat"), "20");
    assert!(matches!(
        synchronize(&repository, &service, &FakeTokens::default(), &|_| {}).await,
        Err(SyncError::InvalidPagination)
    ));
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("10")
    );
}

#[tokio::test]
async fn unauthorized_request_refreshes_once_and_uses_the_same_request() {
    let (repository, _directory) = database().await;
    let service = FakeGmail::default();
    service.page(&["a1"], None);
    service.message("a1", vec![Err(GmailError::Unauthorized), Ok(mail("a1"))]);
    let auth = FakeTokens::default();
    synchronize(&repository, &service, &auth, &|_| {})
        .await
        .expect("refresh retry");
    assert_eq!(
        auth.calls
            .lock()
            .expect("calls")
            .iter()
            .filter(|force| **force)
            .count(),
        1
    );
    assert_eq!(
        service
            .calls()
            .iter()
            .filter(|call| *call == "get:a1:false")
            .count(),
        2
    );
}

#[tokio::test]
async fn invalid_message_cannot_advance_checkpoint_but_other_messages_are_preserved() {
    let (repository, _directory) = database().await;
    let service = FakeGmail::default();
    service.page(&["a1", "a2"], None);
    service.message("a1", vec![Ok(mail("mismatched"))]);
    service.message("a2", vec![Ok(mail("a2"))]);
    assert!(matches!(
        synchronize(&repository, &service, &FakeTokens::default(), &|_| {}).await,
        Err(SyncError::Partial)
    ));
    assert_eq!(
        repository.snapshot().await.expect("snapshot").emails.len(),
        1
    );
    assert!(repository
        .gmail_sync_state()
        .await
        .expect("state")
        .expect("bound")
        .gmail_history_id
        .is_none());
}

#[tokio::test]
async fn another_account_is_rejected_even_after_disconnect_and_cached_data_remains() {
    let (repository, _directory) = database().await;
    seed(&repository, &["a1"]).await;
    repository
        .set_gmail_sync_status("disconnected", None)
        .await
        .expect("disconnect metadata");
    let mut service = FakeGmail::default();
    service.profile.email_address = "different@fiction.example".into();
    assert!(
        synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
            .await
            .is_err()
    );
    assert!(service.calls().is_empty());
    assert_eq!(
        repository.snapshot().await.expect("snapshot").emails.len(),
        1
    );
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .email_address,
        ACCOUNT
    );
}

#[tokio::test]
async fn syncing_and_changed_content_preserve_manual_classification_stage_links_and_completed_actions(
) {
    let (repository, _directory) = database().await;
    seed(&repository, &["a1", "a2"]).await;
    let snapshot = repository
        .save_application(application())
        .await
        .expect("application");
    let job_id = snapshot.applications[0].id.clone();
    let first = snapshot
        .emails
        .iter()
        .find(|email| email.gmail_message_id == "a1")
        .expect("first")
        .id
        .clone();
    let second = snapshot
        .emails
        .iter()
        .find(|email| email.gmail_message_id == "a2")
        .expect("second")
        .id
        .clone();
    repository
        .link_email(&first, &job_id)
        .await
        .expect("manual link");
    let mut edit = application();
    edit.id = Some(job_id.clone());
    edit.current_stage = "preparing".into();
    repository
        .save_application(edit)
        .await
        .expect("manual stage");
    repository
        .resolve_action(None, Some(&first))
        .await
        .expect("complete action");
    repository
        .set_email_disposition(&second, EmailDisposition::NotJob)
        .await
        .expect("manual classification");
    let mut changed = mail("a2");
    changed.snippet = "A revised fictional snippet".into();
    changed.payload.as_mut().expect("payload").body.data = Some(
        URL_SAFE_NO_PAD.encode("Unfortunately we decided not to proceed with your application."),
    );
    repository
        .cache_gmail_message(ACCOUNT, &changed)
        .await
        .expect("changed content");
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            messages_added: vec![HistoryMessage {
                message: reference("a1"),
            }],
            ..Default::default()
        }],
        None,
        "110",
    );
    synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("sync");
    let snapshot = repository.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.applications[0].current_stage, "preparing");
    assert!(snapshot.applications[0].stage_manually_set);
    assert_eq!(snapshot.links[0].association_source, "manual");
    assert!(
        snapshot
            .emails
            .iter()
            .find(|email| email.id == first)
            .expect("first")
            .action_completed
    );
    let corrected = snapshot
        .emails
        .iter()
        .find(|email| email.id == second)
        .expect("second");
    assert!(corrected.manual_override && !corrected.is_job_related);
}

#[tokio::test]
async fn increasing_limit_does_not_skip_pending_history_changes() {
    let (repository, directory) = database().await;
    seed(&repository, &["a1"]).await;
    let mut settings = repository.snapshot().await.expect("snapshot").settings;
    settings.sync_email_limit = 600;
    repository.save_settings(settings).await.expect("settings");
    let service = FakeGmail::default();
    service.history(
        vec![HistoryRecord {
            labels_removed: vec![HistoryLabels {
                message: reference("a1"),
                label_ids: vec!["UNREAD".into()],
            }],
            ..Default::default()
        }],
        None,
        "110",
    );
    service.page(&["a1"], None);
    let mut current = mail("a1");
    current.label_ids = vec!["INBOX".into()];
    service.message("a1", vec![Ok(current)]);
    synchronize(&repository, &service, &FakeTokens::default(), &|_| {})
        .await
        .expect("expanded scan");
    assert!(service
        .calls()
        .iter()
        .any(|call| call.starts_with("history:10:")));
    assert_eq!(labels(&directory, "a1").await, vec!["INBOX"]);
    assert_eq!(
        repository
            .gmail_sync_state()
            .await
            .expect("state")
            .expect("bound")
            .gmail_history_id
            .as_deref(),
        Some("110")
    );
}

#[tokio::test]
async fn local_job_edits_remain_available_while_sync_waits_on_gmail() {
    struct PausedGmail {
        inner: FakeGmail,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    #[async_trait]
    impl GmailService for PausedGmail {
        async fn profile(&self, token: &str) -> Result<GmailProfile, GmailError> {
            self.inner.profile(token).await
        }
        async fn list_messages(
            &self,
            token: &str,
            page: Option<&str>,
            max: u32,
        ) -> Result<MessagePage, GmailError> {
            self.inner.list_messages(token, page, max).await
        }
        async fn list_history(
            &self,
            token: &str,
            start: &str,
            page: Option<&str>,
        ) -> Result<HistoryPage, GmailError> {
            self.inner.list_history(token, start, page).await
        }
        async fn get_message(
            &self,
            token: &str,
            id: &str,
            metadata: bool,
        ) -> Result<GmailMessage, GmailError> {
            self.entered.notify_one();
            self.release.notified().await;
            self.inner.get_message(token, id, metadata).await
        }
    }
    let (repository, _directory) = database().await;
    let inner = FakeGmail::default();
    inner.page(&["a1"], None);
    inner.message("a1", vec![Ok(mail("a1"))]);
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let service = PausedGmail {
        inner,
        entered: entered.clone(),
        release: release.clone(),
    };
    let sync_repository = repository.clone();
    let task = tokio::spawn(async move {
        synchronize(&sync_repository, &service, &FakeTokens::default(), &|_| {}).await
    });
    entered.notified().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        repository.save_application(application()),
    )
    .await
    .expect("local edit remains responsive")
    .expect("local edit");
    release.notify_one();
    task.await.expect("sync task").expect("sync result");
    assert_eq!(
        repository
            .snapshot()
            .await
            .expect("snapshot")
            .applications
            .len(),
        1
    );
}
