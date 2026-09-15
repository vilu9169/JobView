use sqlx::{FromRow, SqliteConnection};
use uuid::Uuid;

use crate::{
    domain::Email,
    error::{AppError, AppResult},
    services::stages::infer_stage,
};

pub(super) struct EventDraft<'a> {
    pub application_id: &'a str,
    pub event_type: &'a str,
    pub event_date: &'a str,
    pub source_email_id: Option<&'a str>,
    pub confidence: Option<f64>,
    pub event_source: &'a str,
    pub notes: Option<&'a str>,
    pub target_stage: Option<&'a str>,
    pub created_at: &'a str,
}

pub(super) async fn append(
    connection: &mut SqliteConnection,
    event: EventDraft<'_>,
) -> AppResult<()> {
    sqlx::query("INSERT INTO application_events (id, application_id, event_type, event_date, source_email_id, confidence, event_source, notes, target_stage, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string())
        .bind(event.application_id)
        .bind(event.event_type)
        .bind(event.event_date)
        .bind(event.source_email_id)
        .bind(event.confidence)
        .bind(event.event_source)
        .bind(event.notes)
        .bind(event.target_stage)
        .bind(event.created_at)
        .execute(connection).await?;
    Ok(())
}

pub(super) async fn note(
    connection: &mut SqliteConnection,
    application_id: &str,
    notes: &str,
    timestamp: &str,
) -> AppResult<()> {
    append(
        connection,
        EventDraft {
            application_id,
            event_type: "note",
            event_date: timestamp,
            source_email_id: None,
            confidence: None,
            event_source: "manual",
            notes: Some(notes),
            target_stage: None,
            created_at: timestamp,
        },
    )
    .await
}

#[derive(FromRow)]
struct ProjectionEvent {
    event_type: String,
    event_source: String,
    target_stage: Option<String>,
}

/// Rebuild the cached stage from preserved history after a link or correction.
/// A user's most recent explicit stage edit always wins over inferred events.
pub(super) async fn project(
    connection: &mut SqliteConnection,
    application_id: &str,
    timestamp: &str,
) -> AppResult<()> {
    let initial: String =
        sqlx::query_scalar("SELECT initial_stage FROM job_applications WHERE id = ?")
            .bind(application_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(AppError::NotFound("Application"))?;
    let manual: Option<String> = sqlx::query_scalar("SELECT target_stage FROM application_events WHERE application_id = ? AND event_type = 'stage_changed' AND event_source = 'manual' AND target_stage IS NOT NULL AND superseded_at IS NULL ORDER BY event_date DESC, rowid DESC LIMIT 1")
        .bind(application_id).fetch_optional(&mut *connection).await?;
    let manually_set = manual.is_some();
    let stage = if let Some(manual) = manual {
        manual
    } else {
        let events = sqlx::query_as::<_, ProjectionEvent>("SELECT event_type, event_source, target_stage FROM application_events WHERE application_id = ? AND superseded_at IS NULL ORDER BY event_date ASC, rowid ASC")
            .bind(application_id).fetch_all(&mut *connection).await?;
        events.into_iter().fold(initial, |stage, event| {
            // The initial manual event documents the base state. Email events can
            // precede creation of the tracker record and still advance that state.
            if event.event_source == "manual_initial" || event.target_stage.is_some() {
                stage
            } else {
                infer_stage(&stage, &event.event_type, false)
            }
        })
    };
    sqlx::query("UPDATE job_applications SET current_stage = ?, stage_manually_set = ?, updated_at = ? WHERE id = ? AND (current_stage <> ? OR stage_manually_set <> ?)")
        .bind(&stage).bind(manually_set).bind(timestamp).bind(application_id).bind(&stage).bind(manually_set)
        .execute(connection).await?;
    Ok(())
}

pub(super) async fn supersede_email(
    connection: &mut SqliteConnection,
    email_id: &str,
    timestamp: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE application_events SET superseded_at = ? WHERE source_email_id = ? AND event_source IN ('local_rules', 'gemini') AND superseded_at IS NULL")
        .bind(timestamp).bind(email_id).execute(connection).await?;
    Ok(())
}

/// Keep one active inferred event per linked email. An unchanged rebuild is a
/// no-op; a changed classification retains its old event as superseded audit data.
pub(super) async fn sync_email(
    connection: &mut SqliteConnection,
    application_id: &str,
    email: &Email,
    timestamp: &str,
) -> AppResult<()> {
    if !email.is_job_related {
        supersede_email(connection, &email.id, timestamp).await?;
        return project(connection, application_id, timestamp).await;
    }
    let event_type = email.event_type.as_deref().unwrap_or("note");
    let notes = format!("{} · Rule: {}", email.subject, email.reasoning_code);
    let same: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM application_events WHERE application_id = ? AND source_email_id = ? AND event_type = ? AND event_date = ? AND confidence = ? AND notes = ? AND event_source = ? AND superseded_at IS NULL")
        .bind(application_id).bind(&email.id).bind(event_type).bind(&email.received_at)
        .bind(email.classification_confidence).bind(&notes).bind(&email.classification_source)
        .fetch_one(&mut *connection).await?;
    if same == 0 {
        supersede_email(connection, &email.id, timestamp).await?;
        append(
            connection,
            EventDraft {
                application_id,
                event_type,
                event_date: &email.received_at,
                source_email_id: Some(&email.id),
                confidence: Some(email.classification_confidence),
                event_source: &email.classification_source,
                notes: Some(&notes),
                target_stage: None,
                created_at: timestamp,
            },
        )
        .await?;
    }
    project(connection, application_id, timestamp).await
}
