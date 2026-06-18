"use client";

import {
  Archive,
  Boxes,
  CheckCircle2,
  Code2,
  Database,
  KeyRound,
  ShieldCheck,
  Wrench,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useTranslations } from "next-intl";

import { ActivityEvent, apiFetch, AppSummary, OpsMetrics } from "@/lib/api";

import { ActivityList } from "../shared/activity-list";
import { JsonBlock } from "../shared/code-editor";
import {
  ActionCard,
  HealthRow,
  Metric,
  Panel,
  Section,
  StatusBadge,
} from "../shared/layout-primitives";
import type { DiagnosticCheck, View } from "../types";
import { browserSafeOrigin, displayProjectName, formatBytes } from "../utils/display";
import { pathForView } from "../nav-config";

export function OverviewView({
  apps,
  app,
  onViewChange,
}: {
  apps: AppSummary[];
  app: AppSummary;
  onViewChange: (view: View) => void;
}) {
  const t = useTranslations("overview");
  const common = useTranslations("common");
  const ready = useQuery({
    queryKey: ["ready"],
    queryFn: () => apiFetch<Record<string, unknown>>("/api/ready", { auth: false }),
  });
  const diagnostics = useQuery({
    queryKey: ["ops", "diagnostics"],
    queryFn: () => apiFetch<Record<string, unknown>>("/api/admin/ops/diagnostics"),
  });
  const metrics = useQuery({
    queryKey: ["ops", "metrics"],
    queryFn: () => apiFetch<OpsMetrics>("/api/admin/ops/metrics"),
  });
  const activity = useQuery({
    queryKey: ["activity", app.id],
    queryFn: async () => {
      const response = await apiFetch<{ events?: ActivityEvent[]; activity?: ActivityEvent[] }>(
        `/api/apps/${app.id}/activity`,
      );
      return response.events ?? response.activity ?? [];
    },
  });
  const platform = diagnostics.data as { checks?: DiagnosticCheck[] } | undefined;
  const platformChecks = platform?.checks ?? [];
  const failedChecks = platformChecks.filter((check) => !check.ok).length;
  const warningChecks = platformChecks.filter((check) => check.ok && check.severity === "warning").length;
  const functionsEnabled = metrics.data?.functions.enabled ?? true;
  const navigateTo = (nextView: View) => {
    onViewChange(nextView);
    window.history.replaceState(null, "", pathForView(nextView));
  };

  return (
    <Section
      title={t("title")}
      description={t("description")}
      action={<StatusBadge ok={Boolean(ready.data?.ready)} />}
    >
      <div className="grid gap-4 md:grid-cols-4">
        <Metric label={t("apps")} value={apps.length} icon={Boxes} />
        <Metric label={t("selectedApp")} value={displayProjectName(app)} icon={ShieldCheck} />
        <Metric label={t("ready")} value={ready.data?.status?.toString() ?? common("checking")} icon={CheckCircle2} />
        <Metric label={t("diagnostics")} value={failedChecks || diagnostics.isError ? common("needsAttention") : common("ok")} icon={Wrench} />
      </div>
      <div className="grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
        <Panel title={t("quickstart")}>
          <div className="grid gap-3 md:grid-cols-2">
            <ActionCard
              icon={KeyRound}
              title={t("quickstartKeys")}
              description={t("quickstartKeysDescription")}
              action={t("openKeys")}
              onClick={() => navigateTo("keys")}
            />
            <ActionCard
              icon={Database}
              title={t("quickstartData")}
              description={t("quickstartDataDescription")}
              action={t("openData")}
              onClick={() => navigateTo("data")}
            />
            <ActionCard
              icon={Archive}
              title={t("quickstartStorage")}
              description={t("quickstartStorageDescription")}
              action={t("openStorage")}
              onClick={() => navigateTo("storage")}
            />
            <ActionCard
              icon={Code2}
              title={t("quickstartFunctions")}
              description={functionsEnabled ? t("quickstartFunctionsDescription") : t("functionsDisabledDescription")}
              action={t("openFunctions")}
              onClick={() => navigateTo("functions")}
            />
          </div>
        </Panel>
        <Panel title={t("projectStatus")}>
          <div className="space-y-3">
            <HealthRow label={t("database")} value={formatBytes(metrics.data?.database.size_bytes ?? 0)} ok={!failedChecks} />
            <HealthRow label={t("storage")} value={metrics.data?.storage.root ?? common("checking")} ok={metrics.data?.storage.ok ?? true} />
            <HealthRow label={t("functionsRuntime")} value={functionsEnabled ? common("enabled") : common("disabled")} ok={functionsEnabled} />
            <HealthRow label={t("diagnosticWarnings")} value={String(warningChecks)} ok={warningChecks === 0} muted />
            <div className="rounded-md border bg-muted/30 p-3">
              <div className="text-xs font-medium text-muted-foreground">{t("projectId")}</div>
              <div className="mt-1 truncate font-mono text-xs">{app.id}</div>
            </div>
          </div>
        </Panel>
      </div>
      <div className="grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
        <Panel title={t("recentActivity")}>
          <ActivityList events={activity.data ?? []} />
        </Panel>
        <Panel title={t("operatorNotes")}>
          <div className="space-y-3 text-sm text-muted-foreground">
            <p>{t("operatorNotesDescription")}</p>
            <div className="rounded-md border bg-muted/30 p-3 font-mono text-xs text-foreground">
              {browserSafeOrigin()}/api/apps/{app.id}
            </div>
            <details className="rounded-md border bg-background p-3">
              <summary className="cursor-pointer text-sm font-medium text-foreground">{t("viewDiagnostics")}</summary>
              <div className="mt-3">
                <JsonBlock value={diagnostics.data ?? ready.data ?? { status: "loading" }} minHeight={220} />
              </div>
            </details>
          </div>
        </Panel>
      </div>
    </Section>
  );
}
