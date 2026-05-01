PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

ALTER TABLE users RENAME TO users_old_app_isolation;

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT 0,
    is_admin BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, email)
);

INSERT INTO users (id, app_id, email, password_hash, is_active, is_admin, created_at)
SELECT id, 'default', email, password_hash, is_active, is_admin, created_at
FROM users_old_app_isolation;

-- Keep the renamed legacy table so SQLite versions that rewrite existing
-- foreign keys during ALTER TABLE do not leave dependent tables pointing at a
-- missing table. New application queries use the rebuilt users table.
CREATE TRIGGER IF NOT EXISTS trg_users_mirror_insert_app_isolation
AFTER INSERT ON users
BEGIN
    INSERT OR IGNORE INTO users_old_app_isolation (
        id, email, password_hash, is_active, is_admin, created_at
    ) VALUES (
        NEW.id, NEW.email, NEW.password_hash, NEW.is_active, NEW.is_admin, NEW.created_at
    );
END;

CREATE TRIGGER IF NOT EXISTS trg_users_mirror_update_app_isolation
AFTER UPDATE ON users
BEGIN
    UPDATE users_old_app_isolation
    SET email = NEW.email,
        password_hash = NEW.password_hash,
        is_active = NEW.is_active,
        is_admin = NEW.is_admin,
        created_at = NEW.created_at
    WHERE id = NEW.id;
END;

ALTER TABLE refresh_tokens ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE password_reset_tokens ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE auth_events ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';

CREATE INDEX IF NOT EXISTS idx_users_app_email ON users(app_id, email);
CREATE INDEX IF NOT EXISTS idx_users_app_id ON users(app_id, id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_app_user ON refresh_tokens(app_id, user_id);
CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_app_user ON password_reset_tokens(app_id, user_id);
CREATE INDEX IF NOT EXISTS idx_auth_events_app_user_created_at ON auth_events(app_id, user_id, created_at DESC);

ALTER TABLE data_tables RENAME TO data_tables_old_app_isolation;

CREATE TABLE data_tables (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    schema_json TEXT NOT NULL,
    access_policy_json TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, name),
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE CASCADE
);

INSERT INTO data_tables (id, app_id, name, display_name, schema_json, access_policy_json, created_by, created_at)
SELECT id, COALESCE(app_id, 'default'), name, display_name, schema_json, access_policy_json, created_by, created_at
FROM data_tables_old_app_isolation;

-- Kept for the same foreign-key compatibility reason as users_old_app_isolation.
CREATE TRIGGER IF NOT EXISTS trg_data_tables_mirror_insert_app_isolation
AFTER INSERT ON data_tables
BEGIN
    INSERT OR IGNORE INTO data_tables_old_app_isolation (
        id, name, display_name, schema_json, access_policy_json, created_by, created_at, app_id
    ) VALUES (
        NEW.id, NEW.name, NEW.display_name, NEW.schema_json, NEW.access_policy_json, NEW.created_by, NEW.created_at, NEW.app_id
    );
END;

CREATE TRIGGER IF NOT EXISTS trg_data_tables_mirror_update_app_isolation
AFTER UPDATE ON data_tables
BEGIN
    UPDATE data_tables_old_app_isolation
    SET name = NEW.name,
        display_name = NEW.display_name,
        schema_json = NEW.schema_json,
        access_policy_json = NEW.access_policy_json,
        created_by = NEW.created_by,
        created_at = NEW.created_at,
        app_id = NEW.app_id
    WHERE id = NEW.id;
END;

ALTER TABLE functions RENAME TO functions_old_app_isolation;

CREATE TABLE functions (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    endpoint_slug TEXT NOT NULL,
    runtime TEXT NOT NULL DEFAULT 'deno',
    source_code TEXT NOT NULL,
    invoke_policy TEXT NOT NULL DEFAULT 'authenticated',
    env_json TEXT NOT NULL DEFAULT '{}',
    api_key_hash TEXT NULL,
    allowed_origins_json TEXT NOT NULL DEFAULT '[]',
    rate_limit_per_minute INTEGER NOT NULL DEFAULT 60,
    timeout_ms INTEGER NOT NULL DEFAULT 3000,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    active_version_number INTEGER NOT NULL DEFAULT 1,
    active_version_id TEXT NULL,
    secret_key_count INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, name),
    UNIQUE(app_id, endpoint_slug),
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY(updated_by) REFERENCES users(id) ON DELETE CASCADE
);

