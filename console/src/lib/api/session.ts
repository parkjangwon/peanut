import type { LoginResponse, PeanutUser } from "./types";

export function storeSession(session: LoginResponse, remember?: boolean) {
  const shouldRemember = remember ?? Boolean(localStorage.getItem("peanut.refreshToken"));
  const storage = shouldRemember ? localStorage : sessionStorage;
  const otherStorage = shouldRemember ? sessionStorage : localStorage;
  otherStorage.removeItem("peanut.accessToken");
  otherStorage.removeItem("peanut.refreshToken");
  otherStorage.removeItem("peanut.user");
  storage.setItem("peanut.accessToken", session.access_token);
  storage.setItem("peanut.refreshToken", session.refresh_token);
  storage.setItem("peanut.user", JSON.stringify(session.user));
}

export function clearSession() {
  sessionStorage.removeItem("peanut.accessToken");
  sessionStorage.removeItem("peanut.refreshToken");
  sessionStorage.removeItem("peanut.user");
  localStorage.removeItem("peanut.accessToken");
  localStorage.removeItem("peanut.refreshToken");
  localStorage.removeItem("peanut.user");
}

export function storedUser(): PeanutUser | null {
  if (typeof window === "undefined") return null;
  const raw = sessionStorage.getItem("peanut.user") ?? localStorage.getItem("peanut.user");
  if (!raw) return null;
  try {
    return JSON.parse(raw) as PeanutUser;
  } catch {
    return null;
  }
}

export function readAccessToken() {
  if (typeof window === "undefined") return null;
  return (
    sessionStorage.getItem("peanut.accessToken") ??
    localStorage.getItem("peanut.accessToken")
  );
}

export function readRefreshToken() {
  if (typeof window === "undefined") return null;
  return (
    sessionStorage.getItem("peanut.refreshToken") ??
    localStorage.getItem("peanut.refreshToken")
  );
}
