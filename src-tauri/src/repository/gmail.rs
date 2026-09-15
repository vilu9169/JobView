//! Account-scoped cache writes and durable Gmail synchronization checkpoints.
//! Credentials are deliberately absent from this repository.

use std::collections::BTreeSet;

use crate::{
    domain::{Email, SyncMetadata},
    error::{AppError, AppResult},
    services::{
        classification::{ClassificationService, LocalClassificationService},
        gmail::{message_to_raw, GmailMessage},
        normalization::normalize_email,
    },
};
use uuid::Uuid;

use super::{events, linked_application, validation, Repository};

impl Repository {
    pub async fn gmail_sync_state(&self) -> AppResult<Option<SyncMetadata>> {
        Ok(
            sqlx::query_as::<_, SyncMetadata>("SELECT * FROM sync_metadata LIMIT 1")
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn bind_gmail_account(&self, email_address: &str) -> AppResult<()> {
        let address = normalize_account(email_address)?;
        let mut tx = self.pool.begin().await?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT email_address FROM sync_metadata LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(existing) = existing {
            if !existing.eq_ignore_ascii_case(&address) {
                return Err(AppError::Integration("This workspace already belongs to another Gmail account. Reconnect the same account to keep cached emails and applications separate.".into()));
            }
            sqlx::query("UPDATE sync_metadata SET sync_status = 'connected', error_code = NULL")
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("INSERT INTO sync_metadata (account_id, email_address, sync_status) VALUES (?, ?, 'connected')")
                .bind(&address).bind(&address).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_gmail_sync_status(
        &self,
        status: &str,
        error_code: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query("UPDATE sync_metadata SET sync_status = ?, error_code = ?, last_attempt_at = CASE WHEN ? = 'syncing' THEN ? ELSE last_attempt_at END")
            .bind(status).bind(error_code).bind(status).bind(validation::now()).execute(&self.pool).await?;
        Ok(())
    }

    /// Called only after all pages and message writes succeeded. A failed or
    /// interrupted pass leaves its previous checkpoint intact for a safe retry.
    pub async fn finish_gmail_sync(&self, history_id: &str, initial_limit: i64) -> AppResult<()> {
        if history_id.is_empty() || !history_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AppError::Integration(
                "Gmail returned an invalid synchronization checkpoint. Try syncing again.".into(),
            ));
        }
        sqlx::query("UPDATE sync_metadata SET gmail_history_id = ?, initial_sync_limit = MAX(initial_sync_limit, ?), last_successful_sync_at = ?, sync_status = 'connected', error_code = NULL")
            .bind(history_id).bind(initial_limit).bind(validation::now()).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn gmail_message_is_cached(
        &self,
        account_id: &str,
        message_id: &str,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM emails WHERE gmail_account_id = ? AND gmail_message_id = ?)")
            .bind(account_id).bind(message_id).fetch_one(&self.pool).await?)
    }

    pub async fn cached_gmail_ids(&self, account_id: &str) -> AppResult<Vec<String>> {
        Ok(sqlx::query_scalar("SELECT gmail_message_id FROM emails WHERE gmail_account_id = ? AND remote_deleted = 0 ORDER BY received_at DESC")
            .bind(account_id).fetch_all(&self.pool).await?)
    }

    pub async fn replace_gmail_metadata(
        &self,
        account_id: &str,
        message: &GmailMessage,
    ) -> AppResult<()> {
        sqlx::query("UPDATE emails SET gmail_labels = ?, gmail_history_id = ?, synced_at = ?, remote_deleted = 0 WHERE gmail_account_id = ? AND gmail_message_id = ?")
            .bind(serde_json::to_string(&message.label_ids)?).bind(&message.history_id).bind(validation::now())
            .bind(account_id).bind(&message.id).execute(&self.pool).await?;
        Ok(())
    }

    /// Full MIME payloads are only received for new messages. This upsert is
    /// deliberately robust to retries and future explicit content re-fetches.
    /// Returns true only when a new cached email was inserted.
    pub async fn cache_gmail_message(
        &self,
        account_id: &str,
        message: &GmailMessage,
    ) -> AppResult<bool> {
        let raw = message_to_raw(message).map_err(|_| AppError::Integration("A Gmail message could not be normalized. Its sync checkpoint was retained for retry.".into()))?;
        let normalized = normalize_email(&raw);
        let received_at = validation::timestamp(&normalized.received_at)?;
        let timestamp = validation::now();
        let labels = serde_json::to_string(&message.label_ids)?;
        let raw_payload = serde_json::to_string(&raw)?;
        let gmail_payload = serde_json::to_string(message)?;
        let mut tx = self.pool.begin().await?;
        let bound: Option<String> =
            sqlx::query_scalar("SELECT account_id FROM sync_metadata LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?;
        if bound.as_deref() != Some(account_id) {
            return Err(AppError::Integration(
                "Connect the Gmail account before caching its emails.".into(),
            ));
        }
        let existing =
            sqlx::query_as::<_, Email>("SELECT * FROM emails WHERE gmail_message_id = ?")
                .bind(&message.id)
                .fetch_optional(&mut *tx)
                .await?;
        if existing
            .as_ref()
            .is_some_and(|email| email.gmail_account_id.as_deref() != Some(account_id))
        {
            return Err(AppError::Integration("A message identifier belongs to another data source. No cached data was overwritten.".into()));
        }
        let is_new = existing.is_none();
        let id = existing
            .as_ref()
            .map(|email| email.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if is_new {
            let result = LocalClassificationService.classify(&normalized);
            sqlx::query("INSERT INTO emails (id, gmail_message_id, gmail_thread_id, sender_name, sender_email, recipients, subject, snippet, body_text, received_at, content_hash, category, classification_confidence, classification_source, classified_at, requires_action, suggested_action, created_at, updated_at, is_job_related, reasoning_code, extracted_company, extracted_role, suggested_stage, event_type, deadline, raw_payload, synced_at, gmail_labels, gmail_history_id, gmail_account_id, gmail_raw_payload) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'local_rules', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id).bind(&message.id).bind(&message.thread_id).bind(&normalized.sender_name).bind(&normalized.sender_email)
                .bind(serde_json::to_string(&normalized.recipients)?).bind(&normalized.subject).bind(&normalized.snippet)
                .bind(&normalized.body_text).bind(received_at).bind(&normalized.content_hash).bind(&result.category)
                .bind(result.confidence).bind(&timestamp).bind(result.requires_action).bind(&result.suggested_action)
                .bind(&timestamp).bind(&timestamp).bind(result.is_job_related).bind(&result.reasoning_code)
                .bind(&result.company).bind(&result.role).bind(&result.suggested_stage).bind(&result.event_type)
                .bind(&result.deadline).bind(raw_payload).bind(&timestamp).bind(labels).bind(&message.history_id)
                .bind(account_id).bind(gmail_payload).execute(&mut *tx).await?;
        } else if let Some(existing) = &existing {
            let content_changed = existing.content_hash != normalized.content_hash;
            sqlx::query("UPDATE emails SET gmail_thread_id = ?, sender_name = ?, sender_email = ?, recipients = ?, subject = ?, snippet = ?, body_text = ?, received_at = ?, content_hash = ?, raw_payload = ?, gmail_raw_payload = ?, gmail_labels = ?, gmail_history_id = ?, synced_at = ?, remote_deleted = 0, updated_at = CASE WHEN ? THEN ? ELSE updated_at END WHERE id = ?")
                .bind(&message.thread_id).bind(&normalized.sender_name).bind(&normalized.sender_email)
                .bind(serde_json::to_string(&normalized.recipients)?).bind(&normalized.subject).bind(&normalized.snippet)
                .bind(&normalized.body_text).bind(received_at).bind(&normalized.content_hash).bind(raw_payload)
                .bind(gmail_payload).bind(labels).bind(&message.history_id).bind(&timestamp)
                .bind(content_changed).bind(&timestamp).bind(&id).execute(&mut *tx).await?;
            if content_changed && !existing.manual_override {
                super::reclassify_email(&mut tx, &id, &timestamp).await?;
            }
            // Manual classification, links, stage edits and action completion
            // are independent of immutable Gmail content and never reset here.
            if content_changed && existing.manual_override {
                if let Some(application_id) = linked_application(&mut tx, &id).await? {
                    events::project(&mut tx, &application_id, &timestamp).await?;
                }
            }
        }
        tx.commit().await?;
        Ok(is_new)
    }

    pub async fn update_gmail_labels(
        &self,
        account_id: &str,
        message_id: &str,
        added: &[String],
        removed: &[String],
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT gmail_labels FROM emails WHERE gmail_account_id = ? AND gmail_message_id = ?",
        )
        .bind(account_id)
        .bind(message_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(stored) = stored {
            let mut labels: BTreeSet<String> = serde_json::from_str(&stored)?;
            labels.extend(added.iter().cloned());
            for label in removed {
                labels.remove(label);
            }
            sqlx::query("UPDATE emails SET gmail_labels = ?, synced_at = ? WHERE gmail_account_id = ? AND gmail_message_id = ?")
                .bind(serde_json::to_string(&labels)?).bind(validation::now()).bind(account_id).bind(message_id)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_gmail_deleted(
        &self,
        account_id: &str,
        message_id: &str,
        deleted: bool,
    ) -> AppResult<()> {
        // Retain cached source evidence and user history when remote mail disappears.
        sqlx::query("UPDATE emails SET remote_deleted = ?, synced_at = ? WHERE gmail_account_id = ? AND gmail_message_id = ?")
            .bind(deleted).bind(validation::now()).bind(account_id).bind(message_id).execute(&self.pool).await?;
        Ok(())
    }
}

fn normalize_account(address: &str) -> AppResult<String> {
    let address = address.trim().to_lowercase();
    if address.len() > 320 || !address.contains('@') || address.chars().any(char::is_whitespace) {
        return Err(AppError::Integration(
            "Gmail did not return a valid account address.".into(),
        ));
    }
    Ok(address)
}
