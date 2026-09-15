use super::*;
use tempfile::TempDir;

async fn database() -> (Repository, TempDir) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = Repository::open(directory.path().join("workspace.sqlite3"))
        .await
        .expect("migrate database");
    (repository, directory)
}

fn application(company: &str) -> ApplicationInput {
    ApplicationInput {
        id: None,
        company: company.into(),
        role: "Frontend Engineer".into(),
        location: "Stockholm".into(),
        job_url: "https://jobs.example/engineer".into(),
        source: "Careers page".into(),
        applied_at: Some("2026-09-01".into()),
        current_stage: "applied".into(),
        next_action: None,
        next_action_due_at: None,
        notes: "A fictional application".into(),
        archived: false,
        source_email_id: None,
    }
}

fn fixture_id(snapshot: &WorkspaceSnapshot, suffix: &str) -> String {
    snapshot
        .emails
        .iter()
        .find(|email| email.gmail_message_id == format!("fixture-{suffix}-v1"))
        .expect("fixture exists")
        .id
        .clone()
}

#[tokio::test]
async fn gmail_migration_upgrades_existing_tracker_without_losing_manual_data() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("upgrade.sqlite3");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("legacy database");
    let all = sqlx::migrate!("./migrations");
    let mut legacy = sqlx::migrate::Migrator::DEFAULT;
    legacy.migrations = std::borrow::Cow::Owned(
        all.iter()
            .filter(|migration| migration.version == 1)
            .cloned()
            .collect(),
    );
    legacy.run(&pool).await.expect("original migration");
    sqlx::query("INSERT INTO job_applications (id, company, role, location, job_url, source, current_stage, initial_stage, notes, stage_manually_set, created_at, updated_at) VALUES ('legacy-job', 'Northstar Labs', 'Engineer', '', '', 'Manual', 'interview', 'applied', 'Preserve this manual note', 1, '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')")
        .execute(&pool).await.expect("legacy manual job");
    sqlx::query("INSERT INTO emails (id, gmail_message_id, gmail_thread_id, sender_name, sender_email, recipients, subject, snippet, body_text, received_at, content_hash, category, classification_confidence, classification_source, created_at, updated_at, reasoning_code, manual_override, action_completed, raw_payload, synced_at) VALUES ('legacy-email', 'fixture-legacy', 'fixture-thread', 'Fictional Recruiter', 'recruiter@fiction.example', '[]', 'Interview', 'A fictional message', 'A fictional message', '2026-09-01T00:00:00Z', 'legacy-hash', 'other', 1, 'manual', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z', 'manual_correction', 1, 1, '{}', '2026-09-01T00:00:00Z')")
        .execute(&pool).await.expect("legacy corrected email");
    pool.close().await;
    let repository = Repository::open(&path)
        .await
        .expect("upgrade using embedded migrations");
    let snapshot = repository.snapshot().await.expect("upgraded snapshot");
    assert_eq!(snapshot.applications.len(), 1);
    assert_eq!(snapshot.applications[0].notes, "Preserve this manual note");
    assert_eq!(snapshot.applications[0].current_stage, "interview");
    assert!(snapshot.applications[0].stage_manually_set);
    assert_eq!(snapshot.emails.len(), 1);
    assert!(snapshot.emails[0].manual_override && snapshot.emails[0].action_completed);
    assert!(snapshot.emails[0].gmail_account_id.is_none());
    assert!(!snapshot.emails[0].remote_deleted);
    assert!(repository
        .gmail_sync_state()
        .await
        .expect("sync state")
        .is_none());
}

#[tokio::test]
async fn fresh_database_migrates_with_defaults_foreign_keys_and_contact_constraints() {
    let (repository, _directory) = database().await;
    let snapshot = repository.snapshot().await.expect("read workspace");
    assert!(snapshot.applications.is_empty() && snapshot.emails.is_empty());
    assert_eq!(snapshot.settings.sync_email_limit, 500);
    assert_eq!(snapshot.settings.ai_mode, "off");
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&repository.pool)
        .await
        .expect("pragma");
    assert_eq!(foreign_keys, 1);
    let invalid = sqlx::query("INSERT INTO contacts (id, application_id, name, email, created_at) VALUES ('contact', 'missing', 'Recruiter', 'recruiter@fiction.example', '2026-09-01T00:00:00Z')")
        .execute(&repository.pool).await;
    assert!(
        invalid.is_err(),
        "contact cannot reference a missing application"
    );
    let snapshot = repository
        .save_application(application("Northstar Labs"))
        .await
        .expect("create");
    let id = &snapshot.applications[0].id;
    for (contact_id, email) in [
        ("a", "recruiter@fiction.example"),
        ("b", "RECRUITER@fiction.example"),
    ] {
        let result = sqlx::query("INSERT INTO contacts (id, application_id, name, email, created_at) VALUES (?, ?, 'Recruiter', ?, '2026-09-01T00:00:00Z')")
            .bind(contact_id).bind(id).bind(email).execute(&repository.pool).await;
        assert_eq!(
            result.is_ok(),
            contact_id == "a",
            "contact uniqueness ignores address case"
        );
    }
}

