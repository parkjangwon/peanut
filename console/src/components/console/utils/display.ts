import type { AppSummary, StorageBucket } from "@/lib/api";

export function displayProjectName(app: AppSummary) {
  const displayName = app.display_name || app.name;
  return displayName.replace(/\bApp\b/g, "Project").replace(/\bApps\b/g, "Projects");
}
export function browserSafeOrigin() {
  if (typeof window === "undefined") return "";
  return window.location.origin;
}
export function storageMimeTypes(bucket: StorageBucket) {
  if (Array.isArray(bucket.allowed_mime_types)) return bucket.allowed_mime_types;
  if (!bucket.allowed_mime_types_json) return [];
  try {
    const parsed = JSON.parse(bucket.allowed_mime_types_json);
    return Array.isArray(parsed) ? parsed.map(String) : [];
  } catch {
    return [];
  }
}

export function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[unit]}`;
}

export function usagePercent(used: number, limit: number) {
  if (limit <= 0) return used > 0 ? 100 : 0;
  return Math.min(100, Math.round((used / limit) * 100));
}
