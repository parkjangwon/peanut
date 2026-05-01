CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT 0,
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, email)
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    token TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_users_app_id ON users(app_id, id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_app_user ON refresh_tokens(app_id, user_id);
