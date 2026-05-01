"use client";

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

export type StorageBucket = {
  id: string;
  app_id: string;
  name: string;
  public_read: boolean;
  max_object_bytes?: number | null;
  allowed_mime_types?: string[] | null;
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

const API_BASE =
  process.env.NEXT_PUBLIC_PEANUT_API_URL?.replace(/\/$/, "") ?? "";

function readAccessToken() {
  if (typeof window === "undefined") return null;
  return sessionStorage.getItem("peanut.accessToken");
}

function readRefreshToken() {
  if (typeof window === "undefined") return null;
  return sessionStorage.getItem("peanut.refreshToken");
}

export function storeSession(session: LoginResponse) {
  sessionStorage.setItem("peanut.accessToken", session.access_token);
  sessionStorage.setItem("peanut.refreshToken", session.refresh_token);
  sessionStorage.setItem("peanut.user", JSON.stringify(session.user));
}

export function clearSession() {
  sessionStorage.removeItem("peanut.accessToken");
  sessionStorage.removeItem("peanut.refreshToken");
  sessionStorage.removeItem("peanut.user");
}

export function storedUser(): PeanutUser | null {
  if (typeof window === "undefined") return null;
  const raw = sessionStorage.getItem("peanut.user");
  if (!raw) return null;
  try {
    return JSON.parse(raw) as PeanutUser;
  } catch {
    return null;
  }
}

export async function apiFetch<T>(
  path: string,
  init: RequestInit & { auth?: boolean } = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type") && init.body) {
    headers.set("Content-Type", "application/json");
  }
  if (init.auth !== false) {
    const token = readAccessToken();
    if (token) headers.set("Authorization", `Bearer ${token}`);
  }

  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers,
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;

  if (!response.ok) {
    const error = new Error(
      payload?.error ?? payload?.message ?? `Request failed with ${response.status}`,
    ) as ApiError;
    error.status = response.status;
    error.payload = payload;
    throw error;
  }

  return payload as T;
}

export async function bootstrapAdmin(email: string, password: string) {
  return apiFetch<LoginResponse>("/api/bootstrap/admin", {
    method: "POST",
    auth: false,
    body: JSON.stringify({ email, password }),
  });
}

export async function loginAdmin(email: string, password: string) {
  return apiFetch<LoginResponse>("/api/admin/auth/login", {
    method: "POST",
    auth: false,
    body: JSON.stringify({ email, password }),
  });
}

export async function refreshAdminSession() {
  const refresh_token = readRefreshToken();
  if (!refresh_token) throw new Error("No refresh token");
  return apiFetch<LoginResponse>("/api/admin/auth/refresh", {
    method: "POST",
    auth: false,
    body: JSON.stringify({ refresh_token }),
  });
}

export async function logoutAdmin() {
  const refresh_token = readRefreshToken();
  if (!refresh_token) return;
  await apiFetch<{ message: string }>("/api/admin/auth/logout", {
    method: "POST",
    auth: false,
    body: JSON.stringify({ refresh_token }),
  });
}

export async function downloadBackup(name: string) {
  const token = readAccessToken();
  const response = await fetch(
    `${API_BASE}/api/admin/backups/${encodeURIComponent(name)}/download`,
    {
      headers: token ? { Authorization: `Bearer ${token}` } : undefined,
    },
  );
  if (!response.ok) {
    throw new Error(`Backup download failed with ${response.status}`);
  }
  return response.blob();
}
