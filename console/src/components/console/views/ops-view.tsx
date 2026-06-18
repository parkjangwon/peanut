"use client";

import {
  CheckCircle2,
  CircleAlert,
  Database,
  Download,
  Plus,
  RotateCcw,
  Wrench,
} from "lucide-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import {
  apiFetch,
  BackupsResponse,
  downloadBackup,
  OpsMetrics,
  UsageSummary,
  WorkspaceSummary,
} from "@/lib/api";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";

import { JsonBlock } from "../shared/code-editor";
import { DataTableView } from "../shared/data-table-view";
import {
  Metric,
  Panel,
  Section,
  StatusBadge,
} from "../shared/layout-primitives";
import type { DiagnosticCheck } from "../types";
import { groupDiagnosticChecks } from "../utils/diagnostics";
import { formatBytes, usagePercent } from "../utils/display";

export function OpsView() {
  const t = useTranslations("opsView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
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
  const backups = useQuery({
    queryKey: ["ops", "backups"],
    queryFn: () => apiFetch<BackupsResponse>("/api/admin/backups"),
  });
  const workspaceUsage = useQuery({
    queryKey: ["ops", "workspace-usage"],
    queryFn: async () => {
      const response = await apiFetch<{ workspaces: WorkspaceSummary[] }>("/api/workspaces");
      return Promise.all(
        response.workspaces.map(async (workspace) => ({
          workspace,
          usage: await apiFetch<UsageSummary>(`/api/workspaces/${workspace.id}/resource-usage`),
        })),
      );
    },
  });
  const createBackup = useMutation({
    mutationFn: () => apiFetch("/api/admin/backups", { method: "POST", body: JSON.stringify({}) }),
    onSuccess: () => {
      toast.success(t("backupCreated"));
      queryClient.invalidateQueries({ queryKey: ["ops", "backups"] });
      queryClient.invalidateQueries({ queryKey: ["ops", "metrics"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const scheduleRestore = useMutation({
    mutationFn: (name: string) =>
      apiFetch(`/api/admin/backups/${name}/restore`, {
        method: "POST",
        body: JSON.stringify({
          confirmation: name,
          reason: "scheduled from Peanut Console",
        }),
      }),
    onSuccess: () => {
      toast.success(t("restoreScheduled"));
      queryClient.invalidateQueries({ queryKey: ["ops", "backups"] });
      queryClient.invalidateQueries({ queryKey: ["ops", "metrics"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const clearRestore = useMutation({
    mutationFn: () => apiFetch("/api/admin/backups/restore-pending", { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("restoreCleared"));
      queryClient.invalidateQueries({ queryKey: ["ops", "backups"] });
      queryClient.invalidateQueries({ queryKey: ["ops", "metrics"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const download = useMutation({
    mutationFn: downloadBackup,
    onSuccess: (blob, name) => {
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = name;
      link.click();
      URL.revokeObjectURL(url);
      toast.success(t("downloaded", { name }));
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const platform = diagnostics.data as { checks?: DiagnosticCheck[] } | undefined;
  const platformChecks = platform?.checks ?? [];
  const groupedChecks = groupDiagnosticChecks(platformChecks, t);
  const failedChecks = platformChecks.filter((check) => !check.ok).length;
  const warningChecks = platformChecks.filter((check) => check.ok && check.severity === "warning").length;
  const usageRows =
    workspaceUsage.data?.flatMap(({ workspace, usage }) =>
      usage.resource_limits.map((limit) => [
        workspace.display_name,
        limit.resource_key,
        `${limit.used.toLocaleString()} / ${limit.limit.toLocaleString()}`,
        `${usagePercent(limit.used, limit.limit)}%`,
        limit.reset_at ?? limit.period_start,
      ]),
    ) ?? [];
  return (
    <Section title={t("title")} description={t("description")}>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Metric label={t("ready")} value={String(ready.data?.status ?? common("checking"))} icon={CheckCircle2} />
        <Metric label={t("warnings")} value={warningChecks} icon={CircleAlert} />
        <Metric label={t("failures")} value={failedChecks} icon={Wrench} />
        <Metric label={t("dbSize")} value={formatBytes(metrics.data?.database.size_bytes ?? 0)} icon={Database} />
      </div>
      <div className="grid gap-4 xl:grid-cols-[1fr_1.2fr]">
        <Panel title={t("platformChecks")}>
          <div className="space-y-2">
            {groupedChecks.map((check) => (
              <div key={check.id} className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
                <div className="min-w-0">
                  <div className="flex items-center gap-2 font-medium">
                    {check.label}
                    {check.severity === "warning" && <Badge variant="secondary">{common("warning")}</Badge>}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">{check.description}</div>
                </div>
                <Badge variant={check.ok ? "default" : "destructive"}>{check.ok ? common("ok") : common("fail")}</Badge>
              </div>
            ))}
            {!platform?.checks?.length && <Skeleton className="h-40 w-full" />}
          </div>
        </Panel>
        <Panel title={t("runtime")}>
          <DataTableView
            loading={metrics.isLoading}
            columns={[t("columnsArea"), t("columnsValue"), t("columnsState")]}
            rows={[
              [t("storage"), metrics.data?.storage.root ?? "", metrics.data?.storage.ok ? t("writable") : metrics.data?.storage.error ?? ""],
              [t("backups"), metrics.data?.database.backup_count ?? 0, metrics.data?.database.restore_pending ? t("restorePendingState") : t("clearState")],
              [t("functions"), metrics.data?.functions.work_dir ?? "", metrics.data?.functions.enabled ? common("enabled") : common("disabled")],
              [t("pushQueue"), metrics.data?.push.queued ?? 0, t("failed24h", { count: metrics.data?.push.failed_recent ?? 0 })],
              [t("version"), metrics.data?.system.version ?? "", t("uptime", { seconds: metrics.data?.system.uptime_seconds ?? 0 })],
            ]}
          />
        </Panel>
      </div>
      <Panel title={t("workspaceUsage")}>
        <DataTableView
          loading={workspaceUsage.isLoading}
          columns={[t("columnsWorkspace"), t("columnsResource"), t("columnsUsage"), t("columnsPercent"), t("columnsPeriod")]}
          rows={usageRows}
          emptyTitle={t("emptyUsageTitle")}
          emptyDescription={t("emptyUsageDescription")}
        />
      </Panel>
      <Panel title={t("backups")}>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          {backups.data?.restore_pending ? (
            <Alert className="max-w-2xl">
              <CircleAlert className="h-4 w-4" />
              <AlertTitle>{t("restorePending")}</AlertTitle>
              <AlertDescription>{backups.data.restore_pending.backup_name}</AlertDescription>
            </Alert>
          ) : (
            <StatusBadge ok />
          )}
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() => {
                if (window.confirm(t("confirmClearRestore"))) {
                  clearRestore.mutate();
                }
              }}
              disabled={!backups.data?.restore_pending || clearRestore.isPending}
            >
              <RotateCcw className="h-4 w-4" /> {common("clear")}
            </Button>
            <Button onClick={() => createBackup.mutate()} disabled={createBackup.isPending}>
              <Plus className="h-4 w-4" /> {common("create")}
            </Button>
          </div>
        </div>
        <DataTableView
          loading={backups.isLoading}
          columns={[t("columnsName"), t("columnsSize"), t("columnsModified"), t("columnsActions")]}
          rows={(backups.data?.backups ?? []).map((backup) => [
            backup.name,
            formatBytes(backup.size_bytes),
            backup.modified_at,
            <div key={backup.name} className="flex gap-2">
              <Button variant="outline" size="sm" onClick={() => download.mutate(backup.name)}>
                <Download className="h-4 w-4" /> {common("download")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  if (window.confirm(t("confirmScheduleRestore", { name: backup.name }))) {
                    scheduleRestore.mutate(backup.name);
                  }
                }}
              >
                <RotateCcw className="h-4 w-4" /> {common("restore")}
              </Button>
            </div>,
          ])}
          emptyTitle={t("emptyBackupsTitle")}
          emptyDescription={t("emptyBackupsDescription")}
        />
      </Panel>
      <details className="rounded-lg border bg-card p-4">
        <summary className="cursor-pointer text-sm font-medium text-muted-foreground">{t("rawPayloads")}</summary>
        <div className="mt-4 grid gap-4 xl:grid-cols-2">
          <Panel title={t("readyPayload")}><JsonBlock value={ready.data ?? { status: common("loading") }} /></Panel>
          <Panel title={t("diagnosticsPayload")}><JsonBlock value={diagnostics.data ?? { status: common("loading") }} /></Panel>
        </div>
      </details>
    </Section>
  );
}
