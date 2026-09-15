use super::*;
use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

use crate::services::normalization::normalize_email;

struct Reply {
    status: u16,
    headers: String,
    body: String,
}

impl Reply {
    fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            headers: String::new(),
            body: value.to_string(),
        }
    }
}

async fn fake_server(replies: Vec<Reply>) -> (GoogleGmailService, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!(
        "http://{}/gmail/v1/users/me/",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0, "Request ended before headers");
                bytes.extend_from_slice(&buffer[..count]);
                assert!(bytes.len() <= 64 * 1024);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(bytes).unwrap();
            let first_line = request.lines().next().unwrap();
            assert!(first_line.starts_with("GET "));
            // The trace retains only method/path, never the authorization header.
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer fictional-test-token"));
            requests.push(first_line.to_string());
            let response = format!("HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}", reply.status, reply.body.len(), reply.headers, reply.body);
            let _ = socket.write_all(response.as_bytes()).await;
        }
        requests
    });
    (GoogleGmailService::for_test(base), handle)
}

#[tokio::test]
async fn pages_messages_and_history_and_excludes_duplicate_generic_events() {
    let (service, server) = fake_server(vec![
        Reply::json(
            200,
            json!({"emailAddress":"candidate@example.com", "historyId":"100"}),
        ),
        Reply::json(
            200,
            json!({"messages":[{"id":"a1","threadId":"t1"}],"nextPageToken":"next+token/="}),
        ),
        Reply::json(200, json!({"messages":[{"id":"a2","threadId":"t2"}]})),
        Reply::json(
            200,
            json!({"historyId":"200","nextPageToken":"history-next","history":[{
                "messages":[{"id":"a1","threadId":"t1"}],
                "messagesAdded":[{"message":{"id":"a1","threadId":"t1"}}],
                "labelsAdded":[{"message":{"id":"a2","threadId":"t2"},"labelIds":["STARRED"]}],
                "labelsRemoved":[{"message":{"id":"a2","threadId":"t2"},"labelIds":["UNREAD"]}],
                "messagesDeleted":[{"message":{"id":"a3","threadId":"t3"}}]
            }]}),
        ),
        Reply::json(200, json!({"historyId":"200"})),
    ])
    .await;
    assert_eq!(
        service
            .profile("fictional-test-token")
            .await
            .unwrap()
            .email_address,
        "candidate@example.com"
    );
    let first = service
        .list_messages("fictional-test-token", None, 1000)
        .await
        .unwrap();
    assert_eq!(first.messages[0].id, "a1");
    let second = service
        .list_messages("fictional-test-token", first.next_page_token.as_deref(), 1)
        .await
        .unwrap();
    assert_eq!(second.messages[0].id, "a2");
    assert!(second.next_page_token.is_none());
    let history = service
        .list_history("fictional-test-token", "100", None)
        .await
        .unwrap();
    assert_eq!(history.history.len(), 1);
    assert_eq!(history.history[0].messages_added.len(), 1);
    assert_eq!(history.history[0].messages_deleted[0].message.id, "a3");
    assert_eq!(history.history[0].labels_added[0].label_ids, ["STARRED"]);
    assert_eq!(history.history[0].labels_removed[0].label_ids, ["UNREAD"]);
    let last = service
        .list_history(
            "fictional-test-token",
            "100",
            history.next_page_token.as_deref(),
        )
        .await
        .unwrap();
    assert!(last.history.is_empty());
    assert_eq!(last.history_id.as_deref(), Some("200"));
    let requests = server.await.unwrap();
    assert_eq!(requests[0], "GET /gmail/v1/users/me/profile HTTP/1.1");
    assert!(requests[1].contains("maxResults=500&includeSpamTrash=false"));
    assert!(requests[2].contains("pageToken=next%2Btoken%2F%3D"));
    assert!(requests[4].contains("startHistoryId=100&maxResults=500&pageToken=history-next"));
}