INSERT INTO functions (
    id, app_id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json,
    api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, enabled,
    active_version_number, active_version_id, secret_key_count, created_by, updated_by, created_at, updated_at
)
SELECT
    id, COALESCE(app_id, 'default'), name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json,
    api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, enabled,
    active_version_number, active_version_id, secret_key_count, created_by, updated_by, created_at, updated_at
FROM functions_old_app_isolation;

-- Kept for the same foreign-key compatibility reason as users_old_app_isolation.
CREATE TRIGGER IF NOT EXISTS trg_functions_mirror_insert_app_isolation
AFTER INSERT ON functions
BEGIN
    INSERT OR IGNORE INTO functions_old_app_isolation (
        id, name, display_name, endpoint_slug, runtime, source_code, invoke_policy, env_json,
        api_key_hash, allowed_origins_json, rate_limit_per_minute, timeout_ms, enabled,
        active_version_number, active_version_id, secret_key_count, created_by, updated_by,
        created_at, updated_at, app_id
    ) VALUES (
        NEW.id, NEW.name, NEW.display_name, NEW.endpoint_slug, NEW.runtime, NEW.source_code,
        NEW.invoke_policy, NEW.env_json, NEW.api_key_hash, NEW.allowed_origins_json,
        NEW.rate_limit_per_minute, NEW.timeout_ms, NEW.enabled, NEW.active_version_number,
        NEW.active_version_id, NEW.secret_key_count, NEW.created_by, NEW.updated_by,
        NEW.created_at, NEW.updated_at, NEW.app_id
    );
END;

CREATE TRIGGER IF NOT EXISTS trg_functions_mirror_update_app_isolation
AFTER UPDATE ON functions
BEGIN
    UPDATE functions_old_app_isolation
    SET name = NEW.name,
        display_name = NEW.display_name,
        endpoint_slug = NEW.endpoint_slug,
        runtime = NEW.runtime,
        source_code = NEW.source_code,
        invoke_policy = NEW.invoke_policy,
        env_json = NEW.env_json,
        api_key_hash = NEW.api_key_hash,
        allowed_origins_json = NEW.allowed_origins_json,
        rate_limit_per_minute = NEW.rate_limit_per_minute,
        timeout_ms = NEW.timeout_ms,
        enabled = NEW.enabled,
        active_version_number = NEW.active_version_number,
        active_version_id = NEW.active_version_id,
        secret_key_count = NEW.secret_key_count,
        created_by = NEW.created_by,
        updated_by = NEW.updated_by,
        created_at = NEW.created_at,
        updated_at = NEW.updated_at,
        app_id = NEW.app_id
    WHERE id = NEW.id;
END;

ALTER TABLE push_subscriptions RENAME TO push_subscriptions_old_app_isolation;

CREATE TABLE push_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id TEXT NOT NULL DEFAULT 'default',
    user_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, user_id, endpoint),
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

INSERT INTO push_subscriptions (id, app_id, user_id, endpoint, p256dh, auth, created_at)
SELECT id, COALESCE(app_id, 'default'), user_id, endpoint, p256dh, auth, created_at
FROM push_subscriptions_old_app_isolation;

-- Kept for the same foreign-key compatibility reason as users_old_app_isolation.

CREATE INDEX IF NOT EXISTS idx_data_tables_app_id ON data_tables(app_id, name);
CREATE INDEX IF NOT EXISTS idx_functions_app_id ON functions(app_id, name);
CREATE INDEX IF NOT EXISTS idx_functions_app_endpoint ON functions(app_id, endpoint_slug);
CREATE INDEX IF NOT EXISTS idx_push_subscriptions_app_user ON push_subscriptions(app_id, user_id, created_at DESC);

PRAGMA foreign_keys = ON;
PRAGMA legacy_alter_table = OFF;
