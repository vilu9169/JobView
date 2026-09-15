-- Email content is local cache; application records and corrections are local truth.
-- No credential, API-key or OAuth-token columns belong in this database.
CREATE TABLE emails (
    id TEXT PRIMARY KEY NOT NULL,
    gmail_message_id TEXT NOT NULL UNIQUE,
    gmail_thread_id TEXT NOT NULL,
    sender_name TEXT NOT NULL,
    sender_email TEXT NOT NULL,
    recipients TEXT NOT NULL CHECK (json_valid(recipients)),
    subject TEXT NOT NULL,
    snippet TEXT NOT NULL,
    body_text TEXT NOT NULL,
    received_at TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    category TEXT NOT NULL,
    classification_confidence REAL NOT NULL CHECK (classification_confidence BETWEEN 0 AND 1),
    classification_source TEXT NOT NULL CHECK (classification_source IN ('local_rules', 'gemini', 'manual')),
    classified_at TEXT,
    requires_action INTEGER NOT NULL DEFAULT 0 CHECK (requires_action IN (0, 1)),
    suggested_action TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    is_job_related INTEGER NOT NULL DEFAULT 0 CHECK (is_job_related IN (0, 1)),
    reasoning_code TEXT NOT NULL,
    extracted_company TEXT,
    extracted_role TEXT,
    suggested_stage TEXT,
    event_type TEXT,
    deadline TEXT,
    manual_override INTEGER NOT NULL DEFAULT 0 CHECK (manual_override IN (0, 1)),
    ignored INTEGER NOT NULL DEFAULT 0 CHECK (ignored IN (0, 1)),
    action_completed INTEGER NOT NULL DEFAULT 0 CHECK (action_completed IN (0, 1)),
    raw_payload TEXT NOT NULL CHECK (json_valid(raw_payload)),
    gmail_labels TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(gmail_labels)),
    gmail_history_id TEXT,
    synced_at TEXT NOT NULL
);

CREATE TABLE job_applications (
    id TEXT PRIMARY KEY NOT NULL,
    company TEXT NOT NULL CHECK (length(trim(company)) > 0),
    role TEXT NOT NULL CHECK (length(trim(role)) > 0),
    location TEXT NOT NULL DEFAULT '',
    job_url TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'Manual',
    applied_at TEXT,
    current_stage TEXT NOT NULL CHECK (current_stage IN ('discovered', 'preparing', 'applied', 'recruiter_screen', 'interview', 'technical_test', 'final_interview', 'offer', 'rejected', 'withdrawn')),
    initial_stage TEXT NOT NULL CHECK (initial_stage IN ('discovered', 'preparing', 'applied', 'recruiter_screen', 'interview', 'technical_test', 'final_interview', 'offer', 'rejected', 'withdrawn')),
    stage_manually_set INTEGER NOT NULL DEFAULT 0 CHECK (stage_manually_set IN (0, 1)),
    next_action TEXT,
    next_action_due_at TEXT,
    notes TEXT NOT NULL DEFAULT '',
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE application_emails (
    application_id TEXT NOT NULL REFERENCES job_applications(id) ON DELETE CASCADE,
    email_id TEXT NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    association_confidence REAL NOT NULL CHECK (association_confidence BETWEEN 0 AND 1),
    association_source TEXT NOT NULL CHECK (association_source IN ('automatic', 'gemini', 'manual', 'thread_match', 'domain_match')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (application_id, email_id),
    UNIQUE (email_id)
);

CREATE TABLE application_events (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT NOT NULL REFERENCES job_applications(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (event_type IN ('application_submitted', 'application_confirmed', 'recruiter_contact', 'interview_requested', 'interview_scheduled', 'assessment_requested', 'assessment_completed', 'final_interview', 'offer_received', 'rejected', 'withdrawn', 'follow_up', 'note', 'stage_changed')),
    event_date TEXT NOT NULL,
    source_email_id TEXT REFERENCES emails(id) ON DELETE SET NULL,
    confidence REAL CHECK (confidence IS NULL OR confidence BETWEEN 0 AND 1),
    event_source TEXT NOT NULL CHECK (event_source IN ('manual', 'manual_initial', 'local_rules', 'gemini')),
    notes TEXT,
    target_stage TEXT,
    created_at TEXT NOT NULL,
    -- Corrections retain original inferred events for audit without affecting projection.
    superseded_at TEXT
);

CREATE UNIQUE INDEX idx_events_active_source ON application_events (application_id, source_email_id)
    WHERE source_email_id IS NOT NULL AND superseded_at IS NULL AND event_source IN ('local_rules', 'gemini');
CREATE INDEX idx_emails_received ON emails (received_at DESC);
CREATE INDEX idx_emails_job_action ON emails (is_job_related, ignored, requires_action, action_completed);
CREATE INDEX idx_emails_thread ON emails (gmail_thread_id);
CREATE INDEX idx_emails_sender ON emails (sender_email);
CREATE INDEX idx_jobs_stage_archive ON job_applications (archived, current_stage);
CREATE INDEX idx_events_application_date ON application_events (application_id, event_date DESC);
CREATE INDEX idx_events_source ON application_events (source_email_id);

CREATE TABLE contacts (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT NOT NULL REFERENCES job_applications(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    email TEXT NOT NULL COLLATE NOCASE,
    title TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    UNIQUE (application_id, email)
);
CREATE INDEX idx_contacts_email ON contacts (email);

CREATE TABLE email_corrections (
    id TEXT PRIMARY KEY NOT NULL,
    email_id TEXT NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    correction_type TEXT NOT NULL,
    value TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_corrections_email ON email_corrections (email_id, created_at);

CREATE TABLE app_settings (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    sync_email_limit INTEGER NOT NULL DEFAULT 500 CHECK (sync_email_limit BETWEEN 1 AND 5000),
    local_confidence_accept_threshold REAL NOT NULL DEFAULT 0.90 CHECK (local_confidence_accept_threshold BETWEEN 0 AND 1),
    local_confidence_gemini_threshold REAL NOT NULL DEFAULT 0.50 CHECK (local_confidence_gemini_threshold BETWEEN 0 AND local_confidence_accept_threshold),
    ai_mode TEXT NOT NULL DEFAULT 'off' CHECK (ai_mode = 'off')
);
INSERT INTO app_settings (id) VALUES (1);

-- Future read-only Gmail synchronization checkpoint. Never stores authentication.
CREATE TABLE sync_metadata (
    account_id TEXT PRIMARY KEY NOT NULL,
    email_address TEXT NOT NULL,
    gmail_history_id TEXT,
    last_successful_sync_at TEXT,
    last_attempt_at TEXT,
    sync_status TEXT NOT NULL DEFAULT 'disconnected',
    error_code TEXT
);