#[tokio::test]
async fn full_and_metadata_are_read_only_and_never_fetch_attachments() {
    let message = json!({"id":"abc123","threadId":"thread","payload":{
        "mimeType":"text/plain", "body":{"attachmentId":"large-attachment","size":3000}
    }});
    let (service, server) = fake_server(vec![
        Reply::json(200, message),
        Reply::json(200, json!({"id":"abc123","labelIds":["INBOX"]})),
    ])
    .await;
    let full = service
        .get_message("fictional-test-token", "abc123", false)
        .await
        .unwrap();
    assert!(message_to_raw(&full).unwrap().body_text.is_none());
    let metadata = service
        .get_message("fictional-test-token", "abc123", true)
        .await
        .unwrap();
    assert!(metadata.payload.is_none());
    assert_eq!(metadata.label_ids, ["INBOX"]);
    assert_eq!(server.await.unwrap(), [
        "GET /gmail/v1/users/me/messages/abc123?format=full HTTP/1.1",
        "GET /gmail/v1/users/me/messages/abc123?format=minimal&fields=id%2CthreadId%2ClabelIds%2ChistoryId HTTP/1.1"
    ]);
}

#[tokio::test]
async fn retries_transient_and_rate_limit_errors_then_succeeds() {
    let (service, server) = fake_server(vec![
        Reply::json(503, json!({"error":{"message":"private server details"}})),
        Reply::json(
            403,
            json!({"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}),
        ),
        Reply {
            status: 429,
            headers: "Retry-After: 0\r\n".into(),
            body: "{}".into(),
        },
        Reply::json(
            200,
            json!({"emailAddress":"candidate@example.com","historyId":"1"}),
        ),
    ])
    .await;
    assert!(service.profile("fictional-test-token").await.is_ok());
    assert_eq!(server.await.unwrap().len(), 4);
}

#[tokio::test]
async fn retry_exhaustion_is_bounded_and_errors_do_not_expose_remote_body() {
    let (service, server) = fake_server(
        (0..4)
            .map(|_| {
                Reply::json(
                    503,
                    json!({"error":{"message":"fictional-test-token private contents"}}),
                )
            })
            .collect(),
    )
    .await;
    let error = service.profile("fictional-test-token").await.unwrap_err();
    assert_eq!(error, GmailError::ApiFailure(503));
    assert!(!error.to_string().contains("private"));
    assert!(!format!("{error:?}").contains("fictional-test-token"));
    assert_eq!(server.await.unwrap().len(), 4);
}

#[tokio::test]
async fn auth_expired_history_and_permanent_errors_do_not_retry() {
    for (status, expected) in [
        (401, GmailError::Unauthorized),
        (404, GmailError::NotFound),
        (403, GmailError::ApiFailure(403)),
        (400, GmailError::ApiFailure(400)),
        (302, GmailError::ApiFailure(302)),
    ] {
        let (service, server) = fake_server(vec![Reply {
            status,
            headers: "Location: https://example.invalid/token-leak\r\n".into(),
            body: "private error text".into(),
        }])
        .await;
        assert_eq!(
            service
                .list_history("fictional-test-token", "old", None)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(server.await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn long_retry_after_returns_without_early_retry() {
    let (service, server) = fake_server(vec![Reply {
        status: 429,
        headers: "Retry-After: 3600\r\n".into(),
        body: "{}".into(),
    }])
    .await;
    assert_eq!(
        service.profile("fictional-test-token").await.unwrap_err(),
        GmailError::RateLimited
    );
    assert_eq!(server.await.unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_invalid_json_mismatched_id_and_path_injection() {
    let (service, server) = fake_server(vec![
        Reply {
            status: 200,
            headers: String::new(),
            body: "not-json".into(),
        },
        Reply::json(200, json!({"id":"unexpected"})),
    ])
    .await;
    assert_eq!(
        service.profile("fictional-test-token").await.unwrap_err(),
        GmailError::InvalidResponse
    );
    assert_eq!(
        service
            .get_message("fictional-test-token", "abc123", false)
            .await
            .unwrap_err(),
        GmailError::InvalidResponse
    );
    assert_eq!(
        service
            .get_message("fictional-test-token", "../profile?token=bad", false)
            .await
            .unwrap_err(),
        GmailError::InvalidResponse
    );
    assert_eq!(server.await.unwrap().len(), 2);
}

#[tokio::test]
async fn rejects_oversized_response_before_deserialization() {
    let (service, server) = fake_server(vec![Reply {
        status: 200,
        headers: String::new(),
        body: " ".repeat(MAX_RESPONSE_BYTES + 1),
    }])
    .await;
    assert_eq!(
        service.profile("fictional-test-token").await.unwrap_err(),
        GmailError::InvalidResponse
    );
    assert_eq!(server.await.unwrap().len(), 1);
}

#[test]
fn retry_classifier_and_retry_after_date_are_precise() {
    assert_eq!(
        classify_failure(
            403,
            br#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#
        ),
        GmailError::RateLimited
    );
    assert!(!is_retryable(&classify_failure(
        403,
        br#"{"error":{"errors":[{"reason":"dailyLimitExceeded"}]}}"#
    )));
    assert!(!is_retryable(&classify_failure(403, b"rateLimitExceeded")));
    assert_eq!(
        parse_retry_after("9", SystemTime::UNIX_EPOCH),
        Some(Duration::from_secs(9))
    );
    let date = "Wed, 21 Oct 2015 07:28:00 GMT";
    let at = httpdate::parse_http_date(date).unwrap();
    assert_eq!(
        parse_retry_after(date, at - Duration::from_secs(12)),
        Some(Duration::from_secs(12))
    );
    assert_eq!(
        parse_retry_after(date, at + Duration::from_secs(12)),
        Some(Duration::ZERO)
    );
    assert_eq!(parse_retry_after("garbage", at), None);
}

fn message_with_payload(payload: serde_json::Value) -> GmailMessage {
    serde_json::from_value(json!({
        "id":"18cde123", "threadId":"18cde100", "labelIds":["INBOX"],
        "historyId":"7654321", "internalDate":"1788858000000",
        "snippet":"Useful Gmail preview", "payload":payload
    }))
    .unwrap()
}

fn encoded(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(text.as_bytes())
}

#[test]
fn decodes_realistic_alternative_mime_headers_addresses_and_legacy_charset() {
    let message = message_with_payload(json!({"mimeType":"multipart/alternative","headers":[
        {"name":"From","value":format!("=?UTF-8?B?{}?= <Alice@Example.com>", base64::engine::general_purpose::STANDARD.encode("Ålice Morgan"))},
        {"name":"To","value":"\"Morgan, Alex\" <Alex@Example.com>, Team: recruiter@example.com, second@example.com;"},
        {"name":"Cc","value":"Alex@example.com"},
        {"name":"Subject","value":"=?ISO-8859-1?Q?N=E4sta_steg?=\r\n =?UTF-8?Q?_f=C3=B6r_dig?="},
        {"name":"Date","value":"invalid date"}
    ],"parts":[
        {"mimeType":"text/plain","headers":[{"name":"Content-Type","value":"text/plain; charset=iso-8859-1"}],"body":{"data":URL_SAFE.encode(b"Hej! N\xe4sta steg = interviews.")}},
        {"mimeType":"text/html","body":{"data":encoded("<p>Different HTML alternative</p>")}}
    ]}));
    let raw = message_to_raw(&message).unwrap();
    let normalized = normalize_email(&raw);
    assert_eq!(normalized.sender_name, "Ålice Morgan");
    assert_eq!(normalized.sender_email, "alice@example.com");
    assert_eq!(
        normalized.recipients,
        [
            "alex@example.com",
            "recruiter@example.com",
            "second@example.com"
        ]
    );
    assert_eq!(normalized.subject, "Nästa steg för dig");
    assert_eq!(normalized.body_text, "Hej! Nästa steg = interviews.");
    assert_eq!(normalized.received_at, "2026-09-08T09:00:00Z");
    assert_eq!(
        raw.body_html.as_deref(),
        Some("<p>Different HTML alternative</p>")
    );
}

#[test]
fn html_only_message_uses_local_text_renderer_and_no_active_content() {
    let message = message_with_payload(json!({"mimeType":"multipart/related","parts":[
        {"mimeType":"text/html","body":{"data":encoded("<p>Please choose a time &amp; reply.</p><img src='https://tracker.invalid/open'><script>steal()</script><style>.private{}</style>")}},
        {"mimeType":"image/png","filename":"logo.png","body":{"attachmentId":"image","size":42}}
    ]}));
    let normalized = normalize_email(&message_to_raw(&message).unwrap());
    assert!(normalized
        .body_text
        .contains("Please choose a time & reply."));
    assert!(!normalized.body_text.contains("steal"));
    assert!(!normalized.body_text.contains(".private"));
}

#[test]
fn skips_text_attachments_and_embedded_messages_even_without_filename() {
    let message = message_with_payload(json!({"mimeType":"multipart/mixed","parts":[
        {"mimeType":"text/plain","body":{"data":encoded("Visible body")}},
        {"mimeType":"text/plain","filename":"cv.txt","body":{"data":encoded("Secret CV contents")}},
        {"mimeType":"text/plain","headers":[{"name":"Content-Disposition","value":"attachment"}],"body":{"data":encoded("Attached text")}},
        {"mimeType":"text/plain","headers":[{"name":"Content-Type","value":"text/plain; name=offer.txt"}],"body":{"data":encoded("Offer attachment")}},
        {"mimeType":"text/plain","body":{"attachmentId":"external","data":encoded("External attachment")}},
        {"mimeType":"message/rfc822","parts":[{"mimeType":"text/plain","body":{"data":encoded("Forwarded old rejection")}}]}
    ]}));
    assert_eq!(
        normalize_email(&message_to_raw(&message).unwrap()).body_text,
        "Visible body"
    );
}

#[test]
fn broken_or_empty_parts_fall_back_individually_to_html_then_snippet() {
    let mut message = message_with_payload(json!({"mimeType":"multipart/alternative","parts":[
        {"mimeType":"text/plain","body":{"data":"not valid base64!!!"}},
        {"mimeType":"text/plain","body":{"data":encoded("  ")}},
        {"mimeType":"text/html","body":{"data":encoded("<p>HTML survives</p>")}}
    ]}));
    assert_eq!(
        normalize_email(&message_to_raw(&message).unwrap()).body_text,
        "HTML survives"
    );
    message.payload.as_mut().unwrap().parts.pop();
    assert_eq!(
        normalize_email(&message_to_raw(&message).unwrap()).body_text,
        "Useful Gmail preview"
    );
    message.payload = None;
    assert_eq!(
        normalize_email(&message_to_raw(&message).unwrap()).body_text,
        "Useful Gmail preview"
    );
}

#[test]
fn malformed_charset_bytes_do_not_destroy_preview_or_double_decode_transfer_encoding() {
    let bad = message_with_payload(
        json!({"mimeType":"text/plain","headers":[{"name":"Content-Type","value":"text/plain; charset=utf-8"}],"body":{"data":URL_SAFE_NO_PAD.encode([0xff, 0xff])}}),
    );
    assert_eq!(
        normalize_email(&message_to_raw(&bad).unwrap()).body_text,
        "Useful Gmail preview"
    );
    let already_decoded = message_with_payload(
        json!({"mimeType":"text/plain","headers":[{"name":"Content-Transfer-Encoding","value":"quoted-printable"}],"body":{"data":encoded("Keep this literal =3D value")}}),
    );
    assert_eq!(
        normalize_email(&message_to_raw(&already_decoded).unwrap()).body_text,
        "Keep this literal =3D value"
    );
}

#[test]
fn date_and_missing_header_fallbacks_are_deterministic_and_ids_are_required() {
    let mut message = message_with_payload(
        json!({"headers":[{"name":"Date","value":"Tue, 8 Sep 2026 09:00:00 +0200"}]}),
    );
    message.internal_date = Some("invalid".into());
    let raw = message_to_raw(&message).unwrap();
    assert_eq!(raw.received_at, "2026-09-08T07:00:00Z");
    assert!(raw.sender_email.is_empty());
    message.payload = None;
    message.thread_id.clear();
    let raw = message_to_raw(&message).unwrap();
    assert_eq!(raw.received_at, "1970-01-01T00:00:00Z");
    assert_eq!(raw.gmail_thread_id, message.id);
    message.id.clear();
    assert_eq!(
        message_to_raw(&message).unwrap_err(),
        GmailError::InvalidResponse
    );
}

#[test]
fn structured_mime_cache_round_trip_preserves_original_encoded_body() {
    let message = message_with_payload(
        json!({"mimeType":"text/html", "partId":"1", "headers":[{"name":"Subject","value":"=?UTF-8?Q?Original_subject?="}],"body":{"data":encoded("<p>Original &amp; content</p>"),"size":29}}),
    );
    let before = serde_json::to_value(&message).unwrap();
    let raw = message_to_raw(&message).unwrap();
    assert_eq!(raw.subject, "Original subject");
    assert_eq!(serde_json::to_value(&message).unwrap(), before);
    let restored: GmailMessage = serde_json::from_value(before).unwrap();
    assert_eq!(
        restored.payload.unwrap().body.data,
        message.payload.unwrap().body.data
    );
}
