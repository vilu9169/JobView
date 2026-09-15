-- Gmail content is scoped to one explicitly connected account per workspace.
-- Existing fixture data, IDs, associations and corrections remain intact.
ALTER TABLE emails ADD COLUMN gmail_account_id TEXT;
ALTER TABLE emails ADD COLUMN remote_deleted INTEGER NOT NULL DEFAULT 0 CHECK (remote_deleted IN (0, 1));
ALTER TABLE emails ADD COLUMN gmail_raw_payload TEXT CHECK (gmail_raw_payload IS NULL OR json_valid(gmail_raw_payload));
CREATE INDEX idx_emails_gmail_account ON emails (gmail_account_id, gmail_message_id);

ALTER TABLE sync_metadata ADD COLUMN initial_sync_limit INTEGER NOT NULL DEFAULT 0;
-- Explicit one-account binding prevents Gmail ID collisions across mailboxes.
CREATE UNIQUE INDEX idx_sync_single_account ON sync_metadata ((1));
