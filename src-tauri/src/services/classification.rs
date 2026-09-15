use chrono::NaiveDate;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use super::normalization::NormalizedEmail;
use super::stages::stage_for_event;

pub trait ClassificationService: Send + Sync {
    fn classify(&self, email: &NormalizedEmail) -> ClassificationResult;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassificationResult {
    pub is_job_related: bool,
    pub category: String,
    pub event_type: Option<String>,
    pub company: Option<String>,
    pub role: Option<String>,
    pub suggested_stage: Option<String>,
    pub requires_action: bool,
    pub suggested_action: Option<String>,
    pub deadline: Option<String>,
    pub confidence: f64,
    pub reasoning_code: String,
}

/// Deterministic, local rules. Confidence is a rule strength, not a probability.
#[derive(Debug, Default)]
pub struct LocalClassificationService;

impl ClassificationService for LocalClassificationService {
    fn classify(&self, email: &NormalizedEmail) -> ClassificationResult {
        let subject = email.subject.to_lowercase();
        // Visible prose carries intent. Tracking URLs and account-reset query
        // strings must not manufacture recruitment signals.
        let content = without_urls(&format!("{}\n{}", subject, email.body_text.to_lowercase()));
        let sender = format!("{} {}", email.sender_name, email.sender_email).to_lowercase();
        let application_context = contains_any(
            &content,
            &[
                "your application",
                "thank you for applying",
                "thanks for applying",
                "application received",
                "application submitted",
                "application confirmation",
                "application for",
                "candidate for",
                "candidacy",
                "hiring process",
                "recruitment process",
                "offer of employment",
                "job offer",
                "din ansökan",
                "er ansökan",
                "ansökan till",
                "ansökan för",
                "tack för att du sökt",
                "tack för att du har sökt",
                "tack för att du söker",
                "tack för ditt intresse för tjänsten",
                "rekryteringsprocess",
                "tjänsten som",
                "anställningserbjudande",
            ],
        );
        let hiring_sender = contains_any(
            &sender,
            &[
                "recruiter",
                "recruiting",
                "recruitment",
                "talent acquisition",
                "hiring team",
                "careers@",
                "jobs@",
                "hiring@",
                "talent@",
                "rekryter",
                "karriär",
                "karriar",
                "jobb@",
            ],
        );
        let hiring_context = application_context
            || hiring_sender
            || contains_any(
                &content,
                &[
                    "hiring team",
                    "recruiter",
                    "talent acquisition",
                    "open position",
                    "job opportunity",
                    "join our team",
                    "for the position",
                    "for the role",
                    "another candidate",
                    "för tjänsten",
                    "för rollen",
                    "en ledig tjänst",
                    "en tjänst som",
                    "en roll som",
                    "jobbmöjlighet",
                    "andra kandidater",
                    "annan kandidat",
                    "rekryterare",
                    "vi rekryterar",
                    "rekryterar till",
                ],
            );
        let mut result = ClassificationResult {
            is_job_related: false,
            category: "other".into(),
            event_type: None,
            company: None,
            role: None,
            suggested_stage: None,
            requires_action: false,
            suggested_action: None,
            deadline: None,
            confidence: 0.25,
            reasoning_code: "no_specific_job_signal".into(),
        };
        // Advice often says "your application" / "din ansökan" too. Only a
        // concrete personal update can outweigh bulk/commercial indicators.
        let personal_update = contains_any(
            &content,
            &[
                "received your application",
                "thank you for applying",
                "thanks for applying",
                "application received",
                "your interview is",
                "invite you to an interview",
                "pleased to offer you",
                "delighted to offer you",
                "offer of employment",
                "decided not to proceed with your application",
                "tack för din ansökan",
                "tagit emot din ansökan",
                "mottagit din ansökan",
                "tack för att du sökt",
                "tack för att du har sökt",
                "din intervju är",
                "bjuda in dig till intervju",
                "bjuder in dig till intervju",
                "anställningserbjudande",
                "erbjudande om anställning",
                "inte gå vidare med din ansökan",
                "inte att gå vidare med din ansökan",
            ],
        );

        // Portal/security mail is not evidence of a recruitment conversation,
        // even when it comes from an ATS or contains a recruiting signature.
        if contains_any(
            &content,
            &[
                "reset your password",
                "change your password",
                "password reset",
                "reset password",
                "one-time password",
                "verification code",
                "verify your email",
                "activate your account",
                "account activation",
                "återställ ditt lösenord",
                "återställa ditt lösenord",
                "återställ lösenord",
                "återställning av lösenord",
                "lösenordsåterställning",
                "ändra ditt lösenord",
                "byta lösenord",
                "byt lösenord",
                "engångskod",
                "verifieringskod",
                "bekräfta din e-post",
                "verifiera din e-post",
                "aktivera ditt konto",
            ],
        ) && (!personal_update
            || contains_any(
                &subject,
                &[
                    "password",
                    "lösenord",
                    "verification code",
                    "verifieringskod",
                    "aktivera ditt konto",
                ],
            ))
        {
            result.category = "account_notification".into();
            result.confidence = 0.98;
            result.reasoning_code = "account_security_notification".into();
            return result;
        }

        let promotion = contains_any(
            &content,
            &[
                "discount code",
                "discountcode",
                "discount applies",
                "discount is",
                "annual plans",
                "annual subscription",
                "subscription plans",
                "promo code",
                "back to school sale",
                "limited time",
                "% off",
                "% rabatt",
                "rabattkod",
                "kampanjkod",
                "prenumeration",
                "rea på",
                "köp nu",
            ],
        ) && contains_any(
            &content,
            &[
                "courses",
                "learning",
                "subscription",
                "purchase",
                "checkout",
                "shop now",
                "sale",
                "pro student",
                "kurser",
                "utbildning",
                "prenumeration",
                "handla",
                "rabatt",
                "erbjudanden",
            ],
        );
        if promotion && !personal_update {
            result.category = "promotion".into();
            result.confidence = 0.97;
            result.reasoning_code = "commercial_promotion_without_application_context".into();
            return result;
        }

        // Bulk career advice may discuss interviews, offers and hiring. An actual
        // personal application signal is needed before bypassing this filter.
        if !personal_update
            && contains_any(
                &content,
                &[
                    "newsletter",
                    "weekly digest",
                    "monthly digest",
                    "this week's digest",
                    "view our latest articles",
                    "you are receiving this digest",
                    "nyhetsbrev",
                    "veckans nyheter",
                    "veckans jobbtips",
                    "jobbavisering",
                    "jobbevakning",
                    "jobb som matchar",
                    "nya jobb som",
                    "job alert",
                    "jobs matching your",
                ],
            )
        {
            result.category = "newsletter".into();
            result.confidence = 0.98;
            result.reasoning_code = "bulk_newsletter_without_application_context".into();
            return result;
        }
        if !personal_update
            && contains_any(
                &content,
                &[
                    "your receipt",
                    "payment receipt",
                    "order confirmation",
                    "your order has",
                    "thanks for your purchase",
                    "thank you for your purchase",
                    "ditt kvitto",
                    "betalningskvitto",
                    "orderbekräftelse",
                    "tack för ditt köp",
                    "din beställning",
                ],
            )
        {
            result.category = "receipt".into();
            result.confidence = 0.99;
            result.reasoning_code = "purchase_receipt_without_application_context".into();
            return result;
        }

        let rejection = hiring_context
            && contains_any(
                &content,
                &[
                    "decided not to proceed",
                    "decided not to move forward",
                    "will not be moving forward",
                    "unable to move forward with your application",
                    "move forward with another candidate",
                    "moving forward with another candidate",
                    "position has been filled",
                    "not selected for",
                    "application was unsuccessful",
                    "inte gå vidare med din ansökan",
                    "inte vidare med din ansökan",
                    "inte gå vidare med dig",
                    "inte att gå vidare med dig",
                    "inte att gå vidare med din ansökan",
                    "inte går vidare",
                    "gå vidare med andra kandidater",
                    "gå vidare med en annan kandidat",
                    "gått vidare med andra kandidater",
                    "valt andra kandidater",
                    "valt att gå vidare med andra",
                    "tjänsten har tillsatts",
                    "tjänsten är tillsatt",
                    "tjänsten har blivit tillsatt",
                    "inte erbjuda dig tjänsten",
                    "inte erbjuda dig en anställning",
                    "inte aktuell för tjänsten",
                    "inte är aktuell för tjänsten",
                    "din ansökan har fått avslag",
                    "gå vidare med andra sökande",
                ],
            );
        let offer = hiring_context
            && contains_any(
                &content,
                &[
                    "offer of employment",
                    "job offer",
                    "pleased to offer you the position",
                    "delighted to offer you the role",
                    "formal employment offer",
                    "anställningserbjudande",
                    "erbjudande om anställning",
                    "vill erbjuda dig tjänsten",
                    "erbjuder dig tjänsten",
                    "erbjuda dig en anställning",
                    "erbjuder dig en anställning",
                ],
            );
        let assessment = hiring_context
            && contains_any(
                &content,
                &[
                    "coding test",
                    "technical assessment",
                    "take-home assignment",
                    "take home assignment",
                    "complete a case study",
                    "complete the case study",
                    "arbetsprov",
                    "kodtest",
                    "programmeringstest",
                    "tekniskt test",
                    "personlighetstest",
                    "logiktest",
                    "färdighetstest",
                    "begåvningstest",
                    "rekryteringstest",
                    "testinbjudan",
                ],
            );
        let interview = hiring_context
            && contains_any(
                &content,
                &[
                    "interview",
                    "phone screen",
                    "screening call",
                    "intervju",
                    "telefonavstämning",
                ],
            );
        let confirmation = contains_any(
            &content,
            &[
                "thank you for applying",
                "thanks for applying",
                "application received",
                "application submitted",
                "application confirmation",
                "received your application",
                "tack för din ansökan",
                "tack för er ansökan",
                "tack för att du sökt",
                "tack för att du har sökt",
                "tagit emot din ansökan",
                "mottagit din ansökan",
                "mottagit er ansökan",
                "ansökan är mottagen",
                "ansökan har registrerats",
                "ansökan är registrerad",
                "ansökningsbekräftelse",
            ],
        );
        let explicit_reply = contains_any(
            &content,
            &[
                "please reply",
                "please respond",
                "reply by",
                "respond by",
                "please let me know",
                "please let us know",
                "send your availability",
                "share your availability",
                "please choose",
                "please select",
                "book a time",
                "schedule a call",
                "schedule an interview",
                "please confirm",
                "vänligen svara",
                "svara senast",
                "återkom senast",
                "återkom gärna",
                "återkom med",
                "hör gärna av dig",
                "hör av dig med",
                "meddela oss",
                "meddela mig",
                "bekräfta gärna",
                "vänligen bekräfta",
                "boka en tid",
                "boka tid",
                "välj en tid",
                "vilka tider passar",
                "skicka ditt cv",
                "skicka in ditt cv",
                "komplettera din ansökan",
            ],
        );

        let rule: Option<(&str, &str, f64, &str, Option<&str>)> = if rejection {
            Some((
                "rejection",
                "rejected",
                0.98,
                "explicit_rejection_with_hiring_context",
                None,
            ))
        } else if offer {
            Some((
                "offer",
                "offer_received",
                0.98,
                "explicit_employment_offer",
                Some("Review the employment offer and respond"),
            ))
        } else if assessment {
            if contains_any(
                &content,
                &[
                    "assessment received",
                    "received your assessment",
                    "assessment is complete",
                    "assignment received",
                    "received your completed",
                    "mottagit ditt arbetsprov",
                    "tagit emot ditt arbetsprov",
                    "arbetsprov är mottaget",
                    "genomfört testet",
                    "genomfört testerna",
                    "slutfört testet",
                    "testresultat är mottagna",
                    "tack för ditt arbetsprov",
                    "genomfört våra test",
                    "genomfört dina test",
                    "testet är slutfört",
                    "testerna är slutförda",
                    "arbetsprov har tagits emot",
                ],
            ) {
                Some((
                    "assessment",
                    "assessment_completed",
                    0.96,
                    "assessment_completion_confirmed",
                    None,
                ))
            } else {
                let requested = contains_any(
                    &content,
                    &[
                        "please complete",
                        "complete the",
                        "complete a",
                        "invited to",
                        "next step",
                        "submit",
                        "assigned",
                        "genomför ",
                        "slutför ",
                        "gör testet",
                        "göra testet",
                        "göra ett",
                        "genomföra",
                        "skicka in",
                        "lämna in",
                        "nästa steg",
                        "inbjudan",
                        "inbjuder",
                        "bjuder in",
                    ],
                );
                if requested {
                    Some((
                        "assessment",
                        "assessment_requested",
                        0.97,
                        "assessment_request_with_hiring_context",
                        Some("Complete and submit the assessment"),
                    ))
                } else {
                    None
                }
            }
        } else if interview {
            if contains_any(
                &content,
                &[
                    "final interview",
                    "final round interview",
                    "slutintervju",
                    "sista intervjun",
                ],
            ) && (explicit_reply
                || contains_any(
                    &content,
                    &[
                        "confirmed",
                        "scheduled",
                        "invite you",
                        "bekräftad",
                        "inbokad",
                        "bjuder in",
                        "inbjudan",
                    ],
                ))
            {
                Some((
                    "interview",
                    "final_interview",
                    0.96,
                    "explicit_final_interview",
                    if explicit_reply {
                        Some("Confirm your availability for the final interview")
                    } else {
                        None
                    },
                ))
            } else if contains_any(
                &content,
                &[
                    "interview is confirmed",
                    "interview confirmed",
                    "interview is scheduled",
                    "interview scheduled",
                    "screen is confirmed",
                    "screening call is confirmed",
                    "intervju är bokad",
                    "intervju är inbokad",
                    "intervjun är bokad",
                    "intervjun är inbokad",
                    "intervju är bekräftad",
                    "intervjun är bekräftad",
                    "bekräftelse på din intervju",
                    "intervjubekräftelse",
                    "bekräftar din intervju",
                    "bekräftar härmed din intervju",
                ],
            ) {
                Some((
                    "interview",
                    "interview_scheduled",
                    0.97,
                    "interview_time_confirmed",
                    if explicit_reply {
                        Some("Confirm the interview details")
                    } else {
                        None
                    },
                ))
            } else if explicit_reply
                || contains_any(
                    &content,
                    &[
                        "invite you to",
                        "interview invitation",
                        "invitation to interview",
                        "would like to interview",
                        "inbjudan till intervju",
                        "intervjuinbjudan",
                        "bjuda in dig",
                        "bjuder in dig",
                        "välkommen på intervju",
                        "träffa dig för en intervju",
                    ],
                )
            {
                Some((
                    "interview",
                    "interview_requested",
                    0.96,
                    "interview_invitation_with_hiring_context",
                    Some("Reply with your interview availability"),
                ))
            } else {
                None
            }
        } else if confirmation {
            Some((
                "application_confirmation",
                "application_confirmed",
                0.99,
                "explicit_application_confirmation",
                None,
            ))
        } else if hiring_context
            && contains_any(
                &content,
                &[
                    "schedule a call",
                    "discuss the role",
                    "discuss this role",
                    "discuss an opportunity",
                    "your profile caught",
                    "your experience caught",
                    "your background caught",
                    "impressed by your",
                    "supporting your application",
                    "would you be interested",
                    "are you interested in",
                    "talk to you about",
                    "såg din profil",
                    "sett din profil",
                    "din profil fångade",
                    "din profil väckte",
                    "din bakgrund verkar",
                    "din bakgrund passar",
                    "läst ditt cv",
                    "läst din ansökan",
                    "dina erfarenheter passar",
                    "skicka ditt cv",
                    "komplettera din ansökan",
                    "prata med dig om",
                    "berätta mer om tjänsten",
                    "berätta mer om rollen",
                    "intresserad av tjänsten",
                    "intresserad av en tjänst",
                    "intresserad av en roll",
                    "boka ett samtal",
                    "boka ett möte",
                ],
            )
        {
            Some((
                "recruiter_contact",
                "recruiter_contact",
                0.91,
                "recruiter_contact_with_hiring_context",
                if explicit_reply {
                    Some("Reply to the recruiter")
                } else {
                    None
                },
            ))
        } else {
            None
        };

        if let Some((category, event, confidence, reason, action)) = rule {
            result.is_job_related = true;
            result.category = category.into();
            result.event_type = Some(event.into());
            result.suggested_stage = stage_for_event(event).map(str::to_string);
            result.confidence = confidence;
            result.reasoning_code = reason.into();
            result.suggested_action = action.map(str::to_string);
            result.requires_action = action.is_some();
        } else if application_context {
            result.is_job_related = true;
            result.category = "job_related".into();
            result.confidence = 0.62;
            result.reasoning_code = "hiring_context_without_clear_event".into();
        }
        if result.is_job_related {
            (result.company, result.role) = extract_company_role(email);
        }
        if result.requires_action {
            result.deadline = explicit_deadline(&content);
        }
        result
    }
}

fn without_urls(value: &str) -> String {
    static URL: OnceLock<Regex> = OnceLock::new();
    let pattern = URL.get_or_init(|| {
        Regex::new(r"https?://[^\s<>]+|www\.[^\s<>]+").expect("static URL pattern")
    });
    pattern
        .replace_all(value, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn extract_company_role(email: &NormalizedEmail) -> (Option<String>, Option<String>) {
    let mut company = None;
    let mut role = None;
    for line in email.body_text.lines() {
        if let Some((label, value)) = line.split_once(':') {
            let value = value.trim();
            if !(2..=100).contains(&value.chars().count()) {
                continue;
            }
            match label.trim().to_lowercase().as_str() {
                "company" | "företag" | "arbetsgivare" => company = Some(value.to_string()),
                "role" | "position" | "job title" | "roll" | "tjänst" | "befattning" => {
                    role = Some(value.to_string())
                }
                _ => {}
            }
        }
    }
    // Common ATS subject: "Application received: Frontend Engineer at Acme".
    // Require a recognizable title, and never turn a sender domain into a company.
    let title = email
        .subject
        .rsplit_once(':')
        .map_or(email.subject.as_str(), |(_, value)| value)
        .trim();
    if let Some((title, employer)) = title
        .split_once(" at ")
        .or_else(|| title.split_once(" hos "))
        .or_else(|| title.split_once(" på "))
    {
        let title = title
            .trim_start_matches("Din ansökan till ")
            .trim_start_matches("Din ansökan som ")
            .trim_start_matches("Tjänsten som ");
        let known_role = contains_any(
            &title.to_lowercase(),
            &[
                "engineer",
                "developer",
                "designer",
                "analyst",
                "manager",
                "specialist",
                "coordinator",
                "researcher",
                "scientist",
                "consultant",
                "architect",
                "accountant",
                "associate",
                "intern",
                "director",
                "assistant",
                "utvecklare",
                "ingenjör",
                "analytiker",
                "projektledare",
                "chef",
                "samordnare",
                "forskare",
                "konsult",
                "arkitekt",
                "ekonom",
                "assistent",
                "sjuksköterska",
                "lärare",
                "säljare",
                "tekniker",
                "praktikant",
            ],
        );
        if known_role
            && title.len() <= 100
            && (2..=100).contains(&employer.len())
            && !employer.contains('@')
        {
            role.get_or_insert_with(|| title.trim().to_string());
            company.get_or_insert_with(|| employer.trim().to_string());
        }
    }
    (company, role)
}

fn explicit_deadline(content: &str) -> Option<String> {
    static DATE_PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    let pattern = DATE_PATTERN.get_or_init(|| Regex::new(
        r"(?i)\b(?:reply by|respond by|submit by|complete by|deadline(?: is|:)?|due(?: on|:)?|svara senast|återkom senast|senast(?: den)?|sista svarsdag(?: är|:)?|sista inlämningsdag(?: är|:)?)\s+(\d{4}-\d{2}-\d{2})\b"
    ).ok()).as_ref()?;
    if let Some(captures) = pattern.captures(content) {
        let date = captures.get(1)?.as_str();
        NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        return Some(date.into());
    }
    // Require an explicit year: a bare "fredag" or "15 september" is not
    // enough to invent a deadline from the email's received date.
    static SWEDISH_DATE: OnceLock<Regex> = OnceLock::new();
    let pattern = SWEDISH_DATE.get_or_init(|| Regex::new(
        r"(?i)\b(?:senast(?: den)?|sista svarsdag(?: är|:)?|sista inlämningsdag(?: är|:)?)\s+(\d{1,2})(?:\s+([a-zåäö]+)\s+|/(\d{1,2})/)(\d{4})\b"
    ).expect("static Swedish deadline pattern"));
    let captures = pattern.captures(content)?;
    let day = captures.get(1)?.as_str().parse::<u32>().ok()?;
    let month = if let Some(number) = captures.get(3) {
        number.as_str().parse::<u32>().ok()?
    } else {
        match captures.get(2)?.as_str() {
            "januari" => 1,
            "februari" => 2,
            "mars" => 3,
            "april" => 4,
            "maj" => 5,
            "juni" => 6,
            "juli" => 7,
            "augusti" => 8,
            "september" => 9,
            "oktober" => 10,
            "november" => 11,
            "december" => 12,
            _ => return None,
        }
    };
    let year = captures.get(4)?.as_str().parse::<i32>().ok()?;
    Some(
        NaiveDate::from_ymd_opt(year, month, day)?
            .format("%Y-%m-%d")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::normalization::{normalize_email, RawEmail};

    fn classify(subject: &str, body: &str, sender: &str) -> ClassificationResult {
        let raw = RawEmail {
            gmail_message_id: "test".into(),
            gmail_thread_id: "thread".into(),
            sender_name: String::new(),
            sender_email: sender.into(),
            recipients: vec![],
            subject: subject.into(),
            received_at: "2026-09-08T09:00:00Z".into(),
            snippet: String::new(),
            body_text: Some(body.into()),
            body_html: None,
        };
        LocalClassificationService.classify(&normalize_email(&raw))
    }

    #[test]
    fn swedish_recruitment_events_and_actions() {
        let cases = [
            ("Tack för din ansökan", "Vi har tagit emot din ansökan till tjänsten som systemutvecklare.", "application_confirmed", false),
            ("Din ansökan", "Vi har mottagit din ansökan och återkommer när urvalet är klart.", "application_confirmed", false),
            ("En roll som utvecklare", "Jag är rekryterare och såg din profil. Vill du boka ett samtal? Återkom gärna med tider.", "recruiter_contact", true),
            ("Inbjudan till intervju", "Vi har läst din ansökan och vill bjuda in dig till intervju för tjänsten. Välj en tid som passar.", "interview_requested", true),
            ("Intervjubekräftelse", "Din intervju är inbokad för tjänsten som analytiker. Inget svar behövs.", "interview_scheduled", false),
            ("Slutintervju", "Vi bjuder in dig till en slutintervju för rollen. Vänligen bekräfta att du kan komma.", "final_interview", true),
            ("Nästa steg i rekryteringsprocessen", "Vänligen genomför vårt logiktest och personlighetstest senast 2026-09-20.", "assessment_requested", true),
            ("Arbetsprov", "Nästa steg för tjänsten som utvecklare är ett arbetsprov. Skicka in det senast den 20 september 2026.", "assessment_requested", true),
            ("Testresultat", "Tack för att du har genomfört våra tester. Ditt personlighetstest är klart och ingår i rekryteringsprocessen.", "assessment_completed", false),
            ("Besked om din ansökan", "Tack för din ansökan. Vi har valt att gå vidare med andra kandidater.", "rejected", false),
            ("Din ansökan", "Tyvärr kommer vi inte att gå vidare med din ansökan.", "rejected", false),
            ("Din ansökan", "Tjänsten har blivit tillsatt. Tack för ditt intresse.", "rejected", false),
            ("Erbjudande om anställning", "Vi vill erbjuda dig tjänsten som systemutvecklare. Vänligen svara senast 2026-09-20.", "offer_received", true),
        ];
        for (subject, body, event, action) in cases {
            let result = classify(subject, body, "kontakt@fiction.example");
            assert_eq!(
                result.event_type.as_deref(),
                Some(event),
                "{subject}: {body}"
            );
            assert!(result.is_job_related);
            assert_eq!(result.requires_action, action, "{subject}: {body}");
        }
    }

    #[test]
    fn sanitized_learning_sale_is_not_recruiter_contact() {
        let result = classify("Back to school: 50% off", "Back to learning and your career goals. Get 50% off Pro annual plans for a limited time with a discount code. Courses across tech, AI and data. Apply what you learn to build recruiter-ready portfolio projects. Subscription purchases auto-renew at the regular price.", "news@learning.example");
        assert_eq!(result.category, "promotion");
        assert!(!result.is_job_related && !result.requires_action);
        assert!(result.event_type.is_none());
    }

    #[test]
    fn security_messages_from_ats_are_not_job_events() {
        for (subject, body) in [
            ("Account information", "Hello Alex, Please click below to change your password. https://careers.example/reset Best regards, Fictional Talent Acquisition"),
            ("Återställ ditt lösenord", "Klicka här för att återställa ditt lösenord till ditt kandidatskonto. Rekryteringsteamet"),
            ("Aktivera ditt konto", "Bekräfta din e-postadress. Du kan därefter hantera din ansökan."),
            ("Your verification code", "Use this verification code to sign in to your application portal."),
        ] {
            let result = classify(subject, body, "recruitment@ats.example");
            assert_eq!(result.category, "account_notification", "{subject}");
            assert!(!result.is_job_related && !result.requires_action);
            assert!(result.event_type.is_none() && result.suggested_stage.is_none());
        }
    }

    #[test]
    fn sender_signature_keywords_and_urls_are_not_personal_outreach() {
        for body in [
            "Improve your recruiter-ready portfolio and career goals.",
            "Best regards, Talent Acquisition",
            "Med vänlig hälsning, Rekryterare",
            "Din profil har uppdaterats.",
            "Read more https://links.example/your-application?recruiter=please-reply&event=interview",
        ] {
            let result = classify("Information", body, "recruitment@fiction.example");
            assert!(!result.is_job_related, "{body}");
        }
    }

    #[test]
    fn swedish_non_job_mail_stays_out_of_pipeline() {
        for (subject, body) in [
            ("Nyhetsbrev", "Veckans tips om intervjuer och rekryterare."),
            ("Nyhetsbrev: förbättra din ansökan", "Tips inför intervju för tjänsten du vill ha. Så skriver du ditt cv."),
            ("Nya jobb som matchar dig", "Din jobbevakning visar tjänster och intervjutips."),
            ("50% rabatt på kurser", "Använd rabattkod för våra utbildningar och bygg en karriär. Prata som en rekryterare."),
            ("Orderbekräftelse", "Tack för ditt köp av kursen om arbetsintervjuer."),
            ("Veckans intervju", "Läs vår intervju med en författare."),
        ] {
            assert!(!classify(subject, body, "nyheter@fiction.example").is_job_related, "{subject}");
        }
    }

    #[test]
    fn personal_application_confirmation_survives_bulk_footer() {
        let result = classify("Tack för din ansökan", "Vi har mottagit din ansökan till tjänsten som utvecklare. Du kan även läsa vårt nyhetsbrev för fler jobbtips.", "kontakt@fiction.example");
        assert_eq!(result.event_type.as_deref(), Some("application_confirmed"));
        let result = classify("Your application", "We have received your application for the role. If you cannot sign in to the portal, reset your password.", "recruiting@fiction.example");
        assert_eq!(result.event_type.as_deref(), Some("application_confirmed"));
    }

    #[test]
    fn swedish_explicit_fields_and_full_dates_are_extracted_conservatively() {
        for date in ["2026-09-20", "den 20 september 2026", "20/9/2026"] {
            let result = classify(
                "Inbjudan till intervju: Systemutvecklare hos Norrsken AB",
                &format!("För tjänsten som utvecklare. Vänligen svara senast {date}."),
                "hr@fiction.example",
            );
            assert_eq!(result.company.as_deref(), Some("Norrsken AB"));
            assert_eq!(result.role.as_deref(), Some("Systemutvecklare"));
            assert_eq!(result.deadline.as_deref(), Some("2026-09-20"));
        }
        for date in ["den 31 februari 2026", "fredag", "20 september"] {
            let result = classify("Intervjuinbjudan", &format!("För tjänsten som utvecklare. Vänligen svara senast {date}.\nFöretag: Norrsken AB\nBefattning: Systemutvecklare"), "hr@fiction.example");
            assert!(result.deadline.is_none());
            assert_eq!(result.company.as_deref(), Some("Norrsken AB"));
            assert_eq!(result.role.as_deref(), Some("Systemutvecklare"));
        }
    }

    #[test]
    fn swedish_quoted_old_rejection_does_not_override_new_interview() {
        let result = classify("Intervjuinbjudan", "Välkommen på intervju för tjänsten som utvecklare.\nDen 1 september 2026 skrev Rekrytering:\nVi har valt att gå vidare med andra kandidater.", "kontakt@fiction.example");
        assert_eq!(result.event_type.as_deref(), Some("interview_requested"));
    }

    #[test]
    fn fixture_set_exercises_all_required_categories_and_has_only_fictional_addresses() {
        let fixtures: Vec<RawEmail> =
            serde_json::from_str(include_str!("../../../fixtures/emails.json"))
                .expect("valid fictional fixtures");
        let results: Vec<_> = fixtures
            .iter()
            .map(|raw| LocalClassificationService.classify(&normalize_email(raw)))
            .collect();
        for required in [
            "application_confirmation",
            "recruiter_contact",
            "interview",
            "assessment",
            "rejection",
            "newsletter",
            "receipt",
            "offer",
        ] {
            assert!(
                results.iter().any(|result| result.category == required),
                "missing category {required}"
            );
        }
        assert!(fixtures
            .iter()
            .all(|raw| raw.sender_email.ends_with(".example")
                && raw
                    .recipients
                    .iter()
                    .all(|address| address.ends_with(".example"))));
        assert!(results
            .iter()
            .all(|result| (0.0..=1.0).contains(&result.confidence)));
    }

    #[test]
    fn explicit_interview_extracts_known_fields_and_deadline() {
        let result = classify("Interview invitation: Frontend Engineer at Northstar Labs", "We reviewed your application. Please reply with your availability. Reply by 2026-09-10.", "hiring@northstar.example");
        assert_eq!(result.event_type.as_deref(), Some("interview_requested"));
        assert_eq!(result.company.as_deref(), Some("Northstar Labs"));
        assert_eq!(result.role.as_deref(), Some("Frontend Engineer"));
        assert_eq!(result.deadline.as_deref(), Some("2026-09-10"));
        assert!(result.requires_action);
    }

    #[test]
    fn isolated_ambiguous_words_do_not_create_job_events() {
        for body in [
            "Unfortunately the concert was cancelled.",
            "Our special offer ends tonight.",
            "Join our community case study about urban parks.",
            "Listen to an interview with a novelist.",
        ] {
            let result = classify("An update", body, "hello@community.example");
            assert!(!result.is_job_related, "{body}");
            assert!(result.event_type.is_none());
        }
    }

    #[test]
    fn unfortunately_alone_is_not_a_rejection_even_with_hiring_context() {
        let result = classify(
            "Your application",
            "Unfortunately our recruiter is away today. We will be in touch.",
            "hiring@northstar.example",
        );
        assert!(result.is_job_related);
        assert_ne!(result.event_type.as_deref(), Some("rejected"));
    }

    #[test]
    fn old_quoted_rejection_does_not_override_new_interview() {
        let result = classify("Interview invitation: Designer at Pinecone Studio", "Please reply with availability for an interview for the role.\nOn Monday someone wrote:\nWe decided not to proceed with your application.", "hiring@pinecone.example");
        assert_eq!(result.event_type.as_deref(), Some("interview_requested"));
    }

    #[test]
    fn bulk_career_newsletter_is_not_an_application() {
        let result = classify(
            "Weekly digest: interview tips",
            "Our newsletter covers recruiter conversations, hiring teams and offer negotiation.",
            "news@digest.example",
        );
        assert_eq!(result.category, "newsletter");
        assert!(!result.is_job_related);
        assert!(!result.requires_action);
    }

    #[test]
    fn assessment_completion_and_confirmed_interview_do_not_request_action() {
        let completed = classify(
            "Technical assessment received",
            "We received your assessment for the role. Thank you.",
            "hiring@harbor.example",
        );
        assert_eq!(
            completed.event_type.as_deref(),
            Some("assessment_completed")
        );
        assert!(!completed.requires_action);
        let scheduled = classify("Interview confirmed", "Your interview is confirmed for 2026-09-12. Our hiring team looks forward to meeting you.", "hiring@northstar.example");
        assert_eq!(scheduled.event_type.as_deref(), Some("interview_scheduled"));
        assert!(scheduled.deadline.is_none());
    }

    #[test]
    fn missing_or_invalid_deadlines_are_not_invented() {
        for ending in [
            "Reply soon.",
            "Reply by 2026-02-30.",
            "We received your application on 2026-09-01.",
        ] {
            let result = classify(
                "Interview invitation",
                &format!("Please reply with interview availability for the role. {ending}"),
                "hiring@northstar.example",
            );
            assert!(result.requires_action);
            assert!(result.deadline.is_none());
            assert!(result.company.is_none());
            assert!(result.role.is_none());
        }
    }
}
