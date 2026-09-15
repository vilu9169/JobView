//! Read-only Gmail transport. Production URLs are fixed and credentials never
//! appear in errors. MIME data is parsed locally without fetching attachments.

mod mime;
pub use mime::message_to_raw;

use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use reqwest::{redirect::Policy, Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

const GMAIL_BASE_URL: &str = "https://gmail.googleapis.com/gmail/v1/users/me/";
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RETRIES: u32 = 3;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GmailError {
    #[error("Gmail authorization expired. Reconnect your account if refreshing does not help.")]
    Unauthorized,
    #[error("The Gmail message or synchronization history is no longer available.")]
    NotFound,
    #[error("Gmail's rate limit was reached. Try synchronizing again later.")]
    RateLimited,
    #[error("Could not reach Gmail. Check your internet connection and try again.")]
    Network,
    #[error("Gmail returned an invalid or oversized response.")]
    InvalidResponse,
    #[error("Gmail could not complete the request (HTTP {0}).")]
    ApiFailure(u16),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailProfile {
    pub email_address: String,
    pub history_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRef {
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailMessage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    pub history_id: Option<String>,
    pub internal_date: Option<String>,
    #[serde(default)]
    pub snippet: String,
    pub payload: Option<MessagePart>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessagePart {
    pub part_id: String,
    pub mime_type: String,
    pub filename: String,
    pub headers: Vec<MessageHeader>,
    pub body: MessagePartBody,
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessagePartBody {
    pub attachment_id: Option<String>,
    pub size: u64,
    pub data: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub history_id: Option<String>,
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub history: Vec<HistoryRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryRecord {
    // Gmail's generic `messages` array duplicates the specific change arrays.
    pub messages_added: Vec<HistoryMessage>,
    pub messages_deleted: Vec<HistoryMessage>,
    pub labels_added: Vec<HistoryLabels>,
    pub labels_removed: Vec<HistoryLabels>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub message: MessageRef,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryLabels {
    pub message: MessageRef,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

#[async_trait]
pub trait GmailService: Send + Sync {
    async fn profile(&self, access_token: &str) -> Result<GmailProfile, GmailError>;
    async fn list_messages(
        &self,
        access_token: &str,
        page_token: Option<&str>,
        max_results: u32,
    ) -> Result<MessagePage, GmailError>;
    async fn get_message(
        &self,
        access_token: &str,
        message_id: &str,
        metadata_only: bool,
    ) -> Result<GmailMessage, GmailError>;
    async fn list_history(
        &self,
        access_token: &str,
        start_history_id: &str,
        page_token: Option<&str>,
    ) -> Result<HistoryPage, GmailError>;
}

pub struct GoogleGmailService {
    client: Client,
    base_url: Url,
    retry_base: Duration,
}

impl GoogleGmailService {
    pub fn new() -> Result<Self, GmailError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .user_agent(concat!("JobView/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| GmailError::Network)?;
        Ok(Self {
            client,
            base_url: Url::parse(GMAIL_BASE_URL).map_err(|_| GmailError::InvalidResponse)?,
            retry_base: Duration::from_secs(1),
        })
    }

    #[cfg(test)]
    fn for_test(base_url: Url) -> Self {
        // Injecting an endpoint is only compiled into tests.
        Self {
            client: Client::builder()
                .redirect(Policy::none())
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            base_url,
            retry_base: Duration::from_millis(1),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        access_token: &str,
        params: &[(&str, String)],
    ) -> Result<T, GmailError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| GmailError::InvalidResponse)?;
        for attempt in 0..=MAX_RETRIES {
            let response = self
                .client
                .get(url.clone())
                .bearer_auth(access_token)
                .query(params)
                .send()
                .await
                .map_err(|_| GmailError::Network)?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| parse_retry_after(value, SystemTime::now()));
            // Avoid reading error pages for statuses that cannot benefit from it.
            if status == StatusCode::UNAUTHORIZED {
                return Err(GmailError::Unauthorized);
            }
            if status == StatusCode::NOT_FOUND {
                return Err(GmailError::NotFound);
            }
            let body = bounded_body(response).await?;
            if status.is_success() {
                return serde_json::from_slice(&body).map_err(|_| GmailError::InvalidResponse);
            }
            let error = classify_failure(status.as_u16(), &body);
            if attempt < MAX_RETRIES && is_retryable(&error) {
                let exponential = self.retry_base.saturating_mul(1 << attempt);
                // A longer server backoff ends this bounded attempt; a future
                // manual sync can retry without hammering the server too early.
                if retry_after.is_some_and(|delay| delay > MAX_RETRY_DELAY) {
                    return Err(error);
                }
                tokio::time::sleep(exponential.max(retry_after.unwrap_or_default())).await;
                continue;
            }
            return Err(error);
        }
        Err(GmailError::Network)
    }
}

#[async_trait]
impl GmailService for GoogleGmailService {
    async fn profile(&self, access_token: &str) -> Result<GmailProfile, GmailError> {
        let profile: GmailProfile = self.get("profile", access_token, &[]).await?;
        if profile.email_address.trim().is_empty() || profile.history_id.trim().is_empty() {
            return Err(GmailError::InvalidResponse);
        }
        Ok(profile)
    }

    async fn list_messages(
        &self,
        access_token: &str,
        page_token: Option<&str>,
        max_results: u32,
    ) -> Result<MessagePage, GmailError> {
        let mut params = vec![
            ("maxResults", max_results.clamp(1, 500).to_string()),
            ("includeSpamTrash", "false".to_string()),
        ];
        if let Some(token) = page_token {
            params.push(("pageToken", token.to_string()));
        }
        self.get("messages", access_token, &params).await
    }

    async fn get_message(
        &self,
        access_token: &str,
        message_id: &str,
        metadata_only: bool,
    ) -> Result<GmailMessage, GmailError> {
        // IDs are opaque, but contain no URL syntax. Reject anything that could
        // turn this read into a different API path or query.
        if message_id.is_empty()
            || message_id.len() > 256
            || !message_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(GmailError::InvalidResponse);
        }
        let format = if metadata_only { "minimal" } else { "full" };
        let mut params = vec![("format", format.to_string())];
        if metadata_only {
            params.push(("fields", "id,threadId,labelIds,historyId".to_string()));
        }
        let message: GmailMessage = self
            .get(&format!("messages/{message_id}"), access_token, &params)
            .await?;
        if message.id != message_id {
            return Err(GmailError::InvalidResponse);
        }
        Ok(message)
    }

    async fn list_history(
        &self,
        access_token: &str,
        start_history_id: &str,
        page_token: Option<&str>,
    ) -> Result<HistoryPage, GmailError> {
        let mut params = vec![
            ("startHistoryId", start_history_id.to_string()),
            ("maxResults", "500".to_string()),
        ];
        if let Some(token) = page_token {
            params.push(("pageToken", token.to_string()));
        }
        self.get("history", access_token, &params).await
    }
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, GmailError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(GmailError::InvalidResponse);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| GmailError::Network)? {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err(GmailError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_failure(status: u16, body: &[u8]) -> GmailError {
    match status {
        401 => GmailError::Unauthorized,
        404 => GmailError::NotFound,
        429 => GmailError::RateLimited,
        403 => {
            let rate_limited = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/error/errors")
                        .and_then(|v| v.as_array())
                        .cloned()
                })
                .is_some_and(|errors| {
                    errors.iter().any(|error| {
                        matches!(
                            error.get("reason").and_then(|value| value.as_str()),
                            Some("rateLimitExceeded" | "userRateLimitExceeded")
                        )
                    })
                });
            if rate_limited {
                GmailError::RateLimited
            } else {
                GmailError::ApiFailure(status)
            }
        }
        _ => GmailError::ApiFailure(status),
    }
}

fn is_retryable(error: &GmailError) -> bool {
    matches!(
        error,
        GmailError::RateLimited | GmailError::ApiFailure(500..=599)
    )
}

fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|when| when.duration_since(now).unwrap_or_default())
        })
}

#[cfg(test)]
mod tests;
