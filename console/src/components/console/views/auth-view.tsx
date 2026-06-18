"use client";

import { History, Plus, Power, PowerOff, Trash2 } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { apiFetch, AppSummary, PeanutUser } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

import { JsonBlock } from "../shared/code-editor";
import { DataTableView } from "../shared/data-table-view";
import { Panel, Section } from "../shared/layout-primitives";

export function AuthView({ app }: { app: AppSummary }) {
  const t = useTranslations("authView");
  const common = useTranslations("common");
  const [selectedUserId, setSelectedUserId] = useState("");
  const queryClient = useQueryClient();
  const [email, setEmail] = useState("user@example.com");
  const [password, setPassword] = useState("password123");
  const [isActive, setIsActive] = useState("true");
  const [isAdmin, setIsAdmin] = useState("false");
  const users = useQuery({
    queryKey: ["auth", "users", app.id],
    queryFn: async () =>
      (await apiFetch<{ users: PeanutUser[] }>(`/api/apps/${app.id}/auth/users`)).users,
  });
  const providers = useQuery({
    queryKey: ["auth", "providers", app.id],
    queryFn: () => apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/auth/providers`),
  });
  const firstUserId = selectedUserId || users.data?.[0]?.id;
  const sessions = useQuery({
    queryKey: ["auth", "sessions", app.id, firstUserId],
    queryFn: async () =>
      (
        await apiFetch<{ sessions: Array<Record<string, unknown>> }>(
          `/api/apps/${app.id}/auth/users/${firstUserId}/sessions`,
        )
      ).sessions,
    enabled: Boolean(firstUserId),
  });
  const createUser = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/auth/users`, {
        method: "POST",
        body: JSON.stringify({
          email,
          password,
          is_active: isActive === "true",
          is_admin: isAdmin === "true",
          admin_role: isAdmin === "true" ? "developer" : "viewer",
        }),
      }),
    onSuccess: () => {
      toast.success(t("createdUser"));
      queryClient.invalidateQueries({ queryKey: ["auth", "users", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <Tabs defaultValue="users">
        <TabsList>
          <TabsTrigger value="users">{t("users")}</TabsTrigger>
          <TabsTrigger value="sessions">{t("sessions")}</TabsTrigger>
          <TabsTrigger value="providers">{t("providers")}</TabsTrigger>
        </TabsList>
        <TabsContent value="users" className="space-y-4">
          <Panel title={t("createUser")}>
            <div className="grid gap-3 lg:grid-cols-[1fr_180px_140px_140px_auto]">
              <Input value={email} onChange={(event) => setEmail(event.target.value)} placeholder={t("email")} />
              <Input value={password} onChange={(event) => setPassword(event.target.value)} placeholder={t("password")} />
              <Select value={isActive} onValueChange={setIsActive}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="true">{t("active")}: {common("yes")}</SelectItem>
                  <SelectItem value="false">{t("active")}: {common("no")}</SelectItem>
                </SelectContent>
              </Select>
              <Select value={isAdmin} onValueChange={setIsAdmin}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="false">{t("admin")}: {common("no")}</SelectItem>
                  <SelectItem value="true">{t("admin")}: {common("yes")}</SelectItem>
                </SelectContent>
              </Select>
              <Button onClick={() => createUser.mutate()} disabled={createUser.isPending}>
                <Plus className="h-4 w-4" /> {common("create")}
              </Button>
            </div>
          </Panel>
          <DataTableView
            loading={users.isLoading}
            columns={[t("email"), t("userId"), t("active"), t("admin"), t("actions")]}
            rows={(users.data ?? []).map((user) => [
              user.email,
              user.id,
              user.is_active ? common("yes") : common("no"),
              user.is_admin ? common("yes") : common("no"),
              <AuthUserActions key={user.id} appId={app.id} user={user} onInspect={setSelectedUserId} />,
            ])}
          />
        </TabsContent>
        <TabsContent value="sessions">
          <div className="mb-3 max-w-xs">
            <Select value={firstUserId ?? ""} onValueChange={setSelectedUserId}>
              <SelectTrigger><SelectValue placeholder={t("userId")} /></SelectTrigger>
              <SelectContent>
                {(users.data ?? []).map((user) => (
                  <SelectItem key={user.id} value={user.id}>{user.email}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <DataTableView
            loading={sessions.isLoading}
            columns={[t("session"), t("active"), t("created"), t("lastSeen"), t("expires"), t("actions")]}
            rows={(sessions.data ?? []).map((session) => [
              String(session.session_id ?? ""),
              session.is_active ? common("yes") : common("no"),
              String(session.created_at ?? ""),
              String(session.last_seen_at ?? ""),
              String(session.expires_at ?? ""),
              <SessionActions
                key={String(session.session_id)}
                appId={app.id}
                userId={firstUserId ?? ""}
                sessionId={String(session.session_id ?? "")}
                active={Boolean(session.is_active)}
              />,
            ])}
          />
        </TabsContent>
        <TabsContent value="providers"><JsonBlock value={providers.data ?? { status: common("loading") }} /></TabsContent>
      </Tabs>
    </Section>
  );
}

function AuthUserActions({
  appId,
  user,
  onInspect,
}: {
  appId: string;
  user: PeanutUser;
  onInspect: (userId: string) => void;
}) {
  const t = useTranslations("authView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const toggleUser = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/auth/users/${user.id}/${user.is_active ? "deactivate" : "activate"}`, {
        method: "POST",
      }),
    onSuccess: () => {
      toast.success(user.is_active ? t("userDeactivated") : t("userActivated"));
      queryClient.invalidateQueries({ queryKey: ["auth", "users", appId] });
      queryClient.invalidateQueries({ queryKey: ["auth", "sessions", appId, user.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteUser = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${appId}/auth/users/${user.id}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("userDeleted"));
      queryClient.invalidateQueries({ queryKey: ["auth", "users", appId] });
      queryClient.invalidateQueries({ queryKey: ["auth", "sessions", appId, user.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <div className="flex justify-end gap-1">
      <Button variant="outline" size="sm" onClick={() => onInspect(user.id)}>
        <History className="h-4 w-4" /> {t("sessions")}
      </Button>
      <Button
        variant={user.is_active ? "destructive" : "outline"}
        size="sm"
        disabled={toggleUser.isPending}
        onClick={() => {
          const message = user.is_active
            ? t("confirmDeactivate", { email: user.email })
            : t("confirmActivate", { email: user.email });
          if (window.confirm(message)) {
            toggleUser.mutate();
          }
        }}
      >
        {user.is_active ? <PowerOff className="h-4 w-4" /> : <Power className="h-4 w-4" />}
        {user.is_active ? common("deactivate") : common("activate")}
      </Button>
      <Button
        variant="destructive"
        size="sm"
        disabled={deleteUser.isPending}
        onClick={() => {
          if (window.confirm(t("confirmDeleteUser", { email: user.email }))) {
            deleteUser.mutate();
          }
        }}
      >
        <Trash2 className="h-4 w-4" /> {common("delete")}
      </Button>
    </div>
  );
}

function SessionActions({
  appId,
  userId,
  sessionId,
  active,
}: {
  appId: string;
  userId: string;
  sessionId: string;
  active: boolean;
}) {
  const t = useTranslations("authView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const revokeSession = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/auth/users/${userId}/sessions/${sessionId}`, {
        method: "DELETE",
      }),
    onSuccess: () => {
      toast.success(t("sessionRevoked"));
      queryClient.invalidateQueries({ queryKey: ["auth", "sessions", appId, userId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Button
      variant="destructive"
      size="sm"
      disabled={!active || !userId || !sessionId || revokeSession.isPending}
      onClick={() => {
        if (window.confirm(t("confirmSessionRevoke"))) {
          revokeSession.mutate();
        }
      }}
    >
      <Trash2 className="h-4 w-4" /> {common("delete")}
    </Button>
  );
}
