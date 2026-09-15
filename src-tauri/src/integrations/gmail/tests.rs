use super::*;
use crate::services::credentials::{MemoryCredentialService, SecretValue, SecureCredentialService};
use crate::services::gmail::GmailError;

const ACCOUNT: &str = "candidate@fiction.example";
const CLIENT: &str = r#"{"installed":{"client_id":"fictional.apps.googleusercontent.com","client_secret":"fictional-client-secret","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","redirect_uris":["http://localhost"]}}"#;

async fn integration() -> (GmailIntegration, tempfile::TempDir) {
    let directory = tempfile::tempdir().expect("temporary workspace");
    let repository = Repository::open(directory.path().join("coordinator.sqlite3"))
        .await
        .expect("database");
    let store = Arc::new(MemoryCredentialService::default());
    let oauth = OAuthService::new(store.clone()).expect("OAuth client");
    oauth.configure(CLIENT).await.expect("fictional client");
    store
        .set(
            "refresh-token-v1",
            &SecretValue::new(
                br#"{"refresh_token":"fictional-refresh-token","refresh_expires_at":null}"#
                    .to_vec(),
            ),
        )
        .expect("fictional token");
    let integration = GmailIntegration {
        repository,
        oauth: Arc::new(oauth),
        service: Arc::new(GoogleGmailService::new().expect("HTTP client")),
        operation: Mutex::new(()),
        connection_cancellation: watch::channel(false).0,
        runtime: RwLock::new(RuntimeState::default()),
    };
    (integration, directory)
}

#[tokio::test]
async fn failed_profile_lookup_clears_unverified_tokens_and_keeps_client_setup() {
    let (integration, _directory) = integration().await;
    let result = integration
        .validate_connection(integration.connection_cancellation.subscribe(), async {
            Err(SyncError::Gmail(GmailError::Network))
        })
        .await;
    assert!(matches!(result, Err(SyncError::Gmail(GmailError::Network))));
    assert!(!integration.oauth.has_tokens().await.expect("token status"));
    let status = integration.status().await.expect("status");
    assert!(status.configured && !status.connected);
    assert!(status.account_email.is_none());
}

#[tokio::test]
async fn rejected_account_clears_new_tokens_without_rebinding_workspace() {
    let (integration, _directory) = integration().await;
    integration
        .repository
        .bind_gmail_account(ACCOUNT)
        .await
        .expect("existing binding");
    let result = integration
        .validate_connection(integration.connection_cancellation.subscribe(), async {
            Ok(GmailProfile {
                email_address: "other@fiction.example".into(),
                history_id: "100".into(),
            })
        })
        .await;
    assert!(matches!(result, Err(SyncError::Repository(_))));
    assert!(!integration.oauth.has_tokens().await.expect("token status"));
    assert_eq!(
        integration
            .status()
            .await
            .expect("status")
            .account_email
            .as_deref(),
        Some(ACCOUNT)
    );
}

#[tokio::test]
async fn validated_account_is_connected_without_starting_a_mail_sync() {
    let (integration, _directory) = integration().await;
    assert!(
        !integration
            .status()
            .await
            .expect("status before validation")
            .connected
    );
    integration
        .validate_connection(integration.connection_cancellation.subscribe(), async {
            Ok(GmailProfile {
                email_address: ACCOUNT.into(),
                history_id: "100".into(),
            })
        })
        .await
        .expect("valid account");
    let status = integration.status().await.expect("status");
    assert!(status.connected && status.configured);
    assert_eq!(status.account_email.as_deref(), Some(ACCOUNT));
    assert!(status.last_synced_at.is_none());
    assert!(integration
        .repository
        .snapshot()
        .await
        .expect("snapshot")
        .emails
        .is_empty());
}

#[tokio::test]
async fn cancel_during_profile_validation_removes_new_tokens_promptly() {
    let (integration, _directory) = integration().await;
    integration
        .set_runtime(Some(GmailPhase::Connecting), None)
        .expect("runtime");
    let validation = integration.validate_connection(
        integration.connection_cancellation.subscribe(),
        std::future::pending(),
    );
    let cancel = async {
        tokio::task::yield_now().await;
        integration.cancel_connection().await.expect("cancel");
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(validation, cancel)
    })
    .await
    .expect("prompt cancellation");
    assert!(matches!(
        result,
        Err(SyncError::OAuth(OAuthError::Cancelled))
    ));
    assert!(!integration.oauth.has_tokens().await.expect("token status"));
    assert!(integration
        .repository
        .gmail_sync_state()
        .await
        .expect("binding")
        .is_none());
}

#[tokio::test]
async fn reconnect_required_clears_unusable_tokens_and_disconnect_retains_account() {
    let (integration, _directory) = integration().await;
    integration
        .repository
        .bind_gmail_account(ACCOUNT)
        .await
        .expect("binding");
    integration
        .record_failure(&SyncError::Gmail(GmailError::Unauthorized))
        .await
        .expect("record failure");
    let status = integration.status().await.expect("status");
    assert_eq!(status.phase, GmailPhase::ReconnectRequired);
    assert!(!status.connected);
    assert!(!integration
        .oauth
        .has_tokens()
        .await
        .expect("cleared tokens"));
    let status = integration.disconnect().await.expect("disconnect");
    assert_eq!(status.phase, GmailPhase::Disconnected);
    assert_eq!(status.account_email.as_deref(), Some(ACCOUNT));
    assert!(status.last_error.is_none());
}
