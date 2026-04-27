CREATE TABLE IF NOT EXISTS service_tokens (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    access_mode TEXT NOT NULL DEFAULT 'admin',
    user_id TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME NULL,
    expires_at DATETIME NULL,
    revoked_at DATETIME NULL,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_service_tokens_user_id ON service_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_service_tokens_access_mode ON service_tokens(access_mode);
