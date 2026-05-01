ALTER TABLE organizations RENAME TO workspaces;
ALTER TABLE organization_members RENAME TO workspace_members;
ALTER TABLE beta_invites RENAME TO workspace_setup_invites;
ALTER TABLE organization_invites RENAME TO workspace_member_invites;
ALTER TABLE plans RENAME TO limit_profiles;
ALTER TABLE organization_plan_assignments RENAME TO workspace_limit_profiles;

ALTER TABLE workspaces RENAME COLUMN suspended_at TO disabled_at;
ALTER TABLE workspaces RENAME COLUMN suspended_reason TO disabled_reason;
ALTER TABLE apps RENAME COLUMN organization_id TO workspace_id;
ALTER TABLE apps RENAME COLUMN suspended_at TO disabled_at;
ALTER TABLE apps RENAME COLUMN suspended_reason TO disabled_reason;
ALTER TABLE audit_logs RENAME COLUMN organization_id TO workspace_id;

ALTER TABLE workspace_members RENAME COLUMN organization_id TO workspace_id;
ALTER TABLE workspace_member_invites RENAME COLUMN organization_id TO workspace_id;
ALTER TABLE workspace_limit_profiles RENAME COLUMN organization_id TO workspace_id;
ALTER TABLE workspace_limit_profiles RENAME COLUMN plan_id TO limit_profile_id;

ALTER TABLE limit_profiles RENAME COLUMN quotas_json TO resource_limits_json;
ALTER TABLE usage_counters RENAME COLUMN organization_id TO workspace_id;
ALTER TABLE usage_counters RENAME COLUMN quota_key TO resource_key;
ALTER TABLE usage_counters RENAME COLUMN quota_limit TO resource_limit;
ALTER TABLE usage_events RENAME COLUMN organization_id TO workspace_id;
ALTER TABLE usage_events RENAME COLUMN quota_key TO resource_key;

INSERT OR IGNORE INTO limit_profiles (id, display_name, resource_limits_json, created_at)
SELECT 'self_hosted_default', 'Self-hosted Default', resource_limits_json, created_at
FROM limit_profiles
WHERE id = 'beta_free';

UPDATE workspace_limit_profiles
SET limit_profile_id = 'self_hosted_default'
WHERE limit_profile_id = 'beta_free';

DELETE FROM limit_profiles WHERE id = 'beta_free';
