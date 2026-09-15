//! SQLite is the authority for the workspace. All multi-record changes are atomic.
//! The frontend receives snapshots and cannot perform SQL or infer persisted stages.

mod ai;
mod events;
mod gmail;
mod validation;

#[cfg(test)]
mod tests;

use std::{path::Path, time::Duration};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqliteConnection, SqlitePool,
};
use uuid::Uuid;

pub use ai::{AiSettings, AiUsage, CachedAiClassification};

use crate::{
    domain::{
        AppSettings, ApplicationEmail, ApplicationEvent, ApplicationInput, Email, EmailDisposition,
        JobApplication, WorkspaceSnapshot,
    },
    error::{AppError, AppResult},
    services::{
        classification::{ClassificationService, LocalClassificationService},
        normalization::{normalize_email, RawEmail},
    },
};

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
    database_path: String,
}

impl Repository {
    pub async fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            database_path: path.to_string_lossy().into_owned(),
        })
    }

    /// A consistent read transaction prevents a partially refreshed IPC snapshot.
    pub async fn snapshot(&self) -> AppResult<WorkspaceSnapshot> {
        let mut tx = self.pool.begin().await?;
        let applications = sqlx::query_as::<_, JobApplication>(
            "SELECT * FROM job_applications ORDER BY updated_at DESC, company, role",
        )
        .fetch_all(&mut *tx)
        .await?;
        let emails =
            sqlx::query_as::<_, Email>("SELECT * FROM emails ORDER BY received_at DESC, id")
                .fetch_all(&mut *tx)
                .await?;
        let links = sqlx::query_as::<_, ApplicationEmail>(
            "SELECT * FROM application_emails ORDER BY created_at DESC, email_id",
        )
        .fetch_all(&mut *tx)
        .await?;
        let events = sqlx::query_as::<_, ApplicationEvent>("SELECT * FROM application_events WHERE superseded_at IS NULL ORDER BY event_date DESC, rowid DESC")
            .fetch_all(&mut *tx).await?;
        let settings = sqlx::query_as::<_, AppSettings>("SELECT * FROM app_settings WHERE id = 1")
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(WorkspaceSnapshot {
            applications,
            emails,
            links,
            events,
            settings,
            database_path: self.database_path.clone(),
        })
    }

    pub async fn save_application(&self, input: ApplicationInput) -> AppResult<WorkspaceSnapshot> {
        let input = validation::application(input)?;
        let timestamp = validation::now();
        let mut tx = self.pool.begin().await?;
        let id = if let Some(id) = &input.id {
            let existing = application_by_id(&mut tx, id).await?;
            sqlx::query("UPDATE job_applications SET company = ?, role = ?, location = ?, job_url = ?, source = ?, applied_at = ?, next_action = ?, next_action_due_at = ?, notes = ?, archived = ?, updated_at = ? WHERE id = ?")
                .bind(&input.company).bind(&input.role).bind(&input.location).bind(&input.job_url)
                .bind(&input.source).bind(&input.applied_at).bind(&input.next_action)
                .bind(&input.next_action_due_at).bind(&input.notes).bind(input.archived).bind(&timestamp).bind(id)
                .execute(&mut *tx).await?;
            if existing.current_stage != input.current_stage {
                let notes = format!(
                    "Stage changed from {} to {}.",
                    existing.current_stage, input.current_stage
                );
                events::append(
                    &mut tx,
                    events::EventDraft {
                        application_id: id,
                        event_type: "stage_changed",
                        event_date: &timestamp,
                        source_email_id: None,
                        confidence: None,
                        event_source: "manual",
                        notes: Some(&notes),
                        target_stage: Some(&input.current_stage),
                        created_at: &timestamp,
                    },
                )
                .await?;
            }
            if existing.archived != input.archived {
                events::note(
                    &mut tx,
                    id,
                    if input.archived {
                        "Application archived."
                    } else {
                        "Application restored from archive."
                    },
                    &timestamp,
                )
                .await?;
            }
            if existing.applied_at != input.applied_at {
                let event_date = input
                    .applied_at
                    .as_ref()
                    .map(|date| format!("{date}T00:00:00.000Z"))
                    .unwrap_or(existing.created_at);
                sqlx::query("UPDATE application_events SET event_date = ? WHERE application_id = ? AND event_type = 'application_submitted' AND event_source = 'manual_initial'")
                    .bind(event_date).bind(id).execute(&mut *tx).await?;
            }
            id.clone()
        } else {
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO job_applications (id, company, role, location, job_url, source, applied_at, current_stage, initial_stage, next_action, next_action_due_at, notes, archived, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id).bind(&input.company).bind(&input.role).bind(&input.location).bind(&input.job_url)
                .bind(&input.source).bind(&input.applied_at).bind(&input.current_stage).bind(&input.current_stage)
                .bind(&input.next_action).bind(&input.next_action_due_at).bind(&input.notes)
                .bind(input.archived).bind(&timestamp).bind(&timestamp).execute(&mut *tx).await?;
            let event_date = input
                .applied_at
                .as_ref()
                .map(|date| format!("{date}T00:00:00.000Z"))
                .unwrap_or_else(|| timestamp.clone());
            let notes = format!("Application created at {} stage.", input.current_stage);
            events::append(
                &mut tx,
                events::EventDraft {
                    application_id: &id,
                    event_type: if input.applied_at.is_some() {
                        "application_submitted"
                    } else {
                        "stage_changed"
                    },
                    event_date: &event_date,
                    source_email_id: None,
                    confidence: None,
                    event_source: "manual_initial",
                    notes: Some(&notes),
                    target_stage: Some(&input.current_stage),
                    created_at: &timestamp,
                },
            )
            .await?;
            id
        };
        if let Some(email_id) = &input.source_email_id {
            link_in_transaction(&mut tx, email_id, &id, &timestamp).await?;
        }
        events::project(&mut tx, &id, &timestamp).await?;
        tx.commit().await?;
        self.snapshot().await
    }

    pub async fn load_fixtures(&self) -> AppResult<WorkspaceSnapshot> {
        let emails: Vec<RawEmail> =
            serde_json::from_str(include_str!("../../fixtures/emails.json"))?;
        self.import_emails(&emails).await
    }

    /// Fixture import is explicit and idempotent by Gmail message ID. Raw payloads
    /// are retained locally; repeated imports never reset corrections or actions.
    pub async fn import_emails(&self, emails: &[RawEmail]) -> AppResult<WorkspaceSnapshot> {
        let mut tx = self.pool.begin().await?;
        let timestamp = validation::now();
        let classifier = LocalClassificationService;
        for raw in emails {
            if raw.gmail_message_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "An imported email is missing its message ID.".into(),
                ));
            }
            let cached: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM emails WHERE gmail_message_id = ?)",
            )
            .bind(&raw.gmail_message_id)
            .fetch_one(&mut *tx)
            .await?;
            if cached {
                continue;
            }
            let normalized = normalize_email(raw);
            let received_at = validation::timestamp(&normalized.received_at)?;
            let result = classifier.classify(&normalized);
            sqlx::query("INSERT INTO emails (id, gmail_message_id, gmail_thread_id, sender_name, sender_email, recipients, subject, snippet, body_text, received_at, content_hash, category, classification_confidence, classification_source, classified_at, requires_action, suggested_action, created_at, updated_at, is_job_related, reasoning_code, extracted_company, extracted_role, suggested_stage, event_type, deadline, raw_payload, synced_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'local_rules', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(Uuid::new_v4().to_string()).bind(&raw.gmail_message_id).bind(&raw.gmail_thread_id)
                .bind(&normalized.sender_name).bind(&normalized.sender_email)
                .bind(serde_json::to_string(&normalized.recipients)?).bind(&normalized.subject)
                .bind(&normalized.snippet).bind(&normalized.body_text).bind(received_at).bind(&normalized.content_hash)
                .bind(&result.category).bind(result.confidence).bind(&timestamp)
                .bind(result.requires_action).bind(&result.suggested_action).bind(&timestamp).bind(&timestamp)
                .bind(result.is_job_related).bind(&result.reasoning_code).bind(&result.company).bind(&result.role)
                .bind(&result.suggested_stage).bind(&result.event_type).bind(&result.deadline)
                .bind(serde_json::to_string(raw)?).bind(&timestamp).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        tracing::info!("Fictional email import completed");
        self.snapshot().await
    }

    pub async fn link_email(
        &self,
        email_id: &str,
        application_id: &str,
    ) -> AppResult<WorkspaceSnapshot> {
        let mut tx = self.pool.begin().await?;
        link_in_transaction(&mut tx, email_id, application_id, &validation::now()).await?;
        tx.commit().await?;
        self.snapshot().await
    }

    pub async fn set_email_disposition(
        &self,
        email_id: &str,
        disposition: EmailDisposition,
    ) -> AppResult<WorkspaceSnapshot> {
        let mut tx = self.pool.begin().await?;
        let email = email_by_id(&mut tx, email_id).await?;
        let timestamp = validation::now();
        let correction_type = match disposition {
            EmailDisposition::NotJob => {
                sqlx::query("UPDATE emails SET is_job_related = 0, category = 'not_job', classification_confidence = 1.0, classification_source = 'manual', reasoning_code = 'user_marked_not_job', manual_override = 1, ignored = 0, requires_action = 0, suggested_action = NULL, suggested_stage = NULL, event_type = NULL, deadline = NULL, classified_at = ?, updated_at = ? WHERE id = ?")
                    .bind(&timestamp).bind(&timestamp).bind(email_id).execute(&mut *tx).await?;
                let previous = linked_application(&mut tx, email_id).await?;
                events::supersede_email(&mut tx, email_id, &timestamp).await?;
                sqlx::query("DELETE FROM application_emails WHERE email_id = ?")
                    .bind(email_id)
                    .execute(&mut *tx)
                    .await?;
                if let Some(previous) = previous {
                    events::note(&mut tx, &previous, "Email marked not job related; its inferred event no longer affects this application.", &timestamp).await?;
                    events::project(&mut tx, &previous, &timestamp).await?;
                }
                "not_job"
            }
            EmailDisposition::Ignored => {
                sqlx::query("UPDATE emails SET ignored = 1, updated_at = ? WHERE id = ?")
                    .bind(&timestamp)
                    .bind(email_id)
                    .execute(&mut *tx)
                    .await?;
                "ignored"
            }
            EmailDisposition::Restore => {
                sqlx::query("UPDATE emails SET ignored = 0, manual_override = 0, updated_at = ? WHERE id = ?")
                    .bind(&timestamp).bind(email_id).execute(&mut *tx).await?;
                reclassify_email(&mut tx, email_id, &timestamp).await?;
                "restore"
            }
        };
        if !matches!(disposition, EmailDisposition::Ignored) || !email.ignored {
            correction(&mut tx, email_id, correction_type, None, &timestamp).await?;
        }
        tx.commit().await?;
        self.snapshot().await
    }

    pub async fn rebuild_classifications(&self) -> AppResult<WorkspaceSnapshot> {
        let mut tx = self.pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM emails WHERE manual_override = 0 AND classification_source = 'local_rules' ORDER BY received_at ASC, id",
        )
        .fetch_all(&mut *tx)
        .await?;
        let timestamp = validation::now();
        for id in ids {
            reclassify_email(&mut tx, &id, &timestamp).await?;
        }
        tx.commit().await?;
        tracing::info!("Local classifications rebuilt; AI results and manual overrides preserved");
        self.snapshot().await
    }

    pub async fn resolve_action(
        &self,
        application_id: Option<&str>,
        email_id: Option<&str>,
    ) -> AppResult<WorkspaceSnapshot> {
        if application_id.is_none() && email_id.is_none() {
            return Err(AppError::Validation("Choose an action to complete.".into()));
        }
        let mut tx = self.pool.begin().await?;
        let timestamp = validation::now();
        if let (Some(application_id), Some(email_id)) = (application_id, email_id) {
            if linked_application(&mut tx, email_id).await?.as_deref() != Some(application_id) {
                return Err(AppError::Validation(
                    "This email is not linked to the selected application.".into(),
                ));
            }
        }
        if let Some(application_id) = application_id {
            let application = application_by_id(&mut tx, application_id).await?;
            sqlx::query("UPDATE job_applications SET next_action = NULL, next_action_due_at = NULL, updated_at = ? WHERE id = ?")
                .bind(&timestamp).bind(application_id).execute(&mut *tx).await?;
            if let Some(action) = application.next_action {
                sqlx::query("UPDATE emails SET action_completed = 1, updated_at = ? WHERE suggested_action = ? AND id IN (SELECT email_id FROM application_emails WHERE application_id = ?)")
                    .bind(&timestamp).bind(&action).bind(application_id).execute(&mut *tx).await?;
                events::note(
                    &mut tx,
                    application_id,
                    &format!("Completed action: {action}"),
                    &timestamp,
                )
                .await?;
            }
        }
        if let Some(email_id) = email_id {
            let email = email_by_id(&mut tx, email_id).await?;
            sqlx::query("UPDATE emails SET action_completed = 1, updated_at = ? WHERE id = ?")
                .bind(&timestamp)
                .bind(email_id)
                .execute(&mut *tx)
                .await?;
            if !email.action_completed {
                correction(&mut tx, email_id, "action_completed", None, &timestamp).await?;
                if let Some(linked_id) = linked_application(&mut tx, email_id).await? {
                    let description = email.suggested_action.as_deref().unwrap_or("Email action");
                    events::note(
                        &mut tx,
                        &linked_id,
                        &format!("Completed action: {description}"),
                        &timestamp,
                    )
                    .await?;
                }
            }
        }
        tx.commit().await?;
        self.snapshot().await
    }

    pub async fn save_settings(&self, settings: AppSettings) -> AppResult<WorkspaceSnapshot> {
        validation::settings(&settings)?;
        sqlx::query("UPDATE app_settings SET sync_email_limit = ?, local_confidence_accept_threshold = ?, local_confidence_gemini_threshold = ?, ai_mode = ? WHERE id = 1")
            .bind(settings.sync_email_limit).bind(settings.local_confidence_accept_threshold)
            .bind(settings.local_confidence_gemini_threshold).bind(settings.ai_mode).execute(&self.pool).await?;
        self.snapshot().await
    }
}

async fn email_by_id(connection: &mut SqliteConnection, email_id: &str) -> AppResult<Email> {
    sqlx::query_as::<_, Email>("SELECT * FROM emails WHERE id = ?")
        .bind(email_id)
        .fetch_optional(connection)
        .await?
        .ok_or(AppError::NotFound("Email"))
}

async fn application_by_id(
    connection: &mut SqliteConnection,
    application_id: &str,
) -> AppResult<JobApplication> {
    sqlx::query_as::<_, JobApplication>("SELECT * FROM job_applications WHERE id = ?")
        .bind(application_id)
        .fetch_optional(connection)
        .await?
        .ok_or(AppError::NotFound("Application"))
}

async fn linked_application(
    connection: &mut SqliteConnection,
    email_id: &str,
) -> AppResult<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT application_id FROM application_emails WHERE email_id = ?")
            .bind(email_id)
            .fetch_optional(connection)
            .await?,
    )
}

async fn correction(
    connection: &mut SqliteConnection,
    email_id: &str,
    kind: &str,
    value: Option<&str>,
    timestamp: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO email_corrections (id, email_id, correction_type, value, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string()).bind(email_id).bind(kind).bind(value).bind(timestamp).execute(connection).await?;
    Ok(())
}

async fn link_in_transaction(
    connection: &mut SqliteConnection,
    email_id: &str,
    application_id: &str,
    timestamp: &str,
) -> AppResult<()> {
    let _ = application_by_id(connection, application_id).await?;
    let email = email_by_id(connection, email_id).await?;
    if !email.is_job_related || email.ignored {
        return Err(AppError::Validation(
            "Restore this email as job related before linking it.".into(),
        ));
    }
    let previous = linked_application(connection, email_id).await?;
    if previous.as_deref() != Some(application_id) {
        events::supersede_email(connection, email_id, timestamp).await?;
        sqlx::query("DELETE FROM application_emails WHERE email_id = ?")
            .bind(email_id)
            .execute(&mut *connection)
            .await?;
        sqlx::query("INSERT INTO application_emails (application_id, email_id, association_confidence, association_source, created_at) VALUES (?, ?, 1.0, 'manual', ?)")
            .bind(application_id).bind(email_id).bind(timestamp).execute(&mut *connection).await?;
        correction(
            connection,
            email_id,
            "manual_link",
            Some(application_id),
            timestamp,
        )
        .await?;
        if let Some(previous) = previous {
            events::note(connection, &previous, "Email moved to another application; its inferred event no longer affects this application.", timestamp).await?;
            events::project(connection, &previous, timestamp).await?;
        }
    }
    events::sync_email(connection, application_id, &email, timestamp).await
}

async fn reclassify_email(
    connection: &mut SqliteConnection,
    email_id: &str,
    timestamp: &str,
) -> AppResult<()> {
    let raw_payload: String = sqlx::query_scalar("SELECT raw_payload FROM emails WHERE id = ?")
        .bind(email_id)
        .fetch_one(&mut *connection)
        .await?;
    let raw: RawEmail = serde_json::from_str(&raw_payload)?;
    let normalized = normalize_email(&raw);
    let result = LocalClassificationService.classify(&normalized);
    sqlx::query("UPDATE emails SET category = ?, classification_confidence = ?, classification_source = 'local_rules', classified_at = ?, requires_action = ?, suggested_action = ?, is_job_related = ?, reasoning_code = ?, extracted_company = ?, extracted_role = ?, suggested_stage = ?, event_type = ?, deadline = ?, updated_at = ?, body_text = ?, snippet = ?, content_hash = ? WHERE id = ?")
        .bind(&result.category).bind(result.confidence).bind(timestamp).bind(result.requires_action)
        .bind(&result.suggested_action).bind(result.is_job_related).bind(&result.reasoning_code)
        .bind(&result.company).bind(&result.role).bind(&result.suggested_stage).bind(&result.event_type)
        .bind(&result.deadline).bind(timestamp).bind(&normalized.body_text).bind(&normalized.snippet)
        .bind(&normalized.content_hash).bind(email_id).execute(&mut *connection).await?;
    if let Some(application_id) = linked_application(connection, email_id).await? {
        let email = email_by_id(connection, email_id).await?;
        events::sync_email(connection, &application_id, &email, timestamp).await?;
    }
    Ok(())
}