#[tokio::test]
async fn edits_archive_and_reopening_preserve_application_and_manual_timeline() {
    let (repository, directory) = database().await;
    let created = repository
        .save_application(application(" Northstar Labs "))
        .await
        .expect("create");
    let id = created.applications[0].id.clone();
    assert_eq!(created.applications[0].company, "Northstar Labs");
    assert!(!created.applications[0].stage_manually_set);
    let mut edit = application("Northstar Labs — corrected");
    edit.id = Some(id.clone());
    edit.current_stage = "preparing".into();
    edit.notes = "Edited notes remain local".into();
    edit.archived = true;
    repository.save_application(edit).await.expect("edit");
    let reopened = Repository::open(directory.path().join("workspace.sqlite3"))
        .await
        .expect("reopen existing database");
    let snapshot = reopened.snapshot().await.expect("read");
    let job = &snapshot.applications[0];
    assert_eq!(job.id, id);
    assert_eq!(job.company, "Northstar Labs — corrected");
    assert_eq!(job.current_stage, "preparing");
    assert!(job.archived && job.stage_manually_set);
    assert_eq!(job.notes, "Edited notes remain local");
    assert!(snapshot
        .events
        .iter()
        .any(|event| event.event_type == "stage_changed" && event.event_source == "manual"));
    assert!(snapshot
        .events
        .iter()
        .any(|event| event.notes.as_deref() == Some("Application archived.")));
}

#[tokio::test]
async fn fixture_import_and_reclassification_are_idempotent() {
    let (repository, _directory) = database().await;
    let first = repository.load_fixtures().await.expect("import");
    assert_eq!(first.emails.len(), 10);
    assert!(
        first.applications.is_empty(),
        "import never silently creates jobs"
    );
    assert!(first
        .emails
        .iter()
        .any(|email| email.requires_action && email.is_job_related));
    assert!(first.emails.iter().any(|email| !email.is_job_related));
    let second = repository.load_fixtures().await.expect("repeat import");
    assert_eq!(
        first
            .emails
            .iter()
            .map(|email| &email.id)
            .collect::<Vec<_>>(),
        second
            .emails
            .iter()
            .map(|email| &email.id)
            .collect::<Vec<_>>()
    );
    let raw_payload: String = sqlx::query_scalar(
        "SELECT raw_payload FROM emails WHERE gmail_message_id = 'fixture-harbor-assessment-v1'",
    )
    .fetch_one(&repository.pool)
    .await
    .expect("original HTML retained");
    assert!(raw_payload.contains("<strong>technical assessment</strong>"));
    let rebuilt = repository.rebuild_classifications().await.expect("rebuild");
    assert_eq!(rebuilt.emails.len(), 10);
}

