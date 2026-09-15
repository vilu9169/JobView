//! Conservative matching proposals for milestone 3. Milestone 1 associations
//! are always explicit repository commands; this service never writes a link.

use chrono::DateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct MatchingEmail {
    pub extracted_company: Option<String>,
    pub extracted_role: Option<String>,
    pub sender_email: String,
    pub gmail_thread_id: String,
    pub received_at: String,
    pub manual_application_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MatchingApplication {
    pub id: String,
    pub company: String,
    pub role: String,
    pub company_domains: Vec<String>,
    pub known_sender_addresses: Vec<String>,
    pub linked_thread_ids: Vec<String>,
    pub last_activity_at: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSuggestion {
    pub application_id: String,
    pub confidence: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchingResult {
    pub manual_application_id: Option<String>,
    pub suggestions: Vec<MatchSuggestion>,
    pub ambiguous: bool,
}

pub trait ApplicationMatchingService: Send + Sync {
    fn suggest(
        &self,
        email: &MatchingEmail,
        applications: &[MatchingApplication],
    ) -> MatchingResult;
}

#[derive(Debug, Default)]
pub struct ConservativeMatchingService;

impl ApplicationMatchingService for ConservativeMatchingService {
    fn suggest(
        &self,
        email: &MatchingEmail,
        applications: &[MatchingApplication],
    ) -> MatchingResult {
        if let Some(manual_id) = &email.manual_application_id {
            return MatchingResult {
                manual_application_id: Some(manual_id.clone()),
                suggestions: Vec::new(),
                ambiguous: false,
            };
        }

        let sender = email.sender_email.trim().to_lowercase();
        let domain = sender.rsplit_once('@').map(|(_, domain)| domain);
        let mut suggestions = Vec::new();
        for application in applications
            .iter()
            .filter(|application| !application.archived)
        {
            let mut score: f64 = 0.0;
            let mut reasons = Vec::new();
            let thread_match = !email.gmail_thread_id.is_empty()
                && application
                    .linked_thread_ids
                    .contains(&email.gmail_thread_id);
            if thread_match {
                score = 0.96;
                reasons.push("previously_linked_thread".into());
            }
            let company_matches = email
                .extracted_company
                .as_deref()
                .map(|company| equal_name(company, &application.company));
            let role_matches = email
                .extracted_role
                .as_deref()
                .map(|role| equal_name(role, &application.role));
            if company_matches == Some(true) {
                score += 0.38;
                reasons.push("exact_company".into());
            }
            if role_matches == Some(true) {
                score += 0.35;
                reasons.push("exact_role".into());
            }
            if !sender.is_empty()
                && application
                    .known_sender_addresses
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(&sender))
            {
                score += 0.25;
                reasons.push("previously_linked_sender".into());
            }
            if domain.is_some_and(|domain| {
                !is_shared_mail_provider(domain)
                    && application
                        .company_domains
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(domain))
            }) {
                score += 0.20;
                reasons.push("exact_company_domain".into());
            }
            // Recency can support a real identity signal, never create a match.
            if score > 0.0 && is_recent(&email.received_at, application.last_activity_at.as_deref())
            {
                score += 0.04;
                reasons.push("activity_within_90_days".into());
            }
            if company_matches == Some(false) {
                score = score.min(0.30);
                reasons.push("conflicting_company".into());
            }
            if role_matches == Some(false) {
                score = score.min(0.45);
                reasons.push("conflicting_role".into());
            }
            // An exact generic role alone is weak evidence of identity.
            if reasons.len() == 1 && role_matches == Some(true) {
                score = score.min(0.35);
            }
            if score >= 0.20 {
                suggestions.push(MatchSuggestion {
                    application_id: application.id.clone(),
                    confidence: (score.min(0.99) * 100.0).round() / 100.0,
                    reasons,
                });
            }
        }
        suggestions.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.application_id.cmp(&right.application_id))
        });
        let ambiguous =
            suggestions
                .first()
                .zip(suggestions.get(1))
                .is_some_and(|(first, second)| {
                    second.confidence >= 0.50 && first.confidence - second.confidence < 0.15
                });
        MatchingResult {
            manual_application_id: None,
            suggestions,
            ambiguous,
        }
    }
}

