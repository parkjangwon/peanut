CREATE TABLE IF NOT EXISTS organizations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    created_by TEXT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    suspended_at DATETIME NULL,
    suspended_reason TEXT NULL,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE SET NULL
);

INSERT OR IGNORE INTO organizations (id, name, display_name)
VALUES ('default', 'default', 'Default Organization');

ALTER TABLE apps ADD COLUMN organization_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE apps ADD COLUMN suspended_at DATETIME NULL;
ALTER TABLE apps ADD COLUMN suspended_reason TEXT NULL;
ALTER TABLE audit_logs ADD COLUMN organization_id TEXT NULL;

UPDATE apps SET organization_id = 'default' WHERE organization_id IS NULL OR organization_id = '';

CREATE TABLE IF NOT EXISTS organization_members (
    organization_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (organization_id, user_id),
    FOREIGN KEY(organization_id) REFERENCES organizations(id) ON DELETE CASCADE,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO organization_members (organization_id, user_id, role)
SELECT 'default', id, 'owner'
FROM users
WHERE is_admin = TRUE AND admin_role = 'owner';

CREATE TABLE IF NOT EXISTS app_members (
    app_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (app_id, user_id),
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS beta_invites (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    code_hash TEXT NOT NULL UNIQUE,
    email TEXT NULL,
    domain TEXT NULL,
    max_uses INTEGER NOT NULL DEFAULT 1,
    used_count INTEGER NOT NULL DEFAULT 0,
    expires_at DATETIME NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at DATETIME NULL,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS organization_invites (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL,
    email TEXT NOT NULL,
    role TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at DATETIME NULL,
    accepted_at DATETIME NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at DATETIME NULL,
    FOREIGN KEY(organization_id) REFERENCES organizations(id) ON DELETE CASCADE,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    quotas_json TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT OR IGNORE INTO plans (id, display_name, quotas_json)
VALUES (
    'beta_free',
    'Beta Free',
    '{"apps":3,"app_users":10000,"data_rows":250000,"storage_bytes":2147483648,"function_invocations_month":50000,"push_sends_month":50000,"api_requests_month":1000000}'
);

CREATE TABLE IF NOT EXISTS organization_plan_assignments (
    organization_id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL,
    assigned_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(organization_id) REFERENCES organizations(id) ON DELETE CASCADE,
    FOREIGN KEY(plan_id) REFERENCES plans(id) ON DELETE RESTRICT
);

INSERT OR IGNORE INTO organization_plan_assignments (organization_id, plan_id)
VALUES ('default', 'beta_free');

CREATE TABLE IF NOT EXISTS usage_counters (
    organization_id TEXT NOT NULL,
    quota_key TEXT NOT NULL,
    period_start TEXT NOT NULL DEFAULT 'all',
    used INTEGER NOT NULL DEFAULT 0,
    quota_limit INTEGER NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (organization_id, quota_key, period_start),
    FOREIGN KEY(organization_id) REFERENCES organizations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS usage_events (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL,
    app_id TEXT NULL,
    quota_key TEXT NOT NULL,
    amount INTEGER NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(organization_id) REFERENCES organizations(id) ON DELETE CASCADE,
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_apps_org_deleted ON apps(organization_id, deleted_at);
CREATE INDEX IF NOT EXISTS idx_org_members_user ON organization_members(user_id);
CREATE INDEX IF NOT EXISTS idx_beta_invites_created_at ON beta_invites(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_org_created ON usage_events(organization_id, created_at DESC);
