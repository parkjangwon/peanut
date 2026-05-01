"use client";

import {
  Activity,
  Archive,
  Bell,
  Boxes,
  CheckCircle2,
  CircleAlert,
  Cloud,
  Code2,
  Database,
  Download,
  FlaskConical,
  History,
  KeyRound,
  LayoutDashboard,
  LogOut,
  Menu,
  Play,
  Plus,
  RotateCcw,
  Save,
  ShieldCheck,
  Trash2,
  Users,
  Wrench,
} from "lucide-react";
import type React from "react";
import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { PeanutLogo, PeanutMark } from "@/components/console/brand";
import {
  ActivityEvent,
  apiFetch,
  AppSummary,
  BackupsResponse,
  bootstrapAdmin,
  clearSession,
  DataTable,
  downloadBackup,
  FunctionDetail,
  FunctionInvocation,
  FunctionSummary,
  FunctionVersionSummary,
  loginAdmin,
  logoutAdmin,
  OpsMetrics,
  PeanutUser,
  refreshAdminSession,
  UsageSummary,
  SdkStorageObjectSummary,
  StorageBucket,
  storeSession,
  storedUser,
  WorkspaceSummary,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { useConsoleLocale } from "@/i18n/provider";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Sheet, SheetContent, SheetTrigger } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

type View =
  | "overview"
  | "apps"
  | "keys"
  | "auth"
  | "data"
  | "storage"
  | "functions"
  | "push"
  | "activity"
  | "ops";

const navItems: Array<{ view: View; labelKey: string; icon: React.ComponentType<{ className?: string }> }> = [
  { view: "overview", labelKey: "overview", icon: LayoutDashboard },
  { view: "apps", labelKey: "apps", icon: Boxes },
  { view: "keys", labelKey: "keys", icon: KeyRound },
  { view: "auth", labelKey: "auth", icon: Users },
  { view: "data", labelKey: "data", icon: Database },
  { view: "storage", labelKey: "storage", icon: Archive },
  { view: "functions", labelKey: "functions", icon: Code2 },
  { view: "push", labelKey: "push", icon: Bell },
  { view: "activity", labelKey: "activity", icon: Activity },
  { view: "ops", labelKey: "ops", icon: Wrench },
];

export function ConsoleApp() {
  const [user, setUser] = useState<PeanutUser | null>(() => storedUser());
  const [bootstrapping, setBootstrapping] = useState(false);
  const [view, setView] = useState<View>(() =>
    typeof window === "undefined" ? "overview" : viewFromPath(window.location.pathname),
  );
  const [selectedAppId, setSelectedAppId] = useState<string>("");
  const queryClient = useQueryClient();

  const meQuery = useQuery({
    queryKey: ["admin", "me"],
    queryFn: async () => {
      const response = await apiFetch<{ user: PeanutUser }>("/api/admin/auth/me");
      return response.user;
    },
    enabled: Boolean(user),
    retry: false,
  });

  useEffect(() => {
    if (meQuery.error) {
      refreshAdminSession()
        .then((session) => {
          storeSession(session);
          setUser(session.user);
          queryClient.invalidateQueries();
        })
        .catch(() => {
          clearSession();
          setUser(null);
        });
    }
  }, [meQuery.data, meQuery.error, queryClient]);

  const appsQuery = useQuery({
    queryKey: ["apps"],
    queryFn: async () => (await apiFetch<{ apps: AppSummary[] }>("/api/apps")).apps,
    enabled: Boolean(user),
  });

  const apps = useMemo(() => appsQuery.data ?? [], [appsQuery.data]);
  const selectedApp = useMemo(
    () => apps.find((app) => app.id === selectedAppId) ?? apps[0],
    [apps, selectedAppId],
  );
  const activeUser = meQuery.data ?? user;

  if (!activeUser) {
    return (
      <AuthScreen
        bootstrapping={bootstrapping}
        onModeChange={setBootstrapping}
        onAuthenticated={(session) => {
          storeSession(session);
          setUser(session.user);
            queryClient.invalidateQueries();
        }}
      />
    );
  }

  return (
    <div className="min-h-screen bg-background">
      <div className="flex min-h-screen">
        <aside className="hidden w-64 shrink-0 border-r bg-sidebar px-3 py-4 lg:block">
          <PeanutLogo />
          <Separator className="my-4" />
          <ConsoleNav view={view} onChange={setView} />
        </aside>
        <main className="min-w-0 flex-1">
          <ConsoleHeader
            apps={apps}
            selectedAppId={selectedApp?.id ?? ""}
            user={activeUser}
            view={view}
            onAppChange={setSelectedAppId}
            onViewChange={setView}
            onLogout={async () => {
              await logoutAdmin().catch(() => undefined);
              clearSession();
              setUser(null);
              queryClient.clear();
            }}
          />
          <div className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
            <ViewContent
              view={view}
              apps={apps}
              appsLoading={appsQuery.isLoading}
              selectedApp={selectedApp}
            />
          </div>
        </main>
      </div>
    </div>
  );
}