fn equal_name(left: &str, right: &str) -> bool {
    fn normalize(value: &str) -> String {
        value
            .to_lowercase()
            .chars()
            .map(|character| {
                if character.is_alphanumeric() {
                    character
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    let left = normalize(left);
    !left.is_empty() && left == normalize(right)
}

fn is_shared_mail_provider(domain: &str) -> bool {
    [
        "gmail.com",
        "googlemail.com",
        "outlook.com",
        "hotmail.com",
        "live.com",
        "yahoo.com",
        "icloud.com",
        "proton.me",
        "protonmail.com",
    ]
    .contains(&domain)
}

fn is_recent(received_at: &str, activity_at: Option<&str>) -> bool {
    let Some(activity_at) = activity_at else {
        return false;
    };
    let (Ok(received), Ok(activity)) = (
        DateTime::parse_from_rfc3339(received_at),
        DateTime::parse_from_rfc3339(activity_at),
    ) else {
        return false;
    };
    (received - activity).num_days().unsigned_abs() <= 90
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application(id: &str, role: &str) -> MatchingApplication {
        MatchingApplication {
            id: id.into(),
            company: "Northstar Labs".into(),
            role: role.into(),
            company_domains: vec!["northstar.example".into()],
            last_activity_at: Some("2026-09-02T09:00:00Z".into()),
            ..Default::default()
        }
    }

    fn email() -> MatchingEmail {
        MatchingEmail {
            extracted_company: Some("Northstar Labs".into()),
            extracted_role: Some("Frontend Engineer".into()),
            sender_email: "hiring@northstar.example".into(),
            received_at: "2026-09-08T09:00:00Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn exact_company_role_and_domain_beat_other_roles_at_same_company() {
        let result = ConservativeMatchingService.suggest(
            &email(),
            &[
                application("frontend", "Frontend Engineer"),
                application("data", "Data Engineer"),
            ],
        );
        assert_eq!(result.suggestions[0].application_id, "frontend");
        assert!(result.suggestions[0].confidence >= 0.90);
        assert!(result.suggestions[1].confidence <= 0.45);
        assert!(!result.ambiguous);
    }

    #[test]
    fn equal_candidates_are_explicitly_ambiguous_and_ties_are_stable() {
        let result = ConservativeMatchingService.suggest(
            &email(),
            &[
                application("second", "Frontend Engineer"),
                application("first", "Frontend Engineer"),
            ],
        );
        assert!(result.ambiguous);
        assert_eq!(result.suggestions[0].application_id, "first");
        assert_eq!(result.suggestions.len(), 2);
    }

    #[test]
    fn manual_association_always_wins_including_archived_applications() {
        let mut email = email();
        email.manual_application_id = Some("chosen-by-user".into());
        let result = ConservativeMatchingService
            .suggest(&email, &[application("other", "Frontend Engineer")]);
        assert_eq!(
            result.manual_application_id.as_deref(),
            Some("chosen-by-user")
        );
        assert!(result.suggestions.is_empty());
    }

    #[test]
    fn recency_and_shared_provider_domain_cannot_establish_identity() {
        let mut application = application("one", "Frontend Engineer");
        application.company_domains = vec!["gmail.com".into()];
        let email = MatchingEmail {
            sender_email: "stranger@gmail.com".into(),
            received_at: "2026-09-08T09:00:00Z".into(),
            ..Default::default()
        };
        assert!(ConservativeMatchingService
            .suggest(&email, &[application])
            .suggestions
            .is_empty());
    }

    #[test]
    fn previous_thread_is_strong_but_conflicting_role_requires_review() {
        let mut application = application("one", "Frontend Engineer");
        application.linked_thread_ids = vec!["known-thread".into()];
        let mut email = MatchingEmail {
            gmail_thread_id: "known-thread".into(),
            ..Default::default()
        };
        let result = ConservativeMatchingService.suggest(&email, &[application.clone()]);
        assert_eq!(result.suggestions[0].confidence, 0.96);
        email.extracted_role = Some("Data Engineer".into());
        let result = ConservativeMatchingService.suggest(&email, &[application]);
        assert!(result.suggestions[0].confidence <= 0.45);
    }

    #[test]
    fn archived_applications_are_not_new_suggestions_and_subdomains_are_not_fuzzy_matches() {
        let mut archived = application("archived", "Frontend Engineer");
        archived.archived = true;
        assert!(ConservativeMatchingService
            .suggest(&email(), &[archived])
            .suggestions
            .is_empty());
        let spoof = MatchingEmail {
            sender_email: "hiring@northstar.example.attacker.example".into(),
            ..Default::default()
        };
        assert!(ConservativeMatchingService
            .suggest(&spoof, &[application("one", "Frontend Engineer")])
            .suggestions
            .is_empty());
    }
}
