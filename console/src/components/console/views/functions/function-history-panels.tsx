"use client";

import { History, RotateCcw } from "lucide-react";
import { useTranslations } from "next-intl";

import type { FunctionInvocation, FunctionVersionSummary } from "@/lib/api";
import { Button } from "@/components/ui/button";

import { DataTableView } from "../../shared/data-table-view";
import { Panel } from "../../shared/layout-primitives";

export function FunctionHistoryPanels({
  runtimeEnabled,
  versions,
  versionsLoading,
  invocations,
  invocationsLoading,
  onRollback,
  onRetry,
  rollbackPending,
  retryPending,
}: {
  runtimeEnabled: boolean;
  versions: FunctionVersionSummary[] | undefined;
  versionsLoading: boolean;
  invocations: FunctionInvocation[] | undefined;
  invocationsLoading: boolean;
  onRollback: (versionNumber: number) => void;
  onRetry: (invocationId: string) => void;
  rollbackPending: boolean;
  retryPending: boolean;
}) {
  const t = useTranslations("functionsView");
  const common = useTranslations("common");

  return (
    <>
      <Panel title={t("versions")}>
        <DataTableView
          loading={versionsLoading}
          columns={[t("columnsVersion"), t("columnsRuntime"), t("columnsActive"), t("columnsAction")]}
          rows={(versions ?? []).map((version) => [
            version.version_number,
            version.runtime,
            version.is_active ? common("yes") : common("no"),
            version.is_active ? "" : (
              <Button
                key={version.id}
                variant="outline"
                size="sm"
                onClick={() => onRollback(version.version_number)}
                disabled={!runtimeEnabled || rollbackPending}
              >
                <RotateCcw className="h-4 w-4" /> {t("restore")}
              </Button>
            ),
          ])}
          emptyTitle={runtimeEnabled ? t("emptyVersionsTitle") : t("runtimeDisabledEmptyTitle")}
          emptyDescription={runtimeEnabled ? t("emptyVersionsDescription") : t("runtimeDisabledEmptyDescription")}
        />
      </Panel>

      <Panel title={t("invocations")}>
        <DataTableView
          loading={invocationsLoading}
          columns={[t("columnsId"), t("columnsStatus"), t("columnsMode"), t("columnsDuration"), t("columnsRetry")]}
          rows={(invocations ?? []).map((invocation) => [
            invocation.id,
            invocation.status,
            invocation.invoke_mode,
            invocation.duration_ms ?? "",
            <Button
              key={invocation.id}
              variant="outline"
              size="sm"
              onClick={() => onRetry(invocation.id)}
              disabled={!runtimeEnabled || retryPending}
            >
              <History className="h-4 w-4" /> {t("retry")}
            </Button>,
          ])}
          emptyTitle={runtimeEnabled ? t("emptyInvocationsTitle") : t("runtimeDisabledEmptyTitle")}
          emptyDescription={runtimeEnabled ? t("emptyInvocationsDescription") : t("runtimeDisabledEmptyDescription")}
        />
      </Panel>
    </>
  );
}