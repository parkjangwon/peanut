ALTER TABLE refresh_tokens ADD COLUMN session_id TEXT NOT NULL DEFAULT '';
ALTER TABLE refresh_tokens ADD COLUMN revoked_at DATETIME NULL;
ALTER TABLE refresh_tokens ADD COLUMN replaced_by_token TEXT NULL;

UPDATE refresh_tokens
SET session_id = token
WHERE session_id = '';

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_session_id ON refresh_tokens(session_id);

CREATE TABLE IF NOT EXISTS password_reset_tokens (
    token TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    consumed_at DATETIME NULL,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user_id ON password_reset_tokens(user_id);
