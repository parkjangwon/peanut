"use client";

export type PeanutUser = {
  id: string;
  app_id: string;
  email: string;
  is_active: boolean;
  is_admin: boolean;
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
  name: string;
  display_name: string;
  created_by?: string | null;
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
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
  endpoint_slug: string;
  status: string;
  created_at: string;
  updated_at: string;
};

export type ActivityEvent = {
  id: string;
  app_id?: string | null;
  action: string;
  resource_type: string;
  resource_id: string;
  actor_user_id?: string | null;
  metadata?: unknown;
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
