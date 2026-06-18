"use client";

import { Activity } from "lucide-react";
import { useTranslations } from "next-intl";

import type { ActivityEvent } from "@/lib/api";
import { Skeleton } from "@/components/ui/skeleton";

import { EmptyState } from "./empty-state";

export function ActivityList({ events, loading }: { events: ActivityEvent[]; loading?: boolean }) {
  const t = useTranslations("activityView");
  if (loading) return <Skeleton className="h-72 w-full" />;
  if (!events.length) {
    return <EmptyState icon={Activity} title={t("emptyTitle")} description={t("emptyDescription")} />;
  }
  return (
    <div className="divide-y rounded-lg border bg-card">
      {events.slice(0, 12).map((event) => (
        <div key={event.id} className="flex items-start justify-between gap-4 p-4">
          <div className="min-w-0">
            <div className="font-medium">{activityActionLabel(event.action, t)}</div>
            <div className="truncate text-sm text-muted-foreground">
              {activityResourceLabel(event.resource_type ?? event.target_type, t)}
            </div>
          </div>
          <div className="shrink-0 font-mono text-xs text-muted-foreground">{event.created_at}</div>
        </div>
      ))}
    </div>
  );
}

export function activityActionLabel(action: string, t: ReturnType<typeof useTranslations>) {
  const normalized = action.replace(/^app_/, "project_").replace(/\./g, "_");
  const labels: Record<string, string> = {
    project_key_created: t("actions.keyCreated"),
    project_key_rotated: t("actions.keyRotated"),
    project_key_revoked: t("actions.keyRevoked"),
    auth_user_created: t("actions.userCreated"),
    auth_user_deleted: t("actions.userDeleted"),
    auth_user_activated: t("actions.userActivated"),
    auth_user_deactivated: t("actions.userDeactivated"),
    auth_provider_updated: t("actions.providerUpdated"),
    data_table_created: t("actions.tableCreated"),
    data_table_updated: t("actions.tableUpdated"),
    data_table_deleted: t("actions.tableDeleted"),
    data_row_created: t("actions.rowCreated"),
    data_row_updated: t("actions.rowUpdated"),
    data_row_deleted: t("actions.rowDeleted"),
    storage_bucket_created: t("actions.bucketCreated"),
    storage_bucket_updated: t("actions.bucketUpdated"),
    storage_bucket_deleted: t("actions.bucketDeleted"),
    storage_object_put: t("actions.objectUploaded"),
    storage_object_deleted: t("actions.objectDeleted"),
    function_created: t("actions.functionCreated"),
    function_updated: t("actions.functionUpdated"),
    function_deleted: t("actions.functionDeleted"),
    function_rolled_back: t("actions.functionRolledBack"),
    push_message_queued: t("actions.pushQueued"),
    push_message_enqueued: t("actions.pushQueued"),
    backup_restore_scheduled: t("actions.restoreScheduled"),
    workspace_setup_invite_created: t("actions.workspaceInviteCreated"),
    workspace_disabled: t("actions.workspaceDisabled"),
    workspace_enabled: t("actions.workspaceEnabled"),
    project_created: t("actions.projectCreated"),
    project_updated: t("actions.projectUpdated"),
    project_deleted: t("actions.projectDeleted"),
    project_disabled: t("actions.projectDisabled"),
    project_enabled: t("actions.projectEnabled"),
    admin_role_updated: t("actions.adminRoleUpdated"),
  };
  return labels[normalized] ?? t("actions.projectEvent");
}

export function activityResourceLabel(resourceType: string | null | undefined, t: ReturnType<typeof useTranslations>) {
  if (!resourceType) return t("resource.projectEvent");
  const normalized = resourceType.replace(/^app_/, "project_");
  const labels: Record<string, string> = {
    project_key: t("resource.apiKey"),
    auth_user: t("resource.user"),
    auth_provider: t("resource.provider"),
    data_table: t("resource.table"),
    data_row: t("resource.row"),
    storage_bucket: t("resource.bucket"),
    storage_object: t("resource.object"),
    function: t("resource.function"),
    push_message: t("resource.pushMessage"),
    backup: t("resource.backup"),
    workspace: t("resource.workspace"),
    workspace_setup_invite: t("resource.workspaceInvite"),
    project: t("resource.project"),
    admin_role: t("resource.adminRole"),
  };
  return labels[normalized] ?? t("resource.projectEvent");
}
