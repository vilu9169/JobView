use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct JobApplication {
    pub id: String,
    pub company: String,
    pub role: String,
    pub location: String,
    pub job_url: String,
    pub source: String,
    pub applied_at: Option<String>,
    pub current_stage: String,
    pub next_action: Option<String>,
    pub next_action_due_at: Option<String>,
    pub notes: String,
    pub archived: bool,
    pub stage_manually_set: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationInput {
    pub id: Option<String>,
    pub company: String,
    pub role: String,
    pub location: String,
    pub job_url: String,
    pub source: String,
    pub applied_at: Option<String>,
    pub current_stage: String,
    pub next_action: Option<String>,
    pub next_action_due_at: Option<String>,
    pub notes: String,
    pub archived: bool,
    pub source_email_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub id: String,
    pub gmail_message_id: String,
    pub gmail_thread_id: String,
    pub sender_name: String,
    pub sender_email: String,
    #[sqlx(json)]
    pub recipients: Vec<String>,
    pub subject: String,
    pub snippet: String,
    pub body_text: String,
    pub received_at: String,
    pub content_hash: String,
    pub category: String,
    pub classification_confidence: f64,
    pub classification_source: String,
    pub classified_at: Option<String>,
    pub requires_action: bool,
    pub suggested_action: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_job_related: bool,
    pub reasoning_code: String,
    pub extracted_company: Option<String>,
    pub extracted_role: Option<String>,
    pub suggested_stage: Option<String>,
    pub event_type: Option<String>,
    pub deadline: Option<String>,
    pub manual_override: bool,
    pub ignored: bool,
    pub action_completed: bool,
    pub gmail_account_id: Option<String>,
    pub remote_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationEmail {
    pub application_id: String,
    pub email_id: String,
    pub association_confidence: f64,
    pub association_source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationEvent {
    pub id: String,
    pub application_id: String,
    pub event_type: String,
    pub event_date: String,
    pub source_email_id: Option<String>,
    pub confidence: Option<f64>,
    pub event_source: String,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSettings {
    pub sync_email_limit: i64,
    pub local_confidence_accept_threshold: f64,
    pub local_confidence_gemini_threshold: f64,
    pub ai_mode: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            sync_email_limit: 500,
            local_confidence_accept_threshold: 0.9,
            local_confidence_gemini_threshold: 0.5,
            ai_mode: "off".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub email: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SyncMetadata {
    pub account_id: String,
    pub email_address: String,
    pub gmail_history_id: Option<String>,
    pub last_successful_sync_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub sync_status: String,
    pub error_code: Option<String>,
    pub initial_sync_limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub applications: Vec<JobApplication>,
    pub emails: Vec<Email>,
    pub links: Vec<ApplicationEmail>,
    pub events: Vec<ApplicationEvent>,
    pub settings: AppSettings,
    pub database_path: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailDisposition {
    NotJob,
    Ignored,
    Restore,
}
