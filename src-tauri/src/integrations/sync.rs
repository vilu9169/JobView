//! Checkpoint-safe Gmail synchronization, independent of Tauri and real OAuth.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    future::Future,
};

use async_trait::async_trait;
use serde::Serialize;

use crate::{
    error::AppError,
    repository::Repository,
    services::{
        credentials::SecretValue,
        gmail::{GmailError, GmailService, HistoryPage},
        oauth::{OAuthError, OAuthService},
    },
};

const MAX_PAGES: usize = 1000;
const MAX_CHANGES: usize = 100_000;

fn excluded_label(label: &str) -> bool {
    matches!(label, "SPAM" | "TRASH")
}

#[derive(Debug, thiserror::Error)]
pub(super) enum SyncError {
    #[error("{0}")]
    Gmail(#[from] GmailError),
    #[error("{0}")]
    OAuth(#[from] OAuthError),
    #[error("{0}")]
    Repository(#[from] AppError),
    #[error("Some emails could not be cached. Successful emails were saved; the previous checkpoint was kept so Sync can retry safely.")]
    Partial,
    #[error("Gmail returned too many changes or repeated page cursors. No checkpoint was advanced. Try again later.")]
    InvalidPagination,
}

impl SyncError {
    pub(super) fn requires_reconnect(&self) -> bool {
        matches!(self, Self::Gmail(GmailError::Unauthorized))
            || matches!(self, Self::OAuth(error) if error.requires_reconnect())
    }
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct SyncProgress {
    pub processed: u32,
    pub total: u32,
    pub imported: u32,
    pub skipped: u32,
    pub failed: u32,
}

#[async_trait]
pub(super) trait TokenProvider: Send + Sync {
    async fn token(&self, force_refresh: bool) -> Result<SecretValue, OAuthError>;
}

#[async_trait]
impl TokenProvider for OAuthService {
    async fn token(&self, force_refresh: bool) -> Result<SecretValue, OAuthError> {
        self.access_token(force_refresh).await
    }
}

pub(super) async fn authorized<T, F, Fut>(
    auth: &dyn TokenProvider,
    mut request: F,
) -> Result<T, SyncError>
where
    F: FnMut(SecretValue) -> Fut,
    Fut: Future<Output = Result<T, GmailError>>,
{
    let result = request(auth.token(false).await?).await;
    match result {
        Err(GmailError::Unauthorized) => Ok(request(auth.token(true).await?).await?),
        other => Ok(other?),
    }
}

pub(super) fn token_text(token: &SecretValue) -> Result<&str, GmailError> {
    std::str::from_utf8(token.expose()).map_err(|_| GmailError::Unauthorized)
}

#[derive(Default)]
struct Change {
    added: bool,
    deleted: bool,
    added_labels: BTreeSet<String>,
    removed_labels: BTreeSet<String>,
}

struct HistoryPlan {
    changes: BTreeMap<String, Change>,
    checkpoint: String,
}

/// Collect every page before applying it so a page failure is not confused with
/// a complete history pass. The previous cursor remains durable until the end.
async fn history_plan(
    service: &dyn GmailService,
    auth: &dyn TokenProvider,
    start: &str,
) -> Result<Option<HistoryPlan>, SyncError> {
    let mut next = None;
    let mut seen_pages = HashSet::new();
    let mut changes: BTreeMap<String, Change> = BTreeMap::new();
    for _ in 0..MAX_PAGES {
        let page_token = next.as_deref();
        let response = authorized(auth, |token| async move {
            service
                .list_history(token_text(&token)?, start, page_token)
                .await
        })
        .await;
        let page: HistoryPage = match response {
            Err(SyncError::Gmail(GmailError::NotFound)) => return Ok(None),
            other => other?,
        };
        for record in page.history {
            for added in record.messages_added {
                let change = changes.entry(added.message.id).or_default();
                change.added = true;
                change.deleted = false;
            }
            for labels in record.labels_added {
                let change = changes.entry(labels.message.id).or_default();
                for label in labels.label_ids {
                    change.removed_labels.remove(&label);
                    change.added_labels.insert(label);
                }
            }
            for labels in record.labels_removed {
                let change = changes.entry(labels.message.id).or_default();
                for label in labels.label_ids {
                    change.added_labels.remove(&label);
                    change.removed_labels.insert(label);
                }
            }
            for deleted in record.messages_deleted {
                changes.entry(deleted.message.id).or_default().deleted = true;
            }
        }
        if changes.len() > MAX_CHANGES || changes.keys().any(|id| id.is_empty()) {
            return Err(SyncError::InvalidPagination);
        }
        next = page.next_page_token;
        if let Some(cursor) = &next {
            if !seen_pages.insert(cursor.clone()) {
                return Err(SyncError::InvalidPagination);
            }
        } else {
            let checkpoint = page.history_id.ok_or(GmailError::InvalidResponse)?;
            return Ok(Some(HistoryPlan {
                changes,
                checkpoint,
            }));
        }
    }
    Err(SyncError::InvalidPagination)
}

async fn recent_ids(
    service: &dyn GmailService,
    auth: &dyn TokenProvider,
    limit: u32,
) -> Result<Vec<String>, SyncError> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut seen_pages = HashSet::new();
    let mut next = None;
    for _ in 0..MAX_PAGES {
        let page_token = next.as_deref();
        let remaining = limit.saturating_sub(ids.len() as u32).min(500);
        if remaining == 0 {
            return Ok(ids);
        }
        let page = authorized(auth, |token| async move {
            service
                .list_messages(token_text(&token)?, page_token, remaining)
                .await
        })
        .await?;
        for item in page.messages {
            if item.id.is_empty() {
                return Err(GmailError::InvalidResponse.into());
            }
            if seen.insert(item.id.clone()) {
                ids.push(item.id);
            }
            if ids.len() == limit as usize {
                return Ok(ids);
            }
        }
        next = page.next_page_token;
        if let Some(cursor) = &next {
            if !seen_pages.insert(cursor.clone()) {
                return Err(SyncError::InvalidPagination);
            }
        } else {
            return Ok(ids);
        }
    }
    Err(SyncError::InvalidPagination)
}

struct SyncContext<'a> {
    repository: &'a Repository,
    service: &'a dyn GmailService,
    auth: &'a dyn TokenProvider,
    account: &'a str,
    report: &'a (dyn Fn(SyncProgress) + Send + Sync),
}

impl SyncContext<'_> {
    async fn fetch(
        &self,
        id: &str,
        metadata_only: bool,
        progress: &mut SyncProgress,
    ) -> Result<(), SyncError> {
        let result = authorized(self.auth, |token| async move {
            self.service
                .get_message(token_text(&token)?, id, metadata_only)
                .await
        })
        .await;
        let saved = match result {
            Ok(message) if message.id == id => {
                if metadata_only {
                    self.repository
                        .replace_gmail_metadata(self.account, &message)
                        .await?;
                    progress.skipped += 1;
                    Ok(())
                } else if message.label_ids.iter().any(|label| excluded_label(label)) {
                    // History includes Spam/Trash and a listed message can move
                    // there before it is fetched. Keep new imports consistent
                    // with the recent scan's includeSpamTrash=false policy.
                    progress.skipped += 1;
                    Ok(())
                } else {
                    match self
                        .repository
                        .cache_gmail_message(self.account, &message)
                        .await
                    {
                        Ok(true) => {
                            progress.imported += 1;
                            Ok(())
                        }
                        Ok(false) => {
                            progress.skipped += 1;
                            Ok(())
                        }
                        Err(error) => Err(SyncError::Repository(error)),
                    }
                }
            }
            Err(SyncError::Gmail(GmailError::NotFound)) => {
                self.repository
                    .mark_gmail_deleted(self.account, id, true)
                    .await?;
                progress.skipped += 1;
                Ok(())
            }
            Ok(_) => Err(GmailError::InvalidResponse.into()),
            Err(error) => Err(error),
        };
        progress.processed += 1;
        if saved.is_err() {
            progress.failed += 1;
        }
        (self.report)(progress.clone());
        saved
    }
}

/// A sync consists of all available history plus, when needed, a bounded recent
/// scan. Only the final success commits the checkpoint. No local job records or
/// decisions are modified by label changes or remote deletion markers.
pub(super) async fn synchronize(
    repository: &Repository,
    service: &dyn GmailService,
    auth: &dyn TokenProvider,
    report: &(dyn Fn(SyncProgress) + Send + Sync),
) -> Result<SyncProgress, SyncError> {
    let profile = authorized(auth, |token| async move {
        service.profile(token_text(&token)?).await
    })
    .await?;
    repository
        .bind_gmail_account(&profile.email_address)
        .await?;
    let state = repository
        .gmail_sync_state()
        .await?
        .ok_or(GmailError::InvalidResponse)?;
    repository.set_gmail_sync_status("syncing", None).await?;
    let limit = repository.snapshot().await?.settings.sync_email_limit as u32;
    let mut needs_scan =
        state.gmail_history_id.is_none() || i64::from(limit) > state.initial_sync_limit;
    let plan = if let Some(start) = &state.gmail_history_id {
        let plan = history_plan(service, auth, start).await?;
        needs_scan |= plan.is_none();
        plan
    } else {
        None
    };
    let checkpoint = plan
        .as_ref()
        .map(|plan| plan.checkpoint.clone())
        .unwrap_or(profile.history_id);
    let mut progress = SyncProgress::default();
    let context = SyncContext {
        repository,
        service,
        auth,
        account: &state.account_id,
        report,
    };
    if let Some(plan) = plan {
        progress.total = plan.changes.len() as u32;
        report(progress.clone());
        for (id, change) in plan.changes {
            if change.deleted {
                repository
                    .mark_gmail_deleted(&state.account_id, &id, true)
                    .await?;
                progress.skipped += 1;
            } else if (change.added
                || change
                    .removed_labels
                    .iter()
                    .any(|label| excluded_label(label)))
                && !repository
                    .gmail_message_is_cached(&state.account_id, &id)
                    .await?
            {
                // A previously excluded message can re-enter the mailbox via a
                // label removal alone. Fetch its current labels before importing
                // because it may have moved back to Spam/Trash in the meantime.
                // The fetched full message contains current labels; do not apply
                // older history labels on top of a newer authoritative snapshot.
                match context.fetch(&id, false, &mut progress).await {
                    Ok(()) => {}
                    Err(SyncError::Gmail(GmailError::InvalidResponse)) => {}
                    Err(error) => return Err(error),
                }
                continue;
            } else {
                repository
                    .update_gmail_labels(
                        &state.account_id,
                        &id,
                        &change.added_labels.into_iter().collect::<Vec<_>>(),
                        &change.removed_labels.into_iter().collect::<Vec<_>>(),
                    )
                    .await?;
                progress.skipped += 1;
            }
            progress.processed += 1;
            report(progress.clone());
        }
    }
    if needs_scan {
        let mut ids = recent_ids(service, auth, limit).await?;
        let mut included: HashSet<String> = ids.iter().cloned().collect();
        // Refresh only metadata for existing cached messages, including those
        // outside the recent window; recover deletions after history expiry.
        for id in repository.cached_gmail_ids(&state.account_id).await? {
            if included.insert(id.clone()) {
                ids.push(id);
            }
        }
        progress.total += ids.len() as u32;
        report(progress.clone());
        for id in ids {
            let cached = repository
                .gmail_message_is_cached(&state.account_id, &id)
                .await?;
            match context.fetch(&id, cached, &mut progress).await {
                Ok(()) => {}
                Err(SyncError::Gmail(GmailError::InvalidResponse)) => {}
                Err(error) => return Err(error),
            }
        }
    }
    if progress.failed > 0 {
        return Err(SyncError::Partial);
    }
    repository
        .finish_gmail_sync(&checkpoint, i64::from(limit))
        .await?;
    Ok(progress)
}

#[cfg(test)]
mod tests;
