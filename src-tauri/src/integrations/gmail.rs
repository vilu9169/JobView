//! Desktop integration coordinator. No tokens, OAuth responses or email bodies
//! are serializable through this boundary; only operational status crosses IPC.

use std::sync::{Arc, RwLock};

use serde::Serialize;
use tokio::sync::{watch, Mutex};

use crate::{
    domain::WorkspaceSnapshot,
    error::{AppError, AppResult},
    repository::Repository,
    services::{
        credentials::platform_secure_store,
        gmail::{GmailProfile, GmailService, GoogleGmailService},
        oauth::{OAuthError, OAuthService},
    },
};

use super::sync::{authorized, synchronize, token_text, SyncError, SyncProgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GmailPhase {
    NotConfigured,
    Disconnected,
    Connecting,
    Connected,
    Syncing,
    ReconnectRequired,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailStatus {
    pub configured: bool,
    pub connected: bool,
    pub credential_store_available: bool,
    pub client_id: Option<String>,
    pub account_email: Option<String>,
    pub phase: GmailPhase,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub processed: u32,
    pub total: u32,
    pub imported: u32,
    pub skipped: u32,
    pub failed: u32,
}

#[derive(Default, Clone)]
struct RuntimeState {
    phase: Option<GmailPhase>,
    last_error: Option<String>,
    progress: SyncProgress,
}

pub struct GmailIntegration {
    repository: Repository,
    oauth: Arc<OAuthService>,
    service: Arc<dyn GmailService>,
    operation: Mutex<()>,
    connection_cancellation: watch::Sender<bool>,
    runtime: RwLock<RuntimeState>,
}

impl GmailIntegration {
    pub fn new(repository: Repository) -> AppResult<Self> {
        let oauth = OAuthService::new(platform_secure_store()).map_err(safe_error)?;
        let service = GoogleGmailService::new().map_err(safe_error)?;
        Ok(Self {
            repository,
            oauth: Arc::new(oauth),
            service: Arc::new(service),
            operation: Mutex::new(()),
            connection_cancellation: watch::channel(false).0,
            runtime: RwLock::new(RuntimeState::default()),
        })
    }

    pub async fn status(&self) -> AppResult<GmailStatus> {
        let runtime = self.runtime.read().map_err(|_| state_error())?.clone();
        let persisted = self.repository.gmail_sync_state().await?;
        let configuration = self.oauth.configuration().await;
        let token_state = self.oauth.has_tokens().await;
        let credentials_error = configuration.as_ref().err().or(token_state.as_ref().err());
        let available = !matches!(credentials_error, Some(OAuthError::CredentialStore));
        let configured = matches!(&configuration, Ok(Some(_)));
        let needs_reconnect = credentials_error.is_some_and(|error| error.requires_reconnect())
            || runtime.phase == Some(GmailPhase::ReconnectRequired)
            || (runtime.phase.is_none()
                && persisted
                    .as_ref()
                    .is_some_and(|state| state.sync_status == "reconnect_required"));
        let connected = configured
            && persisted.is_some()
            && matches!(token_state, Ok(true))
            && !needs_reconnect;
        let phase = if matches!(
            runtime.phase,
            Some(GmailPhase::Connecting | GmailPhase::Syncing)
        ) {
            runtime.phase.unwrap_or(GmailPhase::Error)
        } else if !available {
            GmailPhase::Error
        } else if needs_reconnect {
            GmailPhase::ReconnectRequired
        } else if !configured {
            GmailPhase::NotConfigured
        } else {
            runtime.phase.unwrap_or(if connected {
                GmailPhase::Connected
            } else {
                GmailPhase::Disconnected
            })
        };
        let previous_failure = persisted.as_ref().filter(|state| state.sync_status == "error")
            .map(|_| "The previous Gmail sync did not complete. Cached emails are safe; choose Sync now to retry.".to_string());
        let last_error = credentials_error
            .map(ToString::to_string)
            .or(runtime.last_error)
            .or(previous_failure);
        Ok(GmailStatus {
            configured,
            connected,
            credential_store_available: available,
            client_id: configuration.ok().flatten().map(|config| config.client_id),
            account_email: persisted.as_ref().map(|state| state.email_address.clone()),
            phase,
            last_synced_at: persisted.and_then(|state| state.last_successful_sync_at),
            last_error,
            processed: runtime.progress.processed,
            total: runtime.progress.total,
            imported: runtime.progress.imported,
            skipped: runtime.progress.skipped,
            failed: runtime.progress.failed,
        })
    }

    pub async fn configure(&self, json: &str) -> AppResult<GmailStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        self.oauth.configure(json).await.map_err(safe_error)?;
        self.set_runtime(Some(GmailPhase::Disconnected), None)?;
        self.repository
            .set_gmail_sync_status("disconnected", None)
            .await?;
        self.status().await
    }

    pub async fn connect(&self) -> AppResult<GmailStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        let previous = self.status().await?;
        if previous.phase == GmailPhase::ReconnectRequired
            || (previous.configured && previous.account_email.is_none())
        {
            // A revoked grant or an interrupted first validation must not make
            // explicit reconnect fail with "already connected" indefinitely.
            self.oauth.disconnect().await.map_err(safe_error)?;
        }
        self.connection_cancellation.send_replace(false);
        let cancellation = self.connection_cancellation.subscribe();
        self.set_runtime(Some(GmailPhase::Connecting), None)?;
        let result: Result<(), SyncError> = async {
            self.oauth.connect().await?;
            let profile_request = authorized(self.oauth.as_ref(), |token| async move {
                self.service.profile(token_text(&token)?).await
            });
            self.validate_connection(cancellation, profile_request)
                .await
        }
        .await;
        match result {
            Ok(()) => self.set_runtime(Some(GmailPhase::Connected), None)?,
            Err(SyncError::OAuth(OAuthError::Cancelled)) => {
                self.repository
                    .set_gmail_sync_status("disconnected", None)
                    .await?;
                self.set_runtime(Some(GmailPhase::Disconnected), None)?;
                return self.status().await;
            }
            Err(error) => {
                self.record_failure(&error).await?;
                return Err(safe_error(error));
            }
        }
        self.status().await
    }

    pub async fn cancel_connection(&self) -> AppResult<GmailStatus> {
        self.connection_cancellation.send_replace(true);
        self.oauth.cancel();
        self.status().await
    }

    async fn validate_connection(
        &self,
        mut cancellation: watch::Receiver<bool>,
        profile_request: impl std::future::Future<Output = Result<GmailProfile, SyncError>>,
    ) -> Result<(), SyncError> {
        let profile = if *cancellation.borrow() {
            Err(SyncError::OAuth(OAuthError::Cancelled))
        } else {
            tokio::select! {
                biased;
                _ = cancellation.changed() => Err(SyncError::OAuth(OAuthError::Cancelled)),
                result = profile_request => result,
            }
        };
        let result = match profile {
            Ok(profile) => self
                .repository
                .bind_gmail_account(&profile.email_address)
                .await
                .map_err(SyncError::from),
            Err(error) => Err(error),
        };
        if result.is_err() {
            // No unverified account's newly issued credentials remain attached
            // after a failed lookup, refused mailbox, or cancelled validation.
            self.oauth.disconnect().await?;
        }
        result
    }

    pub async fn disconnect(&self) -> AppResult<GmailStatus> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        self.oauth.disconnect().await.map_err(safe_error)?;
        self.repository
            .set_gmail_sync_status("disconnected", None)
            .await?;
        self.set_runtime(Some(GmailPhase::Disconnected), None)?;
        self.status().await
    }

    pub async fn sync(&self) -> AppResult<WorkspaceSnapshot> {
        let _operation = self.operation.try_lock().map_err(|_| busy_error())?;
        self.set_runtime(Some(GmailPhase::Syncing), None)?;
        {
            let mut runtime = self.runtime.write().map_err(|_| state_error())?;
            runtime.progress = SyncProgress::default();
        }
        let result = synchronize(
            &self.repository,
            self.service.as_ref(),
            self.oauth.as_ref(),
            &|progress| {
                if let Ok(mut runtime) = self.runtime.write() {
                    runtime.progress = progress;
                }
            },
        )
        .await;
        match result {
            Ok(_) => {
                self.set_runtime(Some(GmailPhase::Connected), None)?;
                tracing::info!("Read-only Gmail synchronization completed");
                self.repository.snapshot().await
            }
            Err(error) => {
                self.record_failure(&error).await?;
                Err(safe_error(error))
            }
        }
    }

    async fn record_failure(&self, error: &SyncError) -> AppResult<()> {
        let (phase, code) = if error.requires_reconnect() {
            (GmailPhase::ReconnectRequired, "reconnect_required")
        } else {
            (GmailPhase::Error, "error")
        };
        self.set_runtime(Some(phase), Some(error.to_string()))?;
        if error.requires_reconnect() {
            self.oauth.disconnect().await.map_err(safe_error)?;
        }
        self.repository
            .set_gmail_sync_status(code, Some("integration_failed"))
            .await?;
        tracing::warn!(error_kind = code, "Gmail operation did not complete");
        Ok(())
    }

    fn set_runtime(&self, phase: Option<GmailPhase>, last_error: Option<String>) -> AppResult<()> {
        let mut runtime = self.runtime.write().map_err(|_| state_error())?;
        runtime.phase = phase;
        runtime.last_error = last_error;
        Ok(())
    }
}

fn safe_error(error: impl std::fmt::Display) -> AppError {
    AppError::Integration(error.to_string())
}
fn state_error() -> AppError {
    AppError::Integration(
        "Gmail status is temporarily unavailable. Restart JobView and try again.".into(),
    )
}
fn busy_error() -> AppError {
    AppError::Integration(
        "Another Gmail operation is in progress. Wait for it to finish or cancel sign-in.".into(),
    )
}

#[cfg(test)]
mod tests;