function AuthScreen({
  bootstrapping,
  onModeChange,
  onAuthenticated,
}: {
  bootstrapping: boolean;
  onModeChange: (value: boolean) => void;
  onAuthenticated: (session: Awaited<ReturnType<typeof loginAdmin>>) => void;
}) {
  const t = useTranslations("auth");
  const [email, setEmail] = useState("admin@peanut.local");
  const [password, setPassword] = useState("");

  const mutation = useMutation({
    mutationFn: () =>
      bootstrapping ? bootstrapAdmin(email, password) : loginAdmin(email, password),
    onSuccess: (session) => {
      toast.success(bootstrapping ? t("adminCreated") : t("signedIn"));
      onAuthenticated(session);
    },
    onError: (error: Error & { status?: number }) => {
      if (!bootstrapping && error.status === 401) {
        toast.error(t("invalidCredentials"));
      } else if (bootstrapping && error.status === 409) {
        toast.error(t("adminExists"));
        onModeChange(false);
      } else {
        toast.error(error.message);
      }
    },
  });

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top_left,color-mix(in_oklch,var(--primary),white_70%),transparent_38%),linear-gradient(180deg,var(--background),white)]">
      <div className="mx-auto grid min-h-screen w-full max-w-6xl grid-cols-1 items-center gap-10 px-5 py-10 lg:grid-cols-[1.05fr_0.95fr]">
        <section className="space-y-8">
          <PeanutLogo />
          <div className="max-w-2xl space-y-5">
            <Badge className="bg-primary/10 text-primary hover:bg-primary/10">
              {t("badge")}
            </Badge>
            <h1 className="text-4xl font-semibold tracking-tight text-foreground sm:text-6xl">
              {t("headline")}
            </h1>
            <p className="max-w-xl text-lg leading-8 text-muted-foreground">
              {t("description")}
            </p>
          </div>
          <div className="grid max-w-2xl grid-cols-1 gap-3 sm:grid-cols-3">
            <Signal icon={ShieldCheck} label={t("signalIsolation")} />
            <Signal icon={Database} label={t("signalData")} />
            <Signal icon={Cloud} label={t("signalOps")} />
          </div>
        </section>
        <section className="rounded-lg border bg-card p-6 shadow-sm">
          <div className="mb-6 flex items-center justify-between gap-4">
            <div>
              <h2 className="text-xl font-semibold">
                {bootstrapping ? t("createFirstAdmin") : t("adminSignIn")}
              </h2>
              <p className="text-sm text-muted-foreground">
                {bootstrapping
                  ? t("freshInstallHelp")
                  : t("signInHelp")}
              </p>
            </div>
            <PeanutMark />
          </div>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              mutation.mutate();
            }}
          >
            <Input
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder={t("emailPlaceholder")}
              autoComplete="email"
            />
            <Input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder={t("passwordPlaceholder")}
              autoComplete={bootstrapping ? "new-password" : "current-password"}
            />
            <Button className="w-full" disabled={mutation.isPending}>
              {mutation.isPending ? t("working") : bootstrapping ? t("createAdmin") : t("signIn")}
            </Button>
          </form>
          <Button
            type="button"
            variant="ghost"
            className="mt-4 w-full"
            onClick={() => onModeChange(!bootstrapping)}
          >
            {bootstrapping ? t("alreadyHaveAdmin") : t("setUpFresh")}
          </Button>
        </section>
      </div>
    </div>
  );
}

