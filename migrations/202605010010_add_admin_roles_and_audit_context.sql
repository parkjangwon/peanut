ALTER TABLE users ADD COLUMN admin_role TEXT NOT NULL DEFAULT 'viewer';

UPDATE users
SET admin_role = 'owner'
WHERE is_admin = TRUE;

ALTER TABLE audit_logs ADD COLUMN actor_role TEXT NOT NULL DEFAULT 'viewer';
ALTER TABLE audit_logs ADD COLUMN request_id TEXT NULL;

CREATE INDEX IF NOT EXISTS idx_audit_logs_app_action_created_at
    ON audit_logs(app_id, action, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_logs_app_resource_created_at
    ON audit_logs(app_id, target_type, target_id, created_at DESC);
