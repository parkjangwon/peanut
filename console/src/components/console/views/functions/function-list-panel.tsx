"use client";

import { Code2 } from "lucide-react";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import type { FunctionDetail, FunctionSummary } from "@/lib/api";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";

import { EmptyState } from "../../shared/empty-state";
import { Panel } from "../../shared/layout-primitives";

export function FunctionListPanel({
  runtimeEnabled,
  functions,
  functionsLoading,
  activeName,
  detail,
  onSelectFunction,
  onLoadDraft,
  onApplyCachedDraft,
}: {
  runtimeEnabled: boolean;
  functions: FunctionSummary[] | undefined;
  functionsLoading: boolean;
  activeName: string;
  detail: FunctionDetail | undefined;
  onSelectFunction: (name: string) => void;
  onLoadDraft: (name: string) => Promise<void>;
  onApplyCachedDraft: () => void;
}) {
  const t = useTranslations("functionsView");

  return (
    <Panel title={t("functions")}>
      <div className="space-y-4">
        <Select
          value={activeName}
          onValueChange={(value) => {
            onSelectFunction(value);
            onLoadDraft(value).catch((error: Error) => toast.error(error.message));
          }}
        >
          <SelectTrigger><SelectValue placeholder={t("selectFunction")} /></SelectTrigger>
          <SelectContent>
            {(functions ?? []).map((fn) => (
              <SelectItem key={fn.name} value={fn.name}>{fn.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          className="w-full"
          onClick={onApplyCachedDraft}
          disabled={!runtimeEnabled || !detail}
        >
          <Code2 className="h-4 w-4" /> {t("loadDraft")}
        </Button>
        <p className="text-xs leading-5 text-muted-foreground">{t("leftRailHint")}</p>
        {!runtimeEnabled ? (
          <EmptyState
            icon={Code2}
            title={t("runtimeDisabledEmptyTitle")}
            description={t("runtimeDisabledEmptyDescription")}
            compact
          />
        ) : functionsLoading ? (
          <Skeleton className="h-44 w-full" />
        ) : (functions ?? []).length ? (
          <div className="space-y-2">
            {(functions ?? []).map((fn) => (
              <button
                key={fn.id}
                type="button"
                onClick={() => {
                  onSelectFunction(fn.name);
                  onLoadDraft(fn.name).catch((error: Error) => toast.error(error.message));
                }}
                className={cn(
                  "w-full rounded-lg border bg-background p-3 text-left transition-colors hover:bg-muted",
                  activeName === fn.name && "border-primary bg-primary/5",
                )}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate font-medium">{fn.display_name || fn.name}</span>
                  <Badge variant={fn.enabled ? "default" : "secondary"}>
                    v{fn.active_version_number}
                  </Badge>
                </div>
                <div className="mt-1 truncate font-mono text-xs text-muted-foreground">/{fn.endpoint_slug}</div>
              </button>
            ))}
          </div>
        ) : (
          <EmptyState
            icon={Code2}
            title={t("emptyFunctionsTitle")}
            description={t("emptyFunctionsDescription")}
            compact
          />
        )}
      </div>
    </Panel>
  );
}