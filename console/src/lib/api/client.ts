import type { ApiError, LoginResponse } from "./types";
import { readAccessToken, readRefreshToken, storeSession } from "./session";

const API_BASE =
  process.env.NEXT_PUBLIC_PEANUT_API_URL?.replace(/\/$/, "") ?? "";

function parsePayload(text: string) {
  if (!text) return null;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return { message: text };
  }
}

async function parseResponse<T>(response: Response): Promise<T> {
  const text = await response.text();
  return parsePayload(text) as T;
}

async function apiErrorFromResponse(response: Response): Promise<ApiError> {
  const payload = parsePayload(await response.text());
  const errorPayload =
    payload && typeof payload === "object"
      ? (payload as { error?: string; message?: string })
      : {};
  const error = new Error(
    errorPayload.error ?? errorPayload.message ?? `Request failed with ${response.status}`,
  ) as ApiError;
  error.status = response.status;
  error.payload = payload;
  return error;
}

async function fetchWithAuth(path: string, init: RequestInit & { auth?: boolean }) {
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type") && init.body) {
    headers.set("Content-Type", "application/json");
  }
  if (init.auth !== false) {
    const token = readAccessToken();
    if (token) headers.set("Authorization", `Bearer ${token}`);
  }

  return fetch(`${API_BASE}${path}`, {
    ...init,
    headers,
  });
}

async function refreshStoredSession() {
  const refresh_token = readRefreshToken();
  if (!refresh_token) return false;
  try {
    const session = await apiFetch<LoginResponse>("/api/admin/auth/refresh", {
      method: "POST",
      auth: false,
      body: JSON.stringify({ refresh_token }),
    });
    storeSession(session);
    return true;
  } catch {
    return false;
  }
}

export async function apiFetch<T>(
  path: string,
  init: RequestInit & { auth?: boolean } = {},
): Promise<T> {
  const response = await fetchWithAuth(path, init);
  if (response.ok) {
    return parseResponse<T>(response);
  }

  if (init.auth !== false && response.status === 401) {
    const refreshed = await refreshStoredSession();
    if (refreshed) {
      const retryResponse = await fetchWithAuth(path, init);
      if (retryResponse.ok) {
        return parseResponse<T>(retryResponse);
      }
      throw await apiErrorFromResponse(retryResponse);
    }
  }

  throw await apiErrorFromResponse(response);
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
