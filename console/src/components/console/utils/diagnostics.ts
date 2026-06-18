import type { useTranslations } from "next-intl";

import type { DiagnosticCheck, DiagnosticGroup } from "../types";

export function groupDiagnosticChecks(
  checks: DiagnosticCheck[],
  t: ReturnType<typeof useTranslations>,
): DiagnosticGroup[] {
  const buckets = new Map<string, DiagnosticCheck[]>();
  for (const check of checks) {
    const bucket = diagnosticBucket(check.name);
    buckets.set(bucket, [...(buckets.get(bucket) ?? []), check]);
  }
  return Array.from(buckets.entries()).map(([bucket, bucketChecks]) => {
    const failed = bucketChecks.filter((check) => !check.ok);
    const warnings = bucketChecks.filter((check) => check.ok && check.severity === "warning");
    return {
      id: bucket,
      label: t(`diagnostic.${bucket}.label`),
      description: t(`diagnostic.${bucket}.description`, {
        count: bucketChecks.length,
        detail: failed.length > 0 ? t("diagnostic.needsAttentionDetail") : t("diagnostic.okDetail"),
      }),
      ok: failed.length === 0,
      severity: failed.length > 0 ? failed[0]?.severity : warnings[0]?.severity,
    };
  });
}

export function diagnosticBucket(name: string) {
  if (name === "db_schema_version") return "schema";
  if (name === "default_app") return "defaultProject";
  if (name === "default_workspace" || name === "workspace_schema" || name === "orphan_workspace_members") return "workspace";
  if (name === "orphan_apps_without_workspace") return "projectLinks";
  if (name === "app_id_column") return "projectIsolation";
  if (name === "app_scoped_unique_index") return "uniqueConstraints";
  if (name === "duplicate_workspace_names" || name === "duplicate_app_names") return "duplicates";
  if (name === "password_reset_delivery") return "passwordReset";
  if (name === "cors_origin_policy") return "cors";
  return "other";
}