#[tokio::test]
async fn rebuilding_old_cache_fixes_swedish_and_false_positives_without_losing_user_decisions() {
    let (repository, _directory) = database().await;
    let base = RawEmail {
        gmail_message_id: "sanitized-ats".into(),
        gmail_thread_id: "sanitized-thread".into(),
        sender_name: "Fictional Talent Acquisition".into(),
        sender_email: "recruitment@fiction.example".into(),
        recipients: vec!["alex@fiction.example".into()],
        subject: "Account information".into(),
        received_at: "2026-09-10T09:00:00Z".into(),
        snippet: String::new(),
        body_text: Some(
            "<p>Please click below to change your password.</p><p>Fictional Talent Acquisition</p>"
                .into(),
        ),
        body_html: None,
    };
    let mut swedish = base.clone();
    swedish.gmail_message_id = "swedish-confirmation".into();
    swedish.subject = "Tack för din ansökan".into();
    swedish.body_text = Some("Vi har mottagit din ansökan till tjänsten som utvecklare.".into());
    let mut manual = swedish.clone();
    manual.gmail_message_id = "manually-corrected".into();
    let snapshot = repository
        .import_emails(&[base, swedish, manual])
        .await
        .expect("cache sanitized examples");
    let id_for = |remote: &str| {
        snapshot
            .emails
            .iter()
            .find(|email| email.gmail_message_id == remote)
            .expect("email")
            .id
            .clone()
    };
    let ats_id = id_for("sanitized-ats");
    let sv_id = id_for("swedish-confirmation");
    let manual_id = id_for("manually-corrected");
    // Reproduce stale pre-fix classifications in an existing local cache.
    sqlx::query("UPDATE emails SET category = 'recruiter_contact', is_job_related = 1, event_type = 'recruiter_contact', suggested_stage = 'recruiter_screen', classification_confidence = 0.91, requires_action = 1, action_completed = 1, body_text = '<p>old rendering</p>', content_hash = 'old-hash' WHERE id = ?")
        .bind(&ats_id).execute(&repository.pool).await.expect("old false positive");
    sqlx::query(
        "UPDATE emails SET category = 'other', is_job_related = 0, event_type = NULL WHERE id = ?",
    )
    .bind(&sv_id)
    .execute(&repository.pool)
    .await
    .expect("old Swedish miss");
    let created = repository
        .save_application(application("Norrsken AB"))
        .await
        .expect("manual job");
    let job_id = created.applications[0].id.clone();
    repository
        .link_email(&ats_id, &job_id)
        .await
        .expect("existing manual link");
    let mut edited = application("Norrsken AB");
    edited.id = Some(job_id.clone());
    edited.current_stage = "interview".into();
    edited.notes = "Keep my notes".into();
    edited.next_action = Some("My manual next step".into());
    repository
        .save_application(edited)
        .await
        .expect("manual stage and action");
    repository
        .set_email_disposition(&manual_id, EmailDisposition::NotJob)
        .await
        .expect("manual correction");
    repository
        .set_email_disposition(&manual_id, EmailDisposition::Ignored)
        .await
        .expect("ignored correction");
    let rebuilt = repository
        .rebuild_classifications()
        .await
        .expect("reclassify existing cache");
    let account = rebuilt
        .emails
        .iter()
        .find(|email| email.id == ats_id)
        .unwrap();
    assert_eq!(account.category, "account_notification");
    assert!(!account.is_job_related && !account.requires_action && account.event_type.is_none());
    assert!(account.action_completed);
    assert!(!account.body_text.contains("<p>"));
    assert!(account.body_text.contains("change your password"));
    assert_ne!(account.content_hash, "old-hash");
    assert_eq!(
        rebuilt
            .emails
            .iter()
            .find(|email| email.id == sv_id)
            .unwrap()
            .category,
        "application_confirmation"
    );
    let correction = rebuilt
        .emails
        .iter()
        .find(|email| email.id == manual_id)
        .unwrap();
    assert!(correction.manual_override && correction.ignored && !correction.is_job_related);
    assert_eq!(rebuilt.links.len(), 1);
    assert_eq!(rebuilt.links[0].association_source, "manual");
    assert_eq!(rebuilt.applications[0].current_stage, "interview");
    assert_eq!(rebuilt.applications[0].notes, "Keep my notes");
    assert_eq!(
        rebuilt.applications[0].next_action.as_deref(),
        Some("My manual next step")
    );
    assert!(!rebuilt
        .events
        .iter()
        .any(|event| event.source_email_id.as_deref() == Some(&ats_id)));
}

#[tokio::test]
async fn linking_drives_stage_and_timeline_without_overwriting_user_action() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let mut input = application("Northstar Labs");
    input.next_action = Some("Review interview notes".into());
    let created = repository.save_application(input).await.expect("create");
    let id = created.applications[0].id.clone();
    let linked = repository.link_email(&email_id, &id).await.expect("link");
    assert_eq!(linked.applications[0].current_stage, "interview");
    assert_eq!(
        linked.applications[0].next_action.as_deref(),
        Some("Review interview notes")
    );
    assert_eq!(linked.links[0].association_source, "manual");
    let event_count = linked.events.len();
    let repeated = repository
        .link_email(&email_id, &id)
        .await
        .expect("link twice");
    assert_eq!(repeated.events.len(), event_count);
    let rebuilt = repository.rebuild_classifications().await.expect("rebuild");
    assert_eq!(rebuilt.events.len(), event_count);
    assert_eq!(
        rebuilt.applications[0].updated_at, linked.applications[0].updated_at,
        "unchanged rebuild must not invent new application activity"
    );
    assert!(rebuilt
        .events
        .iter()
        .any(
            |event| event.source_email_id.as_deref() == Some(email_id.as_str())
                && event.event_type == "interview_requested"
        ));
}

