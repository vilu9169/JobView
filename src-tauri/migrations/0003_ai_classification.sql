-- Opt-in classification stores only validated results and usage, never API keys.
-- Rebuild the singleton table to extend its original CHECK without changing
-- shipped migration checksums or resetting user settings.
CREATE TABLE app_settings_with_ai (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    sync_email_limit INTEGER NOT NULL DEFAULT 500 CHECK (sync_email_limit BETWEEN 1 AND 5000),
    local_confidence_accept_threshold REAL NOT NULL DEFAULT 0.90 CHECK (local_confidence_accept_threshold BETWEEN 0 AND 1),
    local_confidence_gemini_threshold REAL NOT NULL DEFAULT 0.50 CHECK (local_confidence_gemini_threshold BETWEEN 0 AND local_confidence_accept_threshold),
    ai_mode TEXT NOT NULL DEFAULT 'off' CHECK (ai_mode IN ('off', 'uncertain', 'candidates'))
);
INSERT INTO app_settings_with_ai SELECT * FROM app_settings;
DROP TABLE app_settings;
ALTER TABLE app_settings_with_ai RENAME TO app_settings;

CREATE TABLE ai_settings (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    model TEXT NOT NULL DEFAULT 'gemini-2.5-flash-lite',
    max_requests_per_run INTEGER NOT NULL DEFAULT 50 CHECK (max_requests_per_run BETWEEN 1 AND 500)
);
INSERT INTO ai_settings (id) VALUES (1);

CREATE TABLE ai_classification_cache (
    content_hash TEXT PRIMARY KEY NOT NULL,
    model TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    result_json TEXT NOT NULL CHECK (json_valid(result_json)),
    created_at TEXT NOT NULL
);

CREATE TABLE ai_usage (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    requests INTEGER NOT NULL DEFAULT 0 CHECK (requests >= 0),
    succeeded INTEGER NOT NULL DEFAULT 0 CHECK (succeeded >= 0),
    input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    cache_hits INTEGER NOT NULL DEFAULT 0 CHECK (cache_hits >= 0)
);
INSERT INTO ai_usage (id) VALUES (1);

-- Failed and interrupted requests also suppress automatic repeat charges for
-- unchanged content. The user can explicitly retry with forced reclassification.
CREATE TABLE ai_classification_attempts (
    content_hash TEXT PRIMARY KEY NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 1 CHECK (attempts >= 1),
    last_attempt_at TEXT NOT NULL
);
