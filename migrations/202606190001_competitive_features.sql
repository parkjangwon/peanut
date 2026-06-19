-- Push enhancements
ALTER TABLE push_queue ADD COLUMN payload_json TEXT;
ALTER TABLE push_queue ADD COLUMN scheduled_at TEXT;
ALTER TABLE push_queue ADD COLUMN idempotency_key TEXT;
ALTER TABLE push_queue ADD COLUMN broadcast_tag TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_push_queue_idempotency
    ON push_queue(app_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_push_queue_scheduled
    ON push_queue(status, scheduled_at);

-- App webhooks for push delivery notifications
ALTER TABLE apps ADD COLUMN push_webhook_url TEXT;
ALTER TABLE apps ADD COLUMN webhook_secret TEXT;

-- Email verification / change tokens
CREATE TABLE IF NOT EXISTS email_verification_tokens (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    email TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    purpose TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_email_verification_tokens_hash
    ON email_verification_tokens(token_hash, purpose);

-- Data row -> function triggers
CREATE TABLE IF NOT EXISTS data_function_triggers (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    table_id TEXT NOT NULL,
    event TEXT NOT NULL,
    function_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_data_function_triggers_lookup
    ON data_function_triggers(app_id, table_id, event);

-- Cron scheduled function invocations
CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    function_id TEXT NOT NULL,
    cron_expr TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_app
    ON scheduled_jobs(app_id, enabled);