#[tokio::test]
async fn manually_edited_stage_wins_over_later_email_links_and_rebuild() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let created = repository
        .save_application(application("Northstar Labs"))
        .await
        .expect("create");
    let mut edit = application("Northstar Labs");
    edit.id = Some(created.applications[0].id.clone());
    edit.current_stage = "preparing".into();
    repository
        .save_application(edit)
        .await
        .expect("manual stage");
    repository
        .link_email(&email_id, &created.applications[0].id)
        .await
        .expect("link");
    let rebuilt = repository.rebuild_classifications().await.expect("rebuild");
    assert_eq!(rebuilt.applications[0].current_stage, "preparing");
    assert!(rebuilt.applications[0].stage_manually_set);
    assert!(
        rebuilt
            .events
            .iter()
            .any(|event| event.event_type == "interview_requested"),
        "inferred evidence remains visible even when stage is protected"
    );
}

#[tokio::test]
async fn late_import_of_old_confirmation_cannot_regress_interview() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let interview = fixture_id(&imported, "northstar-interview");
    let confirmation = fixture_id(&imported, "northstar-confirmation");
    let created = repository
        .save_application(application("Northstar Labs"))
        .await
        .expect("create");
    let id = &created.applications[0].id;
    repository
        .link_email(&interview, id)
        .await
        .expect("interview first");
    let snapshot = repository
        .link_email(&confirmation, id)
        .await
        .expect("older confirmation second");
    assert_eq!(snapshot.applications[0].current_stage, "interview");
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.source_email_id.is_some())
            .count(),
        2
    );
}

#[tokio::test]
async fn relinking_reprojects_both_jobs_and_keeps_superseded_audit() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let created = repository
        .save_application(application("First application"))
        .await
        .expect("create first");
    let first_id = created.applications[0].id.clone();
    let created = repository
        .save_application(application("Second application"))
        .await
        .expect("create second");
    let second_id = created
        .applications
        .iter()
        .find(|job| job.company == "Second application")
        .expect("second")
        .id
        .clone();
    repository
        .link_email(&email_id, &first_id)
        .await
        .expect("first link");
    let moved = repository
        .link_email(&email_id, &second_id)
        .await
        .expect("move link");
    assert_eq!(moved.links.len(), 1);
    assert_eq!(moved.links[0].application_id, second_id);
    assert_eq!(
        moved
            .applications
            .iter()
            .find(|job| job.id == first_id)
            .expect("first")
            .current_stage,
        "applied"
    );
    assert_eq!(
        moved
            .applications
            .iter()
            .find(|job| job.id == second_id)
            .expect("second")
            .current_stage,
        "interview"
    );
    assert!(moved
        .events
        .iter()
        .filter(|event| event.source_email_id.as_deref() == Some(email_id.as_str()))
        .all(|event| event.application_id == second_id));
    let superseded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM application_events WHERE source_email_id = ? AND superseded_at IS NOT NULL")
        .bind(&email_id).fetch_one(&repository.pool).await.expect("audit retained");
    assert_eq!(superseded, 1);
}

#[tokio::test]
async fn not_job_correction_survives_rebuild_and_restores_only_explicitly() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let created = repository
        .save_application(application("Northstar Labs"))
        .await
        .expect("create");
    repository
        .link_email(&email_id, &created.applications[0].id)
        .await
        .expect("link");
    repository
        .set_email_disposition(&email_id, EmailDisposition::NotJob)
        .await
        .expect("manual correction");
    let rebuilt = repository.rebuild_classifications().await.expect("rebuild");
    let email = rebuilt
        .emails
        .iter()
        .find(|email| email.id == email_id)
        .expect("email retained");
    assert!(!email.is_job_related && email.manual_override && !email.requires_action);
    assert_eq!(email.classification_source, "manual");
    assert!(rebuilt.links.is_empty());
    assert_eq!(rebuilt.applications[0].current_stage, "applied");
    let restored = repository
        .set_email_disposition(&email_id, EmailDisposition::Restore)
        .await
        .expect("restore");
    let email = restored
        .emails
        .iter()
        .find(|email| email.id == email_id)
        .expect("email");
    assert!(email.is_job_related && !email.manual_override);
    assert!(
        restored.links.is_empty(),
        "restoring does not silently restore a disputed association"
    );
}

#[tokio::test]
async fn completed_actions_and_ignored_suggestions_survive_import_and_rebuild() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "harbor-assessment");
    repository
        .resolve_action(None, Some(&email_id))
        .await
        .expect("complete");
    repository
        .set_email_disposition(&email_id, EmailDisposition::Ignored)
        .await
        .expect("ignore");
    repository.load_fixtures().await.expect("repeat import");
    let rebuilt = repository.rebuild_classifications().await.expect("rebuild");
    let email = rebuilt
        .emails
        .iter()
        .find(|email| email.id == email_id)
        .expect("email");
    assert!(email.action_completed && email.ignored);
}

