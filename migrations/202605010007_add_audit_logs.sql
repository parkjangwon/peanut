CREATE TABLE IF NOT EXISTS audit_logs (
    id TEXT PRIMARY KEY,
    app_id TEXT NULL,
    actor_user_id TEXT NOT NULL,
    actor_kind TEXT NOT NULL DEFAULT 'user',
    action TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE SET NULL,
    FOREIGN KEY(actor_user_id) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_app_created_at
    ON audit_logs(app_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_created_at
    ON audit_logs(actor_user_id, created_at DESC);
