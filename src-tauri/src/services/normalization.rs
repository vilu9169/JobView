use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The repository keeps this original input separately from the normalized view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEmail {
    pub gmail_message_id: String,
    pub gmail_thread_id: String,
    pub sender_name: String,
    pub sender_email: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub received_at: String,
    #[serde(default)]
    pub snippet: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedEmail {
    pub sender_name: String,
    pub sender_email: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub received_at: String,
    pub snippet: String,
    pub body_text: String,
    pub content_hash: String,
}

pub fn normalize_email(raw: &RawEmail) -> NormalizedEmail {
    let raw_text = raw
        .body_text
        .as_deref()
        .filter(|body| !body.trim().is_empty());
    // Some ATS messages put HTML in a text/plain MIME part. Recognize actual
    // markup, while keeping normal prose (including angle brackets) as text.
    let text = raw_text.filter(|body| !looks_like_html(body));
    let rendered_html = if text.is_none() {
        raw.body_html.as_deref().or(raw_text).and_then(|html| {
            // The HTML parser tolerates imperfect email markup, decodes entities,
            // ignores script/style contents, and never executes or fetches anything.
            html2text::from_read(html.as_bytes(), 120)
                .ok()
                .filter(|body| !body.trim().is_empty())
        })
    } else {
        None
    };
    let body_text = clean_body(text.or(rendered_html.as_deref()).unwrap_or(&raw.snippet));
    let subject = compact_whitespace(&raw.subject);
    let sender_name = compact_whitespace(&raw.sender_name);
    let sender_email = raw.sender_email.trim().to_lowercase();
    let recipients: Vec<String> = raw
        .recipients
        .iter()
        .map(|recipient| recipient.trim().to_lowercase())
        .filter(|recipient| !recipient.is_empty())
        .collect();
    let snippet = if body_text.is_empty() {
        compact_whitespace(&raw.snippet)
    } else {
        compact_whitespace(&body_text).chars().take(180).collect()
    };

    // Hash precisely the classifier's inputs, with a version and length prefixes
    // to avoid ambiguous concatenations. Quoted history cannot alter this hash.
    let mut hasher = Sha256::new();
    hasher.update(b"mailview-normalization-v2");
    for field in [&sender_name, &sender_email, &subject, &body_text] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }

    NormalizedEmail {
        sender_name,
        sender_email,
        recipients,
        subject,
        received_at: raw.received_at.trim().to_string(),
        snippet,
        body_text,
        content_hash: format!("{:x}", hasher.finalize()),
    }
}

