CREATE TABLE IF NOT EXISTS app_keys (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_type TEXT NOT NULL,
    scopes_json TEXT NOT NULL DEFAULT '[]',
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME NULL,
    expires_at DATETIME NULL,
    revoked_at DATETIME NULL,
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_app_keys_app_id ON app_keys(app_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_app_keys_key_type ON app_keys(key_type);
