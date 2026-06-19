"use client";

import { Bell, CheckCircle2, CircleAlert, History, TrendingUp } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { apiFetch, AppSummary, PeanutUser } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

import { JsonBlock } from "../shared/code-editor";
import { DataTableView } from "../shared/data-table-view";
import { EmptyState } from "../shared/empty-state";
import { Metric, Panel, Section } from "../shared/layout-primitives";
import type { PushQueueResponse, PushQueueStatsResponse } from "../types";

export function PushView({ app }: { app: AppSummary }) {
  const t = useTranslations("pushView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [title, setTitle] = useState("Console test");
  const [body, setBody] = useState("Push from Peanut Console");
  const [userId, setUserId] = useState("");
  const users = useQuery({
    queryKey: ["auth", "users", app.id],
    queryFn: async () =>
      (await apiFetch<{ users: PeanutUser[] }>(`/api/apps/${app.id}/auth/users`)).users,
  });
  const subscriptions = useQuery({
    queryKey: ["push", "subscriptions", app.id],
    queryFn: async () =>
      (await apiFetch<{ subscriptions: Array<Record<string, unknown>> }>(
        `/api/apps/${app.id}/push/subscriptions`,
      )).subscriptions,
  });
  const diagnostics = useQuery({
    queryKey: ["push", "diagnostics", app.id],
    queryFn: () => apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/push/diagnostics`),
  });
  const queue = useQuery({
    queryKey: ["push", "queue", app.id],
    queryFn: () => apiFetch<PushQueueResponse>(`/api/apps/${app.id}/push/queue`),
  });
  const stats = useQuery({
    queryKey: ["push", "queue-stats", app.id],
    queryFn: () => apiFetch<PushQueueStatsResponse>(`/api/apps/${app.id}/push/queue/stats?window_hours=24&limit=5`),
  });
  const sendTest = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/push/test-message`, {
        method: "POST",
        body: JSON.stringify({
          title,
          body,
          user_id: userId || users.data?.[0]?.id,
        }),
    }),
    onSuccess: () => {
      toast.success(t("queued"));
      queryClient.invalidateQueries({ queryKey: ["push", "queue", app.id] });
      queryClient.invalidateQueries({ queryKey: ["push", "queue-stats", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <Tabs defaultValue="subscriptions">
        <TabsList>
          <TabsTrigger value="subscriptions">{t("subscriptions")}</TabsTrigger>
          <TabsTrigger value="test">{t("testMessage")}</TabsTrigger>
          <TabsTrigger value="stats">{t("stats")}</TabsTrigger>
          <TabsTrigger value="diagnostics">{t("diagnostics")}</TabsTrigger>
          <TabsTrigger value="queue">{t("queue")}</TabsTrigger>
        </TabsList>
        <TabsContent value="subscriptions">
          <DataTableView
            loading={subscriptions.isLoading}
            columns={[t("columnsId"), t("columnsKind"), t("columnsTopic"), t("columnsEndpoint"), t("columnsCreated")]}
            rows={(subscriptions.data ?? []).map((subscription) => [
              String(subscription.id ?? ""),
              String(subscription.kind ?? ""),
              String(subscription.topic ?? ""),
              String(subscription.endpoint ?? ""),
              String(subscription.created_at ?? ""),
            ])}
            emptyTitle={t("emptySubscriptionsTitle")}
            emptyDescription={t("emptySubscriptionsDescription")}
          />
        </TabsContent>
        <TabsContent value="test">
          <Panel title={t("queueTestPush")}>
            <div className="grid gap-3 lg:grid-cols-[1fr_1fr_220px_auto]">
              <Input value={title} onChange={(event) => setTitle(event.target.value)} />
              <Input value={body} onChange={(event) => setBody(event.target.value)} />
              <Select value={userId || users.data?.[0]?.id || ""} onValueChange={setUserId}>
                <SelectTrigger><SelectValue placeholder={t("targetUser")} /></SelectTrigger>
                <SelectContent>
                  {(users.data ?? []).map((user) => (
                    <SelectItem key={user.id} value={user.id}>{user.email}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button onClick={() => sendTest.mutate()} disabled={sendTest.isPending || !users.data?.length}>{common("send")}</Button>
            </div>
          </Panel>
        </TabsContent>
        <TabsContent value="stats">
          <PushStatsView stats={stats.data} loading={stats.isLoading} />
        </TabsContent>
        <TabsContent value="diagnostics">
          <Panel title={t("diagnostics")}><JsonBlock value={diagnostics.data ?? { status: common("loading") }} /></Panel>
        </TabsContent>
        <TabsContent value="queue">
          <Panel title={t("queue")}>
            <PushQueueView queue={queue.data} loading={queue.isLoading} />
          </Panel>
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function PushStatsView({ stats, loading }: { stats?: PushQueueStatsResponse; loading?: boolean }) {
  const t = useTranslations("pushView");
  if (loading) return <Skeleton className="h-64 w-full" />;
  if (!stats) {
    return (
      <EmptyState
        icon={TrendingUp}
        title={t("emptyStatsTitle")}
        description={t("emptyStatsDescription")}
        compact
      />
    );
  }
  return (
    <div className="space-y-4">
      <div className="grid gap-3 md:grid-cols-4">
        <Metric label={t("statsWindowHours")} value={`${stats.window_hours}h`} icon={TrendingUp} />
        <Metric label={t("statsRetryScheduled")} value={stats.retry_scheduled} icon={History} />
        <Metric label={t("statsRetryOverdue")} value={stats.retry_overdue} icon={CircleAlert} />
        <Metric label={t("statsReasonLimit")} value={stats.limit} icon={Bell} />
      </div>
      <div className="grid gap-4 lg:grid-cols-2">
        <Panel title={t("statsTerminalFailures")}>
          <DataTableView
            columns={[t("statsReason"), t("statsCount")]}
            rows={stats.terminal_failure_reasons.map((item) => [item.reason, item.count])}
            emptyTitle={t("statsNoFailures")}
            emptyDescription={t("statsNoFailuresDescription")}
          />
        </Panel>
        <Panel title={t("statsDestinationFailures")}>
          <DataTableView
            columns={[t("statsReason"), t("statsCount")]}
            rows={stats.destination_failure_reasons.map((item) => [item.reason, item.count])}
            emptyTitle={t("statsNoFailures")}
            emptyDescription={t("statsNoFailuresDescription")}
          />
        </Panel>
      </div>
    </div>
  );
}

function PushQueueView({ queue, loading }: { queue?: PushQueueResponse; loading?: boolean }) {
  const t = useTranslations("pushView");
  const common = useTranslations("common");
  if (loading) return <Skeleton className="h-64 w-full" />;
  const items = queue?.items ?? [];
  const summary = queue?.summary;
  if (!items.length) {
    return (
      <div className="space-y-3">
        <EmptyState
          icon={Bell}
          title={t("emptyQueueTitle")}
          description={t("emptyQueueDescription")}
          compact
        />
        <details className="rounded-md border bg-background p-3">
          <summary className="cursor-pointer text-sm font-medium text-muted-foreground">{t("rawQueue")}</summary>
          <div className="mt-3">
            <JsonBlock value={queue ?? { status: common("loading") }} minHeight={220} />
          </div>
        </details>
      </div>
    );
  }
  return (
    <div className="space-y-4">
      <div className="grid gap-3 md:grid-cols-4 xl:grid-cols-6">
        <Metric label={t("queuedTotal")} value={summary?.total ?? items.length} icon={Bell} />
        <Metric label={t("queuedPending")} value={summary?.pending ?? 0} icon={History} />
        <Metric label={t("queuedProcessing")} value={summary?.processing ?? 0} icon={History} />
        <Metric label={t("queuedFailed")} value={summary?.failed ?? 0} icon={CircleAlert} />
        <Metric label={t("queuedSent")} value={summary?.sent ?? 0} icon={CheckCircle2} />
        <Metric label={t("queuedPartial")} value={summary?.partial_success ?? 0} icon={CircleAlert} />
      </div>
      <DataTableView
        columns={[
          t("columnsId"),
          t("columnsTitle"),
          t("columnsStatus"),
          t("columnsRetry"),
          t("columnsPartialFailures"),
          t("columnsCreated"),
        ]}
        rows={items.map((item) => [
          item.id,
          item.title,
          item.status,
          item.retry_count,
          item.partial_failure_count,
          item.created_at,
        ])}
      />
      <details className="rounded-md border bg-background p-3">
        <summary className="cursor-pointer text-sm font-medium text-muted-foreground">{t("rawQueue")}</summary>
        <div className="mt-3">
          <JsonBlock value={queue} minHeight={220} />
        </div>
      </details>
    </div>
  );
}