fn looks_like_html(body: &str) -> bool {
    let start = body.trim_start().to_lowercase();
    [
        "<html",
        "<!doctype html",
        "<body",
        "<p>",
        "<p ",
        "<div",
        "<table",
    ]
    .iter()
    .any(|tag| start.starts_with(tag))
        && start.contains('>')
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_body(value: &str) -> String {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for original in normalized.lines() {
        let line = compact_whitespace(original);
        let lower = line.to_lowercase();
        // Only unmistakable quoted-history/signature delimiters cut off the body.
        // A friendly sign-off alone may precede useful recruiter details.
        if line == "--"
            || lower.starts_with("-----original message-----")
            || lower.starts_with("---------- forwarded message")
            || (lower.starts_with("on ") && lower.ends_with(" wrote:"))
            || (lower.starts_with("den ") && lower.contains(" skrev ") && lower.ends_with(':'))
            || lower.starts_with("-----ursprungligt meddelande-----")
            || lower.starts_with("-----originalmeddelande-----")
            || lower == "sent from my iphone"
            || lower == "sent from my android"
            || lower == "skickat från min iphone"
            || lower == "skickat från min android"
        {
            break;
        }
        if line.starts_with('>') {
            continue;
        }
        if lower.starts_with("unsubscribe")
            || lower.starts_with("manage your email preferences")
            || lower.starts_with("view this email in your browser")
            || lower.starts_with("this email and any attachments are confidential")
        {
            continue;
        }
        if line.is_empty() {
            if !lines.is_empty() && lines.last().is_some_and(|last| !last.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn raw(subject: &str, body: &str) -> RawEmail {
        RawEmail {
            gmail_message_id: "fictional-message".into(),
            gmail_thread_id: "fictional-thread".into(),
            sender_name: "Northstar Labs Hiring Team".into(),
            sender_email: "hiring@northstar.example".into(),
            recipients: vec!["candidate@inbox.example".into()],
            subject: subject.into(),
            received_at: "2026-09-08T09:00:00Z".into(),
            snippet: String::new(),
            body_text: Some(body.into()),
            body_html: None,
        }
    }

    #[test]
    fn cleans_whitespace_quote_history_and_signature_without_mutating_raw() {
        let raw = raw(" Interview   invitation ", "Hello,\r\n\r\nPlease   reply.\r\n-- \r\nA signature\r\nOn Monday someone wrote:\r\n> Old rejection");
        let normalized = normalize_email(&raw);
        assert_eq!(normalized.subject, "Interview invitation");
        assert_eq!(normalized.body_text, "Hello,\n\nPlease reply.");
        assert!(raw
            .body_text
            .as_deref()
            .is_some_and(|body| body.contains("Old rejection")));
    }

    #[test]
    fn html_only_mail_decodes_entities_and_excludes_scripts() {
        let mut raw = raw("Interview", "");
        raw.body_html = Some("<html><head><style>h1 { color:red }</style></head><body><p>Hello &amp; welcome.</p><p>Please <b>choose a time</b>.<script>alert('bad')</script></p></body></html>".into());
        let normalized = normalize_email(&raw);
        assert!(normalized.body_text.contains("Hello & welcome."));
        assert!(normalized.body_text.contains("choose a time"));
        assert!(!normalized.body_text.contains("alert"));
        assert!(!normalized.body_text.contains("color:red"));
    }

    #[test]
    fn ats_html_mislabeled_as_plain_text_is_rendered_without_executing_content() {
        let raw = raw("Account information", "<p>Hello Alex,</p><p>Please click below to change your password.</p><p><a href='https://careers.example/reset'>Click here</a></p><script>ignored()</script><p>Fictional Talent Acquisition</p>");
        let normalized = normalize_email(&raw);
        assert!(normalized.body_text.contains("change your password"));
        assert!(!normalized.body_text.contains("<p>"));
        assert!(!normalized.body_text.contains("ignored()"));
        assert!(raw
            .body_text
            .as_ref()
            .expect("original input")
            .contains("<p>"));
        assert_eq!(
            normalize_email(&self::raw("Plain", "Use <Engineer> as a placeholder.")).body_text,
            "Use <Engineer> as a placeholder."
        );
    }

    #[test]
    fn swedish_quote_delimiters_remove_only_previous_messages() {
        for separator in [
            "Den 1 september 2026 skrev Alex:",
            "-----Ursprungligt meddelande-----",
            "-----Originalmeddelande-----",
        ] {
            let normalized = normalize_email(&raw(
                "Intervju",
                &format!(
                    "Välkommen på intervju.\n{separator}\nVi går inte vidare med din ansökan."
                ),
            ));
            assert_eq!(normalized.body_text, "Välkommen på intervju.");
        }
    }

    #[test]
    fn plain_part_takes_precedence_and_empty_parts_fall_back_to_snippet() {
        let mut raw = raw("Hello", "Plain text");
        raw.body_html = Some("<p>Other text</p>".into());
        assert_eq!(normalize_email(&raw).body_text, "Plain text");
        raw.body_text = None;
        raw.body_html = None;
        raw.snippet = "Useful preview".into();
        assert_eq!(normalize_email(&raw).body_text, "Useful preview");
        raw.body_html = Some("<html><body><script>ignored()</script></body></html>".into());
        assert_eq!(normalize_email(&raw).body_text, "Useful preview");
    }

    #[test]
    fn hash_is_stable_for_insignificant_whitespace_and_quotes_but_tracks_content() {
        let first = raw(
            "Application received",
            "Thank you for applying.\nOn Monday Pat wrote:\nOld content",
        );
        let second = raw(
            "Application  received",
            "Thank you   for applying.\nOn Tuesday Pat wrote:\nDifferent old content",
        );
        assert_eq!(
            normalize_email(&first).content_hash,
            normalize_email(&second).content_hash
        );
        let changed = raw("Application received", "We decided not to proceed.");
        assert_ne!(
            normalize_email(&first).content_hash,
            normalize_email(&changed).content_hash
        );
        assert_eq!(normalize_email(&first).content_hash.len(), 64);
    }

    #[test]
    fn snippet_truncation_is_unicode_safe() {
        let normalized = normalize_email(&raw("Hello", &"å".repeat(220)));
        assert_eq!(normalized.snippet.chars().count(), 180);
    }
}