function ConsoleHeader({
  apps,
  selectedAppId,
  user,
  view,
  onAppChange,
  onViewChange,
  onLogout,
}: {
  apps: AppSummary[];
  selectedAppId: string;
  user: PeanutUser;
  view: View;
  onAppChange: (appId: string) => void;
  onViewChange: (view: View) => void;
  onLogout: () => void;
}) {
  const t = useTranslations("common");
  const { locale, setLocale } = useConsoleLocale();
  return (
    <header className="sticky top-0 z-20 border-b bg-background/92 backdrop-blur">
      <div className="flex h-16 items-center justify-between gap-3 px-4 sm:px-6 lg:px-8">
        <div className="flex min-w-0 items-center gap-3">
          <Sheet>
            <SheetTrigger asChild>
              <Button size="icon" variant="ghost" className="lg:hidden">
                <Menu className="h-5 w-5" />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-72">
              <PeanutLogo />
              <Separator className="my-4" />
              <ConsoleNav view={view} onChange={onViewChange} />
            </SheetContent>
          </Sheet>
          <Select value={selectedAppId} onValueChange={onAppChange}>
            <SelectTrigger className="w-[230px] max-w-[58vw] bg-card">
              <SelectValue placeholder={t("selectApp")} />
            </SelectTrigger>
            <SelectContent>
              {apps.map((app) => (
                <SelectItem key={app.id} value={app.id}>
                  {app.display_name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-3">
          <Select value={locale} onValueChange={(value) => setLocale(value as "en" | "ko")}>
            <SelectTrigger className="h-9 w-[116px] bg-card" aria-label={t("language")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="en">{t("english")}</SelectItem>
              <SelectItem value="ko">{t("korean")}</SelectItem>
            </SelectContent>
          </Select>
          <div className="hidden text-right text-sm sm:block">
            <div className="font-medium">{user.email}</div>
            <div className="text-xs text-muted-foreground">{t("platformRole", { role: user.admin_role })}</div>
          </div>
          <Button size="icon" variant="outline" onClick={onLogout}>
            <LogOut className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </header>
  );
}

function ConsoleNav({ view, onChange }: { view: View; onChange: (view: View) => void }) {
  const t = useTranslations("nav");
  return (
    <nav className="space-y-1">
      {navItems.map((item) => (
        <button
          key={item.view}
          onClick={() => {
            onChange(item.view);
            window.history.replaceState(null, "", pathForView(item.view));
          }}
          className={cn(
            "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm text-sidebar-foreground transition-colors hover:bg-sidebar-accent",
            view === item.view && "bg-sidebar-accent font-medium text-sidebar-accent-foreground",
          )}
        >
          <item.icon className="h-4 w-4" />
          {t(item.labelKey)}
        </button>
      ))}
    </nav>
  );
}

function ViewContent({
  view,
  apps,
  appsLoading,
  selectedApp,
}: {
  view: View;
  apps: AppSummary[];
  appsLoading: boolean;
  selectedApp?: AppSummary;
}) {
  const t = useTranslations("apps");
  if (appsLoading) return <Skeleton className="h-80 w-full" />;
  if (!selectedApp && view !== "apps") {
    return (
      <EmptyState
        icon={Boxes}
        title={t("emptyTitle")}
        description={t("emptyDescription")}
      />
    );
  }

  switch (view) {
    case "apps":
      return <AppsView apps={apps} />;
    case "keys":
      return <KeysView app={selectedApp!} />;
    case "auth":
      return <AuthView app={selectedApp!} />;
    case "data":
      return <DataView app={selectedApp!} />;
    case "storage":
      return <StorageView app={selectedApp!} />;
    case "functions":
      return <FunctionsView app={selectedApp!} />;
    case "push":
      return <PushView app={selectedApp!} />;
    case "activity":
      return <ActivityView app={selectedApp!} />;
    case "ops":
      return <OpsView />;
    default:
      return <OverviewView apps={apps} app={selectedApp!} />;
  }
}

function OverviewView({ apps, app }: { apps: AppSummary[]; app: AppSummary }) {
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
  const activity = useQuery({
    queryKey: ["activity", app.id],
    queryFn: async () =>
      (await apiFetch<{ events?: ActivityEvent[]; activity?: ActivityEvent[] }>(
        `/api/apps/${app.id}/activity`,
      )).events ?? (await apiFetch<{ events?: ActivityEvent[]; activity?: ActivityEvent[] }>(
        `/api/apps/${app.id}/activity`,
      )).activity ?? [],
  });

  return (
    <Section
      title={t("title")}
      description={t("description")}
      action={<StatusBadge ok={Boolean(ready.data?.ready)} />}
    >
      <div className="grid gap-4 md:grid-cols-4">
        <Metric label={t("apps")} value={apps.length} icon={Boxes} />
        <Metric label={t("selectedApp")} value={app.display_name} icon={ShieldCheck} />
        <Metric label={t("ready")} value={ready.data?.status?.toString() ?? common("checking")} icon={CheckCircle2} />
        <Metric label={t("diagnostics")} value={diagnostics.isError ? common("needsAttention") : common("loaded")} icon={Wrench} />
      </div>
      <div className="grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
        <Panel title={t("recentActivity")}>
          <ActivityList events={activity.data ?? []} />
        </Panel>
        <Panel title={t("platformSignal")}>
          <JsonBlock value={diagnostics.data ?? ready.data ?? { status: "loading" }} />
        </Panel>
      </div>
    </Section>
  );
}

function AppsView({ apps }: { apps: AppSummary[] }) {
  const t = useTranslations("apps");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [displayName, setDisplayName] = useState("");
  const createApp = useMutation({
    mutationFn: () =>
      apiFetch<{ app: AppSummary }>("/api/apps", {
        method: "POST",
        body: JSON.stringify({ name, display_name: displayName }),
      }),
    onSuccess: () => {
      toast.success(t("created"));
      setOpen(false);
      setName("");
      setDisplayName("");
      queryClient.invalidateQueries({ queryKey: ["apps"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Section
      title={t("title")}
      description={t("description")}
      action={
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogTrigger asChild>
            <Button><Plus className="h-4 w-4" />{common("newApp")}</Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader><DialogTitle>{t("createTitle")}</DialogTitle></DialogHeader>
            <div className="space-y-3">
              <Input placeholder={t("namePlaceholder")} value={name} onChange={(event) => setName(event.target.value)} />
              <Input placeholder={t("displayNamePlaceholder")} value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
              <Button className="w-full" onClick={() => createApp.mutate()} disabled={createApp.isPending}>{common("create")}</Button>
            </div>
          </DialogContent>
        </Dialog>
      }
    >
      <DataTableView
        columns={[t("columnsDisplay"), t("columnsName"), t("columnsId"), t("columnsCreated")]}
        rows={apps.map((app) => [app.display_name, app.name, app.id, app.created_at])}
      />
    </Section>
  );
}

function KeysView({ app }: { app: AppSummary }) {
  const t = useTranslations("keys");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const keys = useQuery({
    queryKey: ["keys", app.id],
    queryFn: async () => (await apiFetch<{ app_keys: Array<Record<string, unknown>> }>(`/api/apps/${app.id}/keys`)).app_keys,
  });
  const [keyType, setKeyType] = useState("server");
  const [name, setName] = useState(t("serverKey"));
  const createKey = useMutation({
    mutationFn: () =>
      apiFetch<{ key: string }>(`/api/apps/${app.id}/keys`, {
        method: "POST",
        body: JSON.stringify({ name, key_type: keyType }),
    }),
    onSuccess: (response) => {
      toast.success(t("created", { prefix: response.key.slice(0, 18) }));
      queryClient.invalidateQueries({ queryKey: ["keys", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Section title={t("title")} description={t("description")}>
      <Panel title={t("create")}>
        <div className="grid gap-3 md:grid-cols-[1fr_180px_auto]">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Select value={keyType} onValueChange={setKeyType}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="client">{t("client")}</SelectItem>
              <SelectItem value="server">{t("server")}</SelectItem>
              <SelectItem value="admin">{t("admin")}</SelectItem>
            </SelectContent>
          </Select>
          <Button onClick={() => createKey.mutate()} disabled={createKey.isPending}>{common("create")}</Button>
        </div>
      </Panel>
      <DataTableView
        loading={keys.isLoading}
        columns={["Name", "Type", "Prefix", "Last used", "Status"]}
        rows={(keys.data ?? []).map((key) => [
          String(key.name ?? ""),
          String(key.key_type ?? ""),
          String(key.key_prefix ?? ""),
          String(key.last_used_at ?? common("never")),
          key.revoked_at ? common("revoked") : common("active"),
        ])}
      />
    </Section>
  );
}

function AuthView({ app }: { app: AppSummary }) {
  const users = useQuery({
    queryKey: ["auth", "users", app.id],
    queryFn: async () =>
      (await apiFetch<{ users: PeanutUser[] }>(`/api/apps/${app.id}/auth/users`)).users,
  });
  const providers = useQuery({
    queryKey: ["auth", "providers", app.id],
    queryFn: () => apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/auth/providers`),
  });
  const firstUserId = users.data?.[0]?.id;
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
  return (
    <Section title="Auth" description="User namespace, sessions, OIDC providers, and security events are app-scoped.">
      <Tabs defaultValue="users">
        <TabsList>
          <TabsTrigger value="users">Users</TabsTrigger>
          <TabsTrigger value="sessions">Sessions</TabsTrigger>
          <TabsTrigger value="providers">Providers</TabsTrigger>
        </TabsList>
        <TabsContent value="users">
          <DataTableView
            loading={users.isLoading}
            columns={["Email", "User ID", "Active", "Admin"]}
            rows={(users.data ?? []).map((user) => [
              user.email,
              user.id,
              String(user.is_active),
              String(user.is_admin),
            ])}
          />
        </TabsContent>
        <TabsContent value="sessions">
          <DataTableView
            loading={sessions.isLoading}
            columns={["Session", "Active", "Created", "Last seen", "Expires"]}
            rows={(sessions.data ?? []).map((session) => [
              String(session.session_id ?? ""),
              String(session.is_active ?? ""),
              String(session.created_at ?? ""),
              String(session.last_seen_at ?? ""),
              String(session.expires_at ?? ""),
            ])}
          />
        </TabsContent>
        <TabsContent value="providers"><JsonBlock value={providers.data ?? { status: "loading" }} /></TabsContent>
      </Tabs>
    </Section>
  );
}

function DataView({ app }: { app: AppSummary }) {
  const queryClient = useQueryClient();
  const [selectedTable, setSelectedTable] = useState("");
  const tables = useQuery({
    queryKey: ["data", "tables", app.id],
    queryFn: async () => (await apiFetch<{ tables: DataTable[] }>(`/api/apps/${app.id}/data/tables`)).tables,
  });
  const activeTable =
    selectedTable || String((tables.data?.[0] as unknown as { name?: string })?.name ?? "");
  const rows = useQuery({
    queryKey: ["data", "rows", app.id, activeTable],
    queryFn: async () =>
      (await apiFetch<{ rows: Array<Record<string, unknown>> }>(
        `/api/apps/${app.id}/data/tables/${activeTable}/rows`,
      )).rows,
    enabled: Boolean(activeTable),
  });
  const [name, setName] = useState("notes");
  const [rowJson, setRowJson] = useState('{\n  "title": "Hello Peanut"\n}');
  const createTable = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/data/tables`, {
        method: "POST",
        body: JSON.stringify({
          name,
          display_name: name,
          schema: { fields: [{ name: "title", type: "string", required: true }] },
          access_policy: { mode: "admin_only" },
        }),
      }),
    onSuccess: () => {
      toast.success("Table created");
      queryClient.invalidateQueries({ queryKey: ["data", "tables", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const createRow = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/data/tables/${activeTable}/rows`, {
        method: "POST",
        body: JSON.stringify({ data: JSON.parse(rowJson) }),
      }),
    onSuccess: () => {
      toast.success("Row created");
      queryClient.invalidateQueries({ queryKey: ["data", "rows", app.id, activeTable] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title="Data" description="Create JSON-backed app tables, inspect rows, and manage import/export workflows.">
      <Panel title="Create table">
        <div className="flex gap-3">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Button onClick={() => createTable.mutate()} disabled={createTable.isPending}>Create</Button>
        </div>
      </Panel>
      <Tabs defaultValue="tables">
        <TabsList>
          <TabsTrigger value="tables">Tables</TabsTrigger>
          <TabsTrigger value="rows">Rows</TabsTrigger>
        </TabsList>
        <TabsContent value="tables">
          <DataTableView
            loading={tables.isLoading}
            columns={["Name", "Display", "Policy", "Created"]}
            rows={(tables.data ?? []).map((table) => [
              table.name,
              String((table as unknown as { display_name?: string }).display_name ?? table.name),
              String((table as unknown as { policy_mode?: string }).policy_mode ?? "admin"),
              table.created_at,
            ])}
          />
        </TabsContent>
        <TabsContent value="rows" className="space-y-4">
          <Panel title="Row editor">
            <div className="grid gap-3 lg:grid-cols-[220px_1fr_auto]">
              <Select value={activeTable} onValueChange={setSelectedTable}>
                <SelectTrigger><SelectValue placeholder="Select table" /></SelectTrigger>
                <SelectContent>
                  {(tables.data ?? []).map((table) => (
                    <SelectItem key={table.name} value={table.name}>{table.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Textarea value={rowJson} onChange={(event) => setRowJson(event.target.value)} className="min-h-28 font-mono text-xs" />
              <Button onClick={() => createRow.mutate()} disabled={!activeTable || createRow.isPending}>Create row</Button>
            </div>
          </Panel>
          <DataTableView
            loading={rows.isLoading}
            columns={["ID", "Data", "Created", "Updated"]}
            rows={(rows.data ?? []).map((row) => [
              String(row.id ?? ""),
              JSON.stringify(row.data ?? {}),
              String(row.created_at ?? ""),
              String(row.updated_at ?? ""),
            ])}
          />
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function StorageView({ app }: { app: AppSummary }) {
  const queryClient = useQueryClient();
  const [selectedBucket, setSelectedBucket] = useState("");
  const buckets = useQuery({
    queryKey: ["storage", "buckets", app.id],
    queryFn: async () => (await apiFetch<{ buckets: StorageBucket[] }>(`/api/apps/${app.id}/storage/buckets`)).buckets,
  });
  const activeBucket = selectedBucket || buckets.data?.[0]?.name || "";
  const objects = useQuery({
    queryKey: ["storage", "objects", app.id, activeBucket],
    queryFn: async () =>
      (await apiFetch<{ objects: SdkStorageObjectSummary[] }>(
        `/api/apps/${app.id}/storage/buckets/${activeBucket}/objects`,
      )).objects,
    enabled: Boolean(activeBucket),
  });
  const [name, setName] = useState("assets");
  const [objectKey, setObjectKey] = useState("hello.txt");
  const [objectBody, setObjectBody] = useState("Hello Peanut");
  const createBucket = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/storage/buckets`, {
        method: "POST",
        body: JSON.stringify({
          name,
          public_read: false,
          allow_client_uploads: false,
          max_object_bytes: null,
          allowed_mime_types: [],
        }),
      }),
    onSuccess: () => {
      toast.success("Bucket created");
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const uploadObject = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/storage/buckets/${activeBucket}/objects/${objectKey}`, {
        method: "PUT",
        headers: { "Content-Type": "text/plain" },
        body: objectBody,
      }),
    onSuccess: () => {
      toast.success("Object uploaded");
      queryClient.invalidateQueries({ queryKey: ["storage", "objects", app.id, activeBucket] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title="Storage" description="Bucket policies and object browser entrypoints for app-scoped files.">
      <Panel title="Create bucket">
        <div className="flex gap-3">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Button onClick={() => createBucket.mutate()} disabled={createBucket.isPending}>Create</Button>
        </div>
      </Panel>
      <Tabs defaultValue="buckets">
        <TabsList>
          <TabsTrigger value="buckets">Buckets</TabsTrigger>
          <TabsTrigger value="objects">Objects</TabsTrigger>
        </TabsList>
        <TabsContent value="buckets">
          <DataTableView
            loading={buckets.isLoading}
            columns={["Name", "Public read", "Client uploads", "Updated"]}
            rows={(buckets.data ?? []).map((bucket) => [
              bucket.name,
              String(bucket.public_read),
              String((bucket as unknown as { allow_client_uploads?: boolean }).allow_client_uploads),
              bucket.updated_at,
            ])}
          />
        </TabsContent>
        <TabsContent value="objects" className="space-y-4">
          <Panel title="Upload object">
            <div className="grid gap-3 lg:grid-cols-[220px_220px_1fr_auto]">
              <Select value={activeBucket} onValueChange={setSelectedBucket}>
                <SelectTrigger><SelectValue placeholder="Select bucket" /></SelectTrigger>
                <SelectContent>
                  {(buckets.data ?? []).map((bucket) => (
                    <SelectItem key={bucket.name} value={bucket.name}>{bucket.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Input value={objectKey} onChange={(event) => setObjectKey(event.target.value)} />
              <Input value={objectBody} onChange={(event) => setObjectBody(event.target.value)} />
              <Button onClick={() => uploadObject.mutate()} disabled={!activeBucket || uploadObject.isPending}>Upload</Button>
            </div>
          </Panel>
          <DataTableView
            loading={objects.isLoading}
            columns={["Key", "Size", "Content type", "Updated"]}
            rows={(objects.data ?? []).map((object) => [
              object.key,
              object.size,
              object.content_type ?? "",
              object.updated_at,
            ])}
          />
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function FunctionsView({ app }: { app: AppSummary }) {
  const queryClient = useQueryClient();
  const [selectedName, setSelectedName] = useState("");
  const [name, setName] = useState("hello_console");
  const [displayName, setDisplayName] = useState("Hello Console");
  const [endpointSlug, setEndpointSlug] = useState("hello-console");
  const [runtime, setRuntime] = useState("javascript");
  const [invokePolicy, setInvokePolicy] = useState("authenticated");
  const [enabled, setEnabled] = useState("true");
  const [timeoutMs, setTimeoutMs] = useState("3000");
  const [sourceCode, setSourceCode] = useState(
    "export default function handler(ctx) {\n  return { ok: true, input: ctx.request.input }\n}\n",
  );
  const [inputJson, setInputJson] = useState('{\n  "message": "Hello Peanut"\n}');
  const [output, setOutput] = useState<Record<string, unknown> | null>(null);
  const functions = useQuery({
    queryKey: ["functions", app.id],
    queryFn: async () => (await apiFetch<{ functions: FunctionSummary[] }>(`/api/apps/${app.id}/functions`)).functions,
  });
  const activeName = selectedName || functions.data?.[0]?.name || "";
  const detail = useQuery({
    queryKey: ["functions", "detail", app.id, activeName],
    queryFn: async () =>
      (await apiFetch<{ function: FunctionDetail }>(`/api/apps/${app.id}/functions/${activeName}`)).function,
    enabled: Boolean(activeName),
  });
  const versions = useQuery({
    queryKey: ["functions", "versions", app.id, activeName],
    queryFn: async () =>
      (await apiFetch<{ versions: FunctionVersionSummary[] }>(
        `/api/apps/${app.id}/functions/${activeName}/versions`,
      )).versions,
    enabled: Boolean(activeName),
  });
  const invocations = useQuery({
    queryKey: ["functions", "invocations", app.id, activeName],
    queryFn: async () =>
      (await apiFetch<{ invocations: FunctionInvocation[] }>(
        `/api/apps/${app.id}/functions/${activeName}/invocations`,
      )).invocations,
    enabled: Boolean(activeName),
  });

  const applyFunctionDraft = (fn: FunctionDetail) => {
    setName(fn.name);
    setDisplayName(fn.display_name);
    setEndpointSlug(fn.endpoint_slug);
    setRuntime(fn.runtime);
    setInvokePolicy(fn.invoke_policy);
    setEnabled(String(fn.enabled));
    setTimeoutMs(String(fn.timeout_ms));
    setSourceCode(fn.source_code);
  };

  const loadFunctionDraft = async (functionName: string) => {
    const response = await apiFetch<{ function: FunctionDetail }>(
      `/api/apps/${app.id}/functions/${functionName}`,
    );
    applyFunctionDraft(response.function);
  };

  const functionPayload = () => ({
    name,
    display_name: displayName,
    endpoint_slug: endpointSlug,
    runtime,
    source_code: sourceCode,
    invoke_policy: invokePolicy,
    timeout_ms: Number(timeoutMs),
    enabled: enabled === "true",
  });

  const createFunction = useMutation({
    mutationFn: () =>
      apiFetch<{ function: FunctionDetail }>(`/api/apps/${app.id}/functions`, {
        method: "POST",
        body: JSON.stringify(functionPayload()),
      }),
    onSuccess: (response) => {
      toast.success("Function created");
      setSelectedName(response.function.name);
      applyFunctionDraft(response.function);
      queryClient.invalidateQueries({ queryKey: ["functions", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const updateFunction = useMutation({
    mutationFn: () =>
      apiFetch<{ function: FunctionDetail }>(`/api/apps/${app.id}/functions/${activeName}`, {
        method: "PATCH",
        body: JSON.stringify({
          display_name: displayName,
          endpoint_slug: endpointSlug,
          runtime,
          source_code: sourceCode,
          invoke_policy: invokePolicy,
          timeout_ms: Number(timeoutMs),
          enabled: enabled === "true",
        }),
      }),
    onSuccess: (response) => {
      toast.success("Function saved");
      setSelectedName(response.function.name);
      applyFunctionDraft(response.function);
      queryClient.invalidateQueries({ queryKey: ["functions", app.id] });
      queryClient.invalidateQueries({ queryKey: ["functions", "detail", app.id, activeName] });
      queryClient.invalidateQueries({ queryKey: ["functions", "versions", app.id, activeName] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteFunction = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/functions/${activeName}`, {
        method: "DELETE",
      }),
    onSuccess: () => {
      toast.success("Function deleted");
      setSelectedName("");
      queryClient.invalidateQueries({ queryKey: ["functions", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const lintFunction = useMutation({
    mutationFn: () =>
      apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/functions/editor/lint`, {
        method: "POST",
        body: JSON.stringify({
          runtime,
          source_code: sourceCode,
          function_name: name || activeName || "editor",
          timeout_ms: Number(timeoutMs),
        }),
      }),
    onSuccess: (response) => {
      setOutput(response);
      toast.success("Lint finished");
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const dryRunFunction = useMutation({
    mutationFn: () =>
      apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/functions/editor/dry-run`, {
        method: "POST",
        body: JSON.stringify({
          runtime,
          source_code: sourceCode,
          function_name: name || activeName || "editor",
          input: parseJsonInput(inputJson),
          timeout_ms: Number(timeoutMs),
        }),
      }),
    onSuccess: (response) => {
      setOutput(response);
      toast.success("Dry-run finished");
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const invokeFunction = useMutation({
    mutationFn: () =>
      apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/function-endpoints/${endpointSlug}`, {
        method: "POST",
        body: JSON.stringify({ input: parseJsonInput(inputJson) }),
      }),
    onSuccess: (response) => {
      setOutput(response);
      queryClient.invalidateQueries({ queryKey: ["functions", "invocations", app.id, activeName] });
      toast.success("Invocation recorded");
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const rollbackVersion = useMutation({
    mutationFn: (versionNumber: number) =>
      apiFetch(`/api/apps/${app.id}/functions/${activeName}/versions/${versionNumber}/rollback`, {
        method: "POST",
      }),
    onSuccess: () => {
      toast.success("Version restored");
      queryClient.invalidateQueries({ queryKey: ["functions", app.id] });
      queryClient.invalidateQueries({ queryKey: ["functions", "detail", app.id, activeName] });
      queryClient.invalidateQueries({ queryKey: ["functions", "versions", app.id, activeName] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const retryInvocation = useMutation({
    mutationFn: (invocationId: string) =>
      apiFetch<Record<string, unknown>>(
        `/api/apps/${app.id}/functions/${activeName}/invocations/${invocationId}/retry`,
        { method: "POST" },
      ),
    onSuccess: (response) => {
      setOutput(response);
      queryClient.invalidateQueries({ queryKey: ["functions", "invocations", app.id, activeName] });
      toast.success("Invocation retried");
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Section title="Functions" description="Create, test, invoke, version, and retry app-scoped functions.">
      <div className="grid gap-4 xl:grid-cols-[320px_1fr]">
        <Panel title="Functions">
          <div className="space-y-3">
            <Select
              value={activeName}
              onValueChange={(value) => {
                setSelectedName(value);
                loadFunctionDraft(value).catch((error: Error) => toast.error(error.message));
              }}
            >
              <SelectTrigger><SelectValue placeholder="Select function" /></SelectTrigger>
              <SelectContent>
                {(functions.data ?? []).map((fn) => (
                  <SelectItem key={fn.name} value={fn.name}>{fn.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              onClick={() => {
                if (activeName && detail.data) applyFunctionDraft(detail.data);
              }}
              disabled={!detail.data}
            >
              <Code2 className="h-4 w-4" /> Load
            </Button>
            <DataTableView
              loading={functions.isLoading}
              columns={["Name", "Endpoint", "Version"]}
              rows={(functions.data ?? []).map((fn) => [
                fn.name,
                fn.endpoint_slug,
                fn.active_version_number,
              ])}
            />
          </div>
        </Panel>

        <Panel title="Editor">
          <div className="grid gap-3 lg:grid-cols-3">
            <Input value={name} onChange={(event) => setName(event.target.value)} placeholder="name" />
            <Input value={displayName} onChange={(event) => setDisplayName(event.target.value)} placeholder="display name" />
            <Input value={endpointSlug} onChange={(event) => setEndpointSlug(event.target.value)} placeholder="endpoint slug" />
            <Select value={runtime} onValueChange={setRuntime}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="javascript">JavaScript</SelectItem>
                <SelectItem value="typescript">TypeScript</SelectItem>
              </SelectContent>
            </Select>
            <Select value={invokePolicy} onValueChange={setInvokePolicy}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="authenticated">Authenticated</SelectItem>
                <SelectItem value="public">Public</SelectItem>
                <SelectItem value="api_key">API key</SelectItem>
              </SelectContent>
            </Select>
            <div className="grid grid-cols-[1fr_110px] gap-3">
              <Input value={timeoutMs} onChange={(event) => setTimeoutMs(event.target.value)} />
              <Select value={enabled} onValueChange={setEnabled}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="true">Enabled</SelectItem>
                  <SelectItem value="false">Disabled</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          <Textarea
            value={sourceCode}
            onChange={(event) => setSourceCode(event.target.value)}
            className="mt-3 min-h-[360px] resize-y font-mono text-xs"
          />
          <div className="mt-3 flex flex-wrap gap-2">
            <Button onClick={() => createFunction.mutate()} disabled={createFunction.isPending}>
              <Plus className="h-4 w-4" /> Create
            </Button>
            <Button variant="outline" onClick={() => updateFunction.mutate()} disabled={!activeName || updateFunction.isPending}>
              <Save className="h-4 w-4" /> Save
            </Button>
            <Button variant="outline" onClick={() => lintFunction.mutate()} disabled={lintFunction.isPending}>
              <Code2 className="h-4 w-4" /> Lint
            </Button>
            <Button variant="outline" onClick={() => dryRunFunction.mutate()} disabled={dryRunFunction.isPending}>
              <FlaskConical className="h-4 w-4" /> Dry-run
            </Button>
            <Button variant="outline" onClick={() => invokeFunction.mutate()} disabled={!activeName || invokeFunction.isPending}>
              <Play className="h-4 w-4" /> Invoke
            </Button>
            <Button variant="destructive" onClick={() => deleteFunction.mutate()} disabled={!activeName || deleteFunction.isPending}>
              <Trash2 className="h-4 w-4" /> Delete
            </Button>
          </div>
        </Panel>
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <Panel title="Input / output">
          <div className="grid gap-3 lg:grid-cols-2">
            <Textarea value={inputJson} onChange={(event) => setInputJson(event.target.value)} className="min-h-72 font-mono text-xs" />
            <JsonBlock value={output ?? { status: "idle" }} />
          </div>
        </Panel>
        <Panel title="Versions">
          <DataTableView
            loading={versions.isLoading}
            columns={["Version", "Runtime", "Active", "Rollback"]}
            rows={(versions.data ?? []).map((version) => [
              version.version_number,
              version.runtime,
              String(version.is_active),
              version.is_active ? "" : (
                <Button
                  key={version.id}
                  variant="outline"
                  size="sm"
                  onClick={() => rollbackVersion.mutate(version.version_number)}
                >
                  <RotateCcw className="h-4 w-4" /> Restore
                </Button>
              ),
            ])}
          />
        </Panel>
      </div>

      <Panel title="Invocations">
        <DataTableView
          loading={invocations.isLoading}
          columns={["ID", "Status", "Mode", "Duration", "Retry"]}
          rows={(invocations.data ?? []).map((invocation) => [
            invocation.id,
            invocation.status,
            invocation.invoke_mode,
            invocation.duration_ms ?? "",
            <Button
              key={invocation.id}
              variant="outline"
              size="sm"
              onClick={() => retryInvocation.mutate(invocation.id)}
            >
              <History className="h-4 w-4" /> Retry
            </Button>,
          ])}
        />
      </Panel>
    </Section>
  );
}

function PushView({ app }: { app: AppSummary }) {
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
    queryFn: () => apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/push/queue`),
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
      toast.success("Push test message queued");
      queryClient.invalidateQueries({ queryKey: ["push", "queue", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title="Push" description="Subscription health, queue pressure, and delivery diagnostics.">
      <Tabs defaultValue="subscriptions">
        <TabsList>
          <TabsTrigger value="subscriptions">Subscriptions</TabsTrigger>
          <TabsTrigger value="test">Test message</TabsTrigger>
          <TabsTrigger value="diagnostics">Diagnostics</TabsTrigger>
          <TabsTrigger value="queue">Queue</TabsTrigger>
        </TabsList>
        <TabsContent value="subscriptions">
          <DataTableView
            loading={subscriptions.isLoading}
            columns={["ID", "Kind", "Topic", "Endpoint", "Created"]}
            rows={(subscriptions.data ?? []).map((subscription) => [
              String(subscription.id ?? ""),
              String(subscription.kind ?? ""),
              String(subscription.topic ?? ""),
              String(subscription.endpoint ?? ""),
              String(subscription.created_at ?? ""),
            ])}
          />
        </TabsContent>
        <TabsContent value="test">
          <Panel title="Queue a test push">
            <div className="grid gap-3 lg:grid-cols-[1fr_1fr_220px_auto]">
              <Input value={title} onChange={(event) => setTitle(event.target.value)} />
              <Input value={body} onChange={(event) => setBody(event.target.value)} />
              <Select value={userId || users.data?.[0]?.id || ""} onValueChange={setUserId}>
                <SelectTrigger><SelectValue placeholder="Target user" /></SelectTrigger>
                <SelectContent>
                  {(users.data ?? []).map((user) => (
                    <SelectItem key={user.id} value={user.id}>{user.email}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button onClick={() => sendTest.mutate()} disabled={sendTest.isPending || !users.data?.length}>Send</Button>
            </div>
          </Panel>
        </TabsContent>
        <TabsContent value="diagnostics">
          <Panel title="Diagnostics"><JsonBlock value={diagnostics.data ?? { status: "loading" }} /></Panel>
        </TabsContent>
        <TabsContent value="queue">
          <Panel title="Queue"><JsonBlock value={queue.data ?? { status: "loading" }} /></Panel>
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function ActivityView({ app }: { app: AppSummary }) {
  const activity = useQuery({
    queryKey: ["activity", app.id],
    queryFn: async () => {
      const response = await apiFetch<{ events?: ActivityEvent[]; activity?: ActivityEvent[] }>(
        `/api/apps/${app.id}/activity`,
      );
      return response.events ?? response.activity ?? [];
    },
  });
  return (
    <Section title="Activity" description="A single feed for app mutations across auth, data, storage, functions, push, and keys.">
      <ActivityList events={activity.data ?? []} loading={activity.isLoading} />
    </Section>
  );
}

function OpsView() {
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
      toast.success("Backup created");
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
      toast.success("Restore scheduled");
      queryClient.invalidateQueries({ queryKey: ["ops", "backups"] });
      queryClient.invalidateQueries({ queryKey: ["ops", "metrics"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const clearRestore = useMutation({
    mutationFn: () => apiFetch("/api/admin/backups/restore-pending", { method: "DELETE" }),
    onSuccess: () => {
      toast.success("Restore cleared");
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
      toast.success(`${name} downloaded`);
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const platform = diagnostics.data as { checks?: Array<{ name: string; ok: boolean; message?: string; severity?: string }> } | undefined;
  const platformChecks = platform?.checks ?? [];
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
    <Section title="Operations" description="Single-node production checks, backup posture, schema invariants, and service health.">
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Metric label="Ready" value={String(ready.data?.status ?? "checking")} icon={CheckCircle2} />
        <Metric label="Warnings" value={warningChecks} icon={CircleAlert} />
        <Metric label="Failures" value={failedChecks} icon={Wrench} />
        <Metric label="DB size" value={formatBytes(metrics.data?.database.size_bytes ?? 0)} icon={Database} />
      </div>
      <div className="grid gap-4 xl:grid-cols-[1fr_1.2fr]">
        <Panel title="Platform checks">
          <div className="space-y-2">
            {platformChecks.map((check) => (
              <div key={check.name} className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
                <div className="min-w-0">
                  <div className="flex items-center gap-2 font-medium">
                    {check.name}
                    {check.severity === "warning" && <Badge variant="secondary">warning</Badge>}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">{check.message ?? ""}</div>
                </div>
                <Badge variant={check.ok ? "default" : "destructive"}>{check.ok ? "OK" : "Fail"}</Badge>
              </div>
            ))}
            {!platform?.checks?.length && <Skeleton className="h-40 w-full" />}
          </div>
        </Panel>
        <Panel title="Runtime">
          <DataTableView
            loading={metrics.isLoading}
            columns={["Area", "Value", "State"]}
            rows={[
              ["Storage", metrics.data?.storage.root ?? "", metrics.data?.storage.ok ? "writable" : metrics.data?.storage.error ?? ""],
              ["Backups", metrics.data?.database.backup_count ?? 0, metrics.data?.database.restore_pending ? "restore pending" : "clear"],
              ["Functions", metrics.data?.functions.work_dir ?? "", metrics.data?.functions.enabled ? "enabled" : "disabled"],
              ["Push queue", metrics.data?.push.queued ?? 0, `${metrics.data?.push.failed_recent ?? 0} failed / 24h`],
              ["Version", metrics.data?.system.version ?? "", `${metrics.data?.system.uptime_seconds ?? 0}s uptime`],
            ]}
          />
        </Panel>
      </div>
      <Panel title="Workspace resource usage">
        <DataTableView
          loading={workspaceUsage.isLoading}
          columns={["Workspace", "Resource", "Usage", "Percent", "Period / reset"]}
          rows={usageRows}
        />
      </Panel>
      <Panel title="Backups">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          {backups.data?.restore_pending ? (
            <Alert className="max-w-2xl">
              <CircleAlert className="h-4 w-4" />
              <AlertTitle>Restore pending</AlertTitle>
              <AlertDescription>{backups.data.restore_pending.backup_name}</AlertDescription>
            </Alert>
          ) : (
            <StatusBadge ok />
          )}
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() => {
                if (window.confirm("Clear the pending restore marker?")) {
                  clearRestore.mutate();
                }
              }}
              disabled={!backups.data?.restore_pending || clearRestore.isPending}
            >
              <RotateCcw className="h-4 w-4" /> Clear
            </Button>
            <Button onClick={() => createBackup.mutate()} disabled={createBackup.isPending}>
              <Plus className="h-4 w-4" /> Create
            </Button>
          </div>
        </div>
        <DataTableView
          loading={backups.isLoading}
          columns={["Name", "Size", "Modified", "Actions"]}
          rows={(backups.data?.backups ?? []).map((backup) => [
            backup.name,
            formatBytes(backup.size_bytes),
            backup.modified_at,
            <div key={backup.name} className="flex gap-2">
              <Button variant="outline" size="sm" onClick={() => download.mutate(backup.name)}>
                <Download className="h-4 w-4" /> Download
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  if (window.confirm(`Schedule restore from ${backup.name} on next restart?`)) {
                    scheduleRestore.mutate(backup.name);
                  }
                }}
              >
                <RotateCcw className="h-4 w-4" /> Restore
              </Button>
            </div>,
          ])}
        />
      </Panel>
      <div className="grid gap-4 xl:grid-cols-2">
        <Panel title="Ready payload"><JsonBlock value={ready.data ?? { status: "loading" }} /></Panel>
        <Panel title="Diagnostics payload"><JsonBlock value={diagnostics.data ?? { status: "loading" }} /></Panel>
      </div>
    </Section>
  );
}

function Section({
  title,
  description,
  action,
  children,
}: {
  title: string;
  description: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-5">
      <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-end">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
          <p className="mt-1 max-w-2xl text-sm text-muted-foreground">{description}</p>
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card className="rounded-lg">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">{title}</CardTitle>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}

function Metric({
  label,
  value,
  icon: Icon,
}: {
  label: string;
  value: React.ReactNode;
  icon: React.ComponentType<{ className?: string }>;
}) {
  return (
    <Panel title={label}>
      <div className="flex items-center justify-between gap-3">
        <div className="truncate text-2xl font-semibold">{value}</div>
        <Icon className="h-5 w-5 shrink-0 text-primary" />
      </div>
    </Panel>
  );
}

function DataTableView({
  columns,
  rows,
  loading,
}: {
  columns: string[];
  rows: React.ReactNode[][];
  loading?: boolean;
}) {
  if (loading) return <Skeleton className="h-64 w-full" />;
  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      <Table>
        <TableHeader>
          <TableRow>
            {columns.map((column) => (
              <TableHead key={column}>{column}</TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length === 0 ? (
            <TableRow>
              <TableCell colSpan={columns.length} className="h-28 text-center text-muted-foreground">
                No records yet.
              </TableCell>
            </TableRow>
          ) : (
            rows.map((row, index) => (
              <TableRow key={index}>
                {row.map((cell, cellIndex) => (
                  <TableCell key={cellIndex} className="max-w-[260px] truncate font-mono text-xs">
                    {cell}
                  </TableCell>
                ))}
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  );
}

function ActivityList({ events, loading }: { events: ActivityEvent[]; loading?: boolean }) {
  if (loading) return <Skeleton className="h-72 w-full" />;
  if (!events.length) {
    return <EmptyState icon={Activity} title="No activity yet" description="Mutations will appear here as the app is used." />;
  }
  return (
    <div className="divide-y rounded-lg border bg-card">
      {events.slice(0, 12).map((event) => (
        <div key={event.id} className="flex items-start justify-between gap-4 p-4">
          <div className="min-w-0">
            <div className="font-medium">{event.action}</div>
            <div className="truncate text-sm text-muted-foreground">
              {event.resource_type ?? event.target_type} / {event.resource_id ?? event.target_id}
            </div>
          </div>
          <div className="shrink-0 font-mono text-xs text-muted-foreground">{event.created_at}</div>
        </div>
      ))}
    </div>
  );
}

function EmptyState({
  icon: Icon,
  title,
  description,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
}) {
  return (
    <div className="flex min-h-64 flex-col items-center justify-center rounded-lg border border-dashed bg-card p-8 text-center">
      <Icon className="mb-3 h-8 w-8 text-primary" />
      <h2 className="font-semibold">{title}</h2>
      <p className="mt-1 max-w-md text-sm text-muted-foreground">{description}</p>
    </div>
  );
}

function JsonBlock({ value }: { value: unknown }) {
  return (
    <Textarea
      readOnly
      value={JSON.stringify(value, null, 2)}
      className="min-h-72 resize-none font-mono text-xs"
    />
  );
}

function parseJsonInput(value: string) {
  try {
    return JSON.parse(value);
  } catch {
    throw new Error("Input must be valid JSON");
  }
}

function formatBytes(value: number) {
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

function usagePercent(used: number, limit: number) {
  if (limit <= 0) return used > 0 ? 100 : 0;
  return Math.min(100, Math.round((used / limit) * 100));
}

function Signal({
  icon: Icon,
  label,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
}) {
  return (
    <div className="flex items-center gap-2 rounded-md border bg-card px-3 py-2 text-sm">
      <Icon className="h-4 w-4 text-primary" />
      {label}
    </div>
  );
}

function StatusBadge({ ok }: { ok: boolean }) {
  return (
    <Badge variant={ok ? "default" : "secondary"} className="h-8 gap-1.5">
      {ok ? <CheckCircle2 className="h-3.5 w-3.5" /> : <CircleAlert className="h-3.5 w-3.5" />}
      {ok ? "Ready" : "Checking"}
    </Badge>
  );
}

function viewFromPath(pathname: string): View {
  const segment = pathname.split("/").filter(Boolean).at(-1);
  if (segment && navItems.some((item) => item.view === segment)) return segment as View;
  return "overview";
}

function pathForView(view: View) {
  return view === "overview" ? "/" : `/${view}`;
}
