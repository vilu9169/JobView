use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{DateTime, SecondsFormat, Utc};
use encoding_rs::Encoding;
use mailparse::{MailAddr, SingleInfo};

use super::{GmailError, GmailMessage, MessagePart};
use crate::services::normalization::RawEmail;

const MAX_MIME_DEPTH: usize = 32;
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Convert Gmail's parsed MIME tree into input for the shared local normalizer.
/// Original MIME headers/parts stay untouched in the separately cached message.
pub fn message_to_raw(message: &GmailMessage) -> Result<RawEmail, GmailError> {
    if message.id.trim().is_empty() {
        return Err(GmailError::InvalidResponse);
    }
    let mut plain = Vec::new();
    let mut html = Vec::new();
    let mut text_budget = MAX_TEXT_BYTES;
    let mut sender = None;
    let mut recipients = Vec::new();
    let mut subject = String::new();
    let mut date_header = None;
    if let Some(payload) = &message.payload {
        sender = addresses(payload, "from").into_iter().next();
        for name in ["to", "cc", "bcc"] {
            for address in addresses(payload, name) {
                let email = address.addr.trim().to_lowercase();
                if !email.is_empty() && !recipients.contains(&email) {
                    recipients.push(email);
                }
            }
        }
        subject = decoded_header(payload, "subject").unwrap_or_default();
        date_header = decoded_header(payload, "date");
        collect_text(payload, 0, &mut text_budget, &mut plain, &mut html);
    }
    let received_at = message
        .internal_date
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .or_else(|| {
            date_header
                .as_deref()
                .and_then(|value| mailparse::dateparse(value).ok())
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
        })
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let (sender_name, sender_email) = sender
        .map(|sender| (sender.display_name.unwrap_or_default(), sender.addr))
        .unwrap_or_default();
    Ok(RawEmail {
        gmail_message_id: message.id.clone(),
        gmail_thread_id: if message.thread_id.is_empty() {
            message.id.clone()
        } else {
            message.thread_id.clone()
        },
        sender_name,
        sender_email,
        recipients,
        subject,
        received_at,
        snippet: message.snippet.clone(),
        body_text: (!plain.is_empty()).then(|| plain.join("\n\n")),
        body_html: (!html.is_empty()).then(|| html.join("\n")),
    })
}

fn header<'a>(part: &'a MessagePart, name: &str) -> Option<&'a str> {
    part.headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

fn decoded_header(part: &MessagePart, name: &str) -> Option<String> {
    let value = header(part, name)?;
    let raw = format!("X: {value}");
    Some(
        mailparse::parse_header(raw.as_bytes())
            .map(|(header, _)| header.get_value())
            .unwrap_or_else(|_| value.to_string()),
    )
}

fn addresses(part: &MessagePart, name: &str) -> Vec<SingleInfo> {
    let mut result = Vec::new();
    for value in part
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(name))
    {
        let raw = format!("X: {}", value.value);
        if let Ok((parsed, _)) = mailparse::parse_header(raw.as_bytes()) {
            if let Ok(addresses) = mailparse::addrparse_header(&parsed) {
                for address in addresses.iter() {
                    match address {
                        MailAddr::Single(single) => result.push(single.clone()),
                        MailAddr::Group(group) => result.extend(group.addrs.iter().cloned()),
                    }
                }
            }
        }
    }
    result
}

fn collect_text(
    part: &MessagePart,
    depth: usize,
    budget: &mut usize,
    plain: &mut Vec<String>,
    html: &mut Vec<String>,
) {
    if depth > MAX_MIME_DEPTH || *budget == 0 || is_attachment(part) {
        return;
    }
    let mime = part.mime_type.split(';').next().unwrap_or_default().trim();
    if mime.eq_ignore_ascii_case("text/plain") || mime.eq_ignore_ascii_case("text/html") {
        if let Some(text) = decode_body(part, *budget).filter(|text| !text.trim().is_empty()) {
            *budget = budget.saturating_sub(text.len());
            if mime.eq_ignore_ascii_case("text/plain") {
                plain.push(text);
            } else {
                html.push(text);
            }
        }
    }
    // Do not walk embedded messages, calendar parts or other attachment-like
    // content. Mixed/alternative/related containers may hold the visible body.
    if mime.to_ascii_lowercase().starts_with("multipart/") || mime.is_empty() {
        for child in &part.parts {
            collect_text(child, depth + 1, budget, plain, html);
        }
    }
}

fn is_attachment(part: &MessagePart) -> bool {
    !part.filename.trim().is_empty()
        || part.body.attachment_id.is_some()
        || header(part, "content-disposition").is_some_and(|value| {
            let parsed = mailparse::parse_content_disposition(value);
            parsed.disposition == mailparse::DispositionType::Attachment
                || parsed.params.contains_key("filename")
        })
        || header(part, "content-type").is_some_and(|value| {
            mailparse::parse_content_type(value)
                .params
                .contains_key("name")
        })
}

fn decode_body(part: &MessagePart, budget: usize) -> Option<String> {
    let encoded = part.body.data.as_deref()?;
    if encoded.len() > budget.saturating_mul(4).saturating_div(3).saturating_add(4) {
        return None;
    }
    // Gmail already removes MIME transfer encoding. Only its base64url envelope
    // is decoded here; treating quoted-printable a second time corrupts text.
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| URL_SAFE.decode(encoded))
        .ok()?;
    let content_type = header(part, "content-type").map(mailparse::parse_content_type);
    let charset = content_type
        .as_ref()
        .and_then(|value| value.params.get("charset"));
    let encoding = charset
        .and_then(|value| Encoding::for_label(value.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (decoded, _, had_errors) = encoding.decode(&bytes);
    if had_errors || decoded.len() > budget {
        return None;
    }
    Some(decoded.into_owned())
}
