import { apiFetch } from "./client";
import type { LoginResponse } from "./types";

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
  const refresh_token =
    typeof window === "undefined"
      ? null
      : sessionStorage.getItem("peanut.refreshToken") ??
        localStorage.getItem("peanut.refreshToken");
  if (!refresh_token) throw new Error("No refresh token");
  return apiFetch<LoginResponse>("/api/admin/auth/refresh", {
    method: "POST",
    auth: false,
    body: JSON.stringify({ refresh_token }),
  });
}

export async function logoutAdmin() {
  const refresh_token =
    typeof window === "undefined"
      ? null
      : sessionStorage.getItem("peanut.refreshToken") ??
        localStorage.getItem("peanut.refreshToken");
  if (!refresh_token) return;
  await apiFetch<{ message: string }>("/api/admin/auth/logout", {
    method: "POST",
    auth: false,
    body: JSON.stringify({ refresh_token }),
  });
}
