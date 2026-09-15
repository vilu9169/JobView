/// Ordered progression; terminal outcomes are handled separately. Keeping stage
/// names and ranking together prevents divergent ordering across event types.
const PROGRESSION: &[&str] = &[
    "discovered",
    "preparing",
    "applied",
    "recruiter_screen",
    "interview",
    "technical_test",
    "final_interview",
    "offer",
];
const TERMINAL: &[&str] = &["offer", "rejected", "withdrawn"];

pub fn is_valid_stage(stage: &str) -> bool {
    PROGRESSION.contains(&stage) || TERMINAL.contains(&stage)
}

pub fn stage_for_event(event_type: &str) -> Option<&'static str> {
    match event_type {
        "application_submitted" | "application_confirmed" => Some("applied"),
        "recruiter_contact" => Some("recruiter_screen"),
        "interview_requested" | "interview_scheduled" => Some("interview"),
        "assessment_requested" | "assessment_completed" => Some("technical_test"),
        "final_interview" => Some("final_interview"),
        "offer_received" => Some("offer"),
        "rejected" => Some("rejected"),
        "withdrawn" => Some("withdrawn"),
        _ => None,
    }
}

/// The repository still records the event even if this projection keeps the
/// current stage. It checks event dates before calling this function as well.
pub fn infer_stage(current: &str, event_type: &str, manual_override: bool) -> String {
    if manual_override || TERMINAL.contains(&current) {
        return current.to_string();
    }
    let Some(suggested) = stage_for_event(event_type) else {
        return current.to_string();
    };
    if TERMINAL.contains(&suggested) {
        return suggested.to_string();
    }
    let current_rank = PROGRESSION.iter().position(|stage| *stage == current);
    let suggested_rank = PROGRESSION.iter().position(|stage| *stage == suggested);
    match (current_rank, suggested_rank) {
        (Some(current_rank), Some(suggested_rank)) if suggested_rank > current_rank => {
            suggested.to_string()
        }
        // Invalid data is not silently "fixed" by an email. The repository
        // validates user writes; unknown stages remain inspectable on read.
        _ => current.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_events_advance_but_confirmations_do_not_regress_progress() {
        assert_eq!(
            infer_stage("applied", "interview_requested", false),
            "interview"
        );
        assert_eq!(
            infer_stage("interview", "application_confirmed", false),
            "interview"
        );
        assert_eq!(
            infer_stage("technical_test", "interview_scheduled", false),
            "technical_test"
        );
    }

    #[test]
    fn every_manual_stage_takes_precedence_even_over_rejections_and_offers() {
        for stage in PROGRESSION.iter().chain(TERMINAL.iter()) {
            for event in [
                "application_confirmed",
                "interview_requested",
                "offer_received",
                "rejected",
            ] {
                assert_eq!(infer_stage(stage, event, true), *stage);
            }
        }
    }

    #[test]
    fn terminal_states_require_user_change_and_unknown_events_do_nothing() {
        for stage in TERMINAL {
            assert_eq!(infer_stage(stage, "interview_requested", false), *stage);
            assert_eq!(infer_stage(stage, "rejected", false), *stage);
        }
        assert_eq!(infer_stage("applied", "note", false), "applied");
        assert_eq!(infer_stage("applied", "stage_changed", false), "applied");
        assert_eq!(infer_stage("interview", "rejected", false), "rejected");
        assert_eq!(
            infer_stage("invalid", "interview_requested", false),
            "invalid"
        );
        assert!(is_valid_stage("recruiter_screen"));
        assert!(!is_valid_stage("screening"));
    }
}
