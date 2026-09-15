use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};

use crate::{
    domain::{AppSettings, ApplicationInput},
    error::{AppError, AppResult},
    services::stages::is_valid_stage,
};

pub(super) fn application(mut input: ApplicationInput) -> AppResult<ApplicationInput> {
    input.company = input.company.trim().to_owned();
    input.role = input.role.trim().to_owned();
    input.location = input.location.trim().to_owned();
    input.job_url = input.job_url.trim().to_owned();
    input.source = input.source.trim().to_owned();
    input.next_action = input
        .next_action
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned());
    for (label, value, maximum) in [
        ("Company", input.company.as_str(), 200),
        ("Role", input.role.as_str(), 200),
        ("Location", input.location.as_str(), 200),
        ("Source", input.source.as_str(), 200),
        ("Job URL", input.job_url.as_str(), 2048),
        ("Notes", input.notes.as_str(), 50_000),
        (
            "Next action",
            input.next_action.as_deref().unwrap_or_default(),
            1000,
        ),
    ] {
        if value.chars().count() > maximum {
            return Err(AppError::Validation(format!(
                "{label} must be {maximum} characters or fewer."
            )));
        }
    }
    if input.company.is_empty() || input.role.is_empty() {
        return Err(AppError::Validation(
            "Company and role are required.".into(),
        ));
    }
    if !is_valid_stage(&input.current_stage) {
        return Err(AppError::Validation(
            "Choose a supported application stage.".into(),
        ));
    }
    if !input.job_url.is_empty()
        && !(input.job_url.starts_with("https://") || input.job_url.starts_with("http://"))
    {
        return Err(AppError::Validation(
            "Job URL must start with https:// or http://.".into(),
        ));
    }
    date(input.applied_at.as_deref(), "Application date")?;
    date(input.next_action_due_at.as_deref(), "Action deadline")?;
    if input.next_action.is_none() && input.next_action_due_at.is_some() {
        return Err(AppError::Validation(
            "Add a next action before setting its deadline.".into(),
        ));
    }
    Ok(input)
}

pub(super) fn settings(settings: &AppSettings) -> AppResult<()> {
    if !(1..=5000).contains(&settings.sync_email_limit) {
        return Err(AppError::Validation(
            "Email limit must be between 1 and 5000.".into(),
        ));
    }
    let accept = settings.local_confidence_accept_threshold;
    let gemini = settings.local_confidence_gemini_threshold;
    if !accept.is_finite()
        || !gemini.is_finite()
        || !(0.0..=1.0).contains(&accept)
        || !(0.0..=accept).contains(&gemini)
    {
        return Err(AppError::Validation("Confidence thresholds must be between 0 and 1, with the lower threshold no higher than the acceptance threshold.".into()));
    }
    if !matches!(
        settings.ai_mode.as_str(),
        "off" | "uncertain" | "candidates"
    ) {
        return Err(AppError::Validation(
            "Choose Off, uncertain emails, or job candidates for AI classification.".into(),
        ));
    }
    Ok(())
}

fn date(value: Option<&str>, label: &str) -> AppResult<()> {
    if let Some(value) = value {
        let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
            AppError::Validation(format!("{label} must be a valid YYYY-MM-DD date."))
        })?;
        if parsed.format("%Y-%m-%d").to_string() != value {
            return Err(AppError::Validation(format!(
                "{label} must use YYYY-MM-DD."
            )));
        }
    }
    Ok(())
}

pub(super) fn timestamp(value: &str) -> AppResult<String> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| {
            date.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .map_err(|_| {
            AppError::Validation("The imported email has an invalid received timestamp.".into())
        })
}

pub(super) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
