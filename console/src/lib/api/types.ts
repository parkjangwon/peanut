export type PeanutUser = {
  id: string;
  app_id: string;
  email: string;
  is_active: boolean;
  is_admin: boolean;
  admin_role: string;
};

export type LoginResponse = {
  access_token: string;
  refresh_token: string;
  token_type: string;
  expires_at: string;
  user: PeanutUser;
};

export type AppSummary = {
  id: string;
  workspace_id: string;
  name: string;
  display_name: string;
  created_by?: string | null;
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
  disabled_at?: string | null;
  disabled_reason?: string | null;
};

export type WorkspaceSummary = {
  id: string;
  name: string;
  display_name: string;
  created_by?: string | null;
  created_at: string;
  updated_at: string;
  disabled_at?: string | null;
  disabled_reason?: string | null;
};

export type ResourceLimitSummary = {
  resource_key: string;
  used: number;
  limit: number;
  period_start: string;
  reset_at?: string | null;
  source: "count" | "counter";
};

export type UsageSummary = {
  workspace_id: string;
  limit_profile_id: string;
  resource_limits: ResourceLimitSummary[];
};

export type AppKey = {
  id: string;
  app_id: string;
  key_type: string;
  scopes: string[];
  created_at: string;
  expires_at?: string | null;
  revoked_at?: string | null;
  last_used_at?: string | null;
};

export type DataTable = {
  id: string;
  app_id: string;
  name: string;
  schema_json?: string | null;
  created_at: string;
  updated_at: string;
};

export type DataRow = {
  id: string;
  owner_user_id?: string | null;
  data: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type DataRowsResponse = {
  rows: DataRow[];
  total: number;
  limit: number;
  offset: number;
  has_more: boolean;
};

export type StorageBucket = {
  id: string;
  app_id: string;
  name: string;
  public_read: boolean;
  allow_client_uploads?: boolean;
  max_object_bytes?: number | null;
  allowed_mime_types?: string[] | null;
  allowed_mime_types_json?: string | null;
  created_at: string;
  updated_at: string;
};

export type SdkStorageObjectSummary = {
  key: string;
  size: number;
  content_type?: string | null;
  etag: string;
  updated_at: string;
};

export type FunctionSummary = {
  id: string;
  app_id: string;
  name: string;
  display_name: string;
  endpoint_slug: string;
  runtime: string;
  invoke_policy: string;
  rate_limit_per_minute: number;
  api_key_present: boolean;
  timeout_ms: number;
  enabled: boolean;
  active_version_number: number;
  secret_key_count: number;
  updated_at: string;
};

export type FunctionDetail = FunctionSummary & {
  source_code: string;
  env_json: string;
  allowed_origins_json: string;
  active_version_id: string;
  created_by: string;
  updated_by: string;
  created_at: string;
};

export type FunctionVersionSummary = {
  id: string;
  app_id: string;
  function_id: string;
  version_number: number;
  runtime: string;
  invoke_policy: string;
  timeout_ms: number;
  created_by: string;
  created_at: string;
  secret_key_count: number;
  is_active: boolean;
};

export type FunctionInvocation = {
  id: string;
  app_id: string;
  function_id: string;
  status: string;
  request_json?: string | null;
  response_json?: string | null;
  error?: string | null;
  duration_ms?: number | null;
  invoke_mode: string;
  function_version_id?: string | null;
  retry_count: number;
  parent_invocation_id?: string | null;
  created_at: string;
  finished_at?: string | null;
};

export type BackupSummary = {
  name: string;
  size_bytes: number;
  modified_at: string;
};

export type RestorePendingSummary = {
  backup_name: string;
  exists: boolean;
  size_bytes?: number | null;
  modified_at?: string | null;
};

export type BackupsResponse = {
  backups: BackupSummary[];
  restore_pending?: RestorePendingSummary | null;
};

export type OpsMetrics = {
  database: {
    size_bytes: number;
    page_count: number;
    page_size: number;
    freelist_count: number;
    backup_count: number;
    last_backup_at?: string | null;
    restore_pending: boolean;
  };
  storage: {
    ok: boolean;
    error?: string | null;
    root: string;
    object_count: number;
    total_bytes: number;
    multipart_stale_count: number;
  };
  push: {
    queued: number;
    retry_scheduled: number;
    retry_overdue: number;
    failed_recent: number;
  };
  functions: {
    enabled: boolean;
    network_allowed: boolean;
    work_dir: string;
    running_limit: number;
    memory_limit_mb: number;
    source_limit_bytes: number;
    output_limit_bytes: number;
    invocations_24h: number;
    failures_24h: number;
    timeouts_24h: number;
  };
  system: {
    version: string;
    uptime_seconds: number;
  };
};

export type ActivityEvent = {
  id: string;
  app_id?: string | null;
  actor_role?: string;
  action: string;
  resource_type?: string;
  resource_id?: string;
  target_type?: string;
  target_id?: string;
  actor_user_id?: string | null;
  metadata?: unknown;
  metadata_json?: string;
  request_id?: string | null;
  created_at: string;
};

export type ApiError = Error & {
  status?: number;
  payload?: unknown;
};