#[tokio::test]
async fn resolving_email_does_not_clear_unrelated_manual_job_action() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let mut input = application("Northstar Labs");
    input.next_action = Some("Research their accessibility practices".into());
    let created = repository.save_application(input).await.expect("create");
    repository
        .link_email(&email_id, &created.applications[0].id)
        .await
        .expect("link");
    let resolved = repository
        .resolve_action(None, Some(&email_id))
        .await
        .expect("complete only email");
    assert_eq!(
        resolved.applications[0].next_action.as_deref(),
        Some("Research their accessibility practices")
    );
}

#[tokio::test]
async fn resolving_job_action_also_completes_matching_linked_email_action() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let mut input = application("Northstar Labs");
    input.next_action = imported
        .emails
        .iter()
        .find(|email| email.id == email_id)
        .expect("email")
        .suggested_action
        .clone();
    input.next_action_due_at = Some("2026-09-09".into());
    let created = repository.save_application(input).await.expect("create");
    let id = &created.applications[0].id;
    repository.link_email(&email_id, id).await.expect("link");
    let resolved = repository
        .resolve_action(Some(id), None)
        .await
        .expect("complete job");
    assert!(
        resolved.applications[0].next_action.is_none()
            && resolved.applications[0].next_action_due_at.is_none()
    );
    assert!(
        resolved
            .emails
            .iter()
            .find(|email| email.id == email_id)
            .expect("email")
            .action_completed
    );
}

#[tokio::test]
async fn failed_create_from_email_and_failed_relink_roll_back_every_record() {
    let (repository, _directory) = database().await;
    let mut input = application("Must not persist");
    input.source_email_id = Some("missing-email".into());
    assert!(repository.save_application(input).await.is_err());
    let empty = repository.snapshot().await.expect("read");
    assert!(empty.applications.is_empty() && empty.events.is_empty());
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "northstar-interview");
    let created = repository
        .save_application(application("Northstar Labs"))
        .await
        .expect("create");
    let id = &created.applications[0].id;
    repository
        .link_email(&email_id, id)
        .await
        .expect("valid link");
    assert!(repository
        .link_email(&email_id, "missing-job")
        .await
        .is_err());
    let unchanged = repository.snapshot().await.expect("read");
    assert_eq!(unchanged.links.len(), 1);
    assert_eq!(unchanged.links[0].application_id, *id);
    assert_eq!(unchanged.applications[0].current_stage, "interview");
}

#[tokio::test]
async fn create_from_email_is_atomic_and_projects_initial_stage() {
    let (repository, _directory) = database().await;
    let imported = repository.load_fixtures().await.expect("import");
    let email_id = fixture_id(&imported, "harbor-assessment");
    let mut input = application("Harbor Analytics");
    input.current_stage = "discovered".into();
    input.applied_at = None;
    input.source_email_id = Some(email_id.clone());
    let snapshot = repository
        .save_application(input)
        .await
        .expect("create from email");
    assert_eq!(snapshot.applications.len(), 1);
    assert_eq!(snapshot.applications[0].current_stage, "technical_test");
    assert!(!snapshot.applications[0].stage_manually_set);
    assert_eq!(snapshot.links[0].email_id, email_id);
}

#[tokio::test]
async fn settings_and_application_input_are_validated_at_the_backend_boundary() {
    let (repository, _directory) = database().await;
    let settings = AppSettings {
        ai_mode: "unsupported".into(),
        ..AppSettings::default()
    };
    assert!(repository.save_settings(settings).await.is_err());
    let settings = AppSettings {
        sync_email_limit: 700,
        ..AppSettings::default()
    };
    let saved = repository
        .save_settings(settings)
        .await
        .expect("valid settings");
    assert_eq!(saved.settings.sync_email_limit, 700);
    for (company, stage, date, url) in [
        ("", "applied", "2026-09-01", "https://example.test"),
        ("Example", "unknown", "2026-09-01", "https://example.test"),
        ("Example", "applied", "2026-02-30", "https://example.test"),
        ("Example", "applied", "2026-09-01", "javascript:alert(1)"),
    ] {
        let mut input = application(company);
        input.current_stage = stage.into();
        input.applied_at = Some(date.into());
        input.job_url = url.into();
        assert!(repository.save_application(input).await.is_err());
    }
    assert!(repository
        .snapshot()
        .await
        .expect("read")
        .applications
        .is_empty());
}
