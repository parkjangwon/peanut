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
  KeyRound,
  LayoutDashboard,
  LogOut,
  Menu,
  Plus,
  ShieldCheck,
  Users,
  Wrench,
} from "lucide-react";
import type React from "react";
import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { PeanutLogo, PeanutMark } from "@/components/console/brand";
import {
  ActivityEvent,
  apiFetch,
  AppSummary,
  bootstrapAdmin,
  clearSession,
  DataTable,
  FunctionSummary,
  loginAdmin,
  logoutAdmin,
  PeanutUser,
  refreshAdminSession,
  SdkStorageObjectSummary,
  StorageBucket,
  storeSession,
  storedUser,
} from "@/lib/api";
import { cn } from "@/lib/utils";
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

const navItems: Array<{ view: View; label: string; icon: React.ComponentType<{ className?: string }> }> = [
  { view: "overview", label: "Overview", icon: LayoutDashboard },
  { view: "apps", label: "Apps", icon: Boxes },
  { view: "keys", label: "API Keys", icon: KeyRound },
  { view: "auth", label: "Auth", icon: Users },
  { view: "data", label: "Data", icon: Database },
  { view: "storage", label: "Storage", icon: Archive },
  { view: "functions", label: "Functions", icon: Code2 },
  { view: "push", label: "Push", icon: Bell },
  { view: "activity", label: "Activity", icon: Activity },
  { view: "ops", label: "Operations", icon: Wrench },
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
  const [email, setEmail] = useState("admin@peanut.local");
  const [password, setPassword] = useState("");

  const mutation = useMutation({
    mutationFn: () =>
      bootstrapping ? bootstrapAdmin(email, password) : loginAdmin(email, password),
    onSuccess: (session) => {
      toast.success(bootstrapping ? "Admin created" : "Signed in");
      onAuthenticated(session);
    },
    onError: (error: Error & { status?: number }) => {
      if (!bootstrapping && error.status === 401) {
        toast.error("Invalid credentials");
      } else if (bootstrapping && error.status === 409) {
        toast.error("An admin already exists. Sign in instead.");
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
              Rust single-binary BaaS
            </Badge>
            <h1 className="text-4xl font-semibold tracking-tight text-foreground sm:text-6xl">
              Peanut control plane, roasted clean.
            </h1>
            <p className="max-w-xl text-lg leading-8 text-muted-foreground">
              Manage apps, auth, data, storage, functions, push, and operations
              from the console embedded inside the Peanut binary.
            </p>
          </div>
          <div className="grid max-w-2xl grid-cols-1 gap-3 sm:grid-cols-3">
            <Signal icon={ShieldCheck} label="App isolation" />
            <Signal icon={Database} label="Data workbench" />
            <Signal icon={Cloud} label="Ops ready" />
          </div>
        </section>
        <section className="rounded-lg border bg-card p-6 shadow-sm">
          <div className="mb-6 flex items-center justify-between gap-4">
            <div>
              <h2 className="text-xl font-semibold">
                {bootstrapping ? "Create first admin" : "Admin sign in"}
              </h2>
              <p className="text-sm text-muted-foreground">
                {bootstrapping
                  ? "Use this once on a fresh Peanut install."
                  : "Sign in with a platform admin account."}
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
              placeholder="admin@example.com"
              autoComplete="email"
            />
            <Input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder="At least 8 characters"
              autoComplete={bootstrapping ? "new-password" : "current-password"}
            />
            <Button className="w-full" disabled={mutation.isPending}>
              {mutation.isPending ? "Working..." : bootstrapping ? "Create admin" : "Sign in"}
            </Button>
          </form>
          <Button
            type="button"
            variant="ghost"
            className="mt-4 w-full"
            onClick={() => onModeChange(!bootstrapping)}
          >
            {bootstrapping ? "I already have an admin" : "Set up a fresh install"}
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
              <SelectValue placeholder="Select app" />
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
          <div className="hidden text-right text-sm sm:block">
            <div className="font-medium">{user.email}</div>
            <div className="text-xs text-muted-foreground">Platform admin</div>
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
          {item.label}
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
  if (appsLoading) return <Skeleton className="h-80 w-full" />;
  if (!selectedApp && view !== "apps") {
    return (
      <EmptyState
        icon={Boxes}
        title="Create your first app"
        description="Peanut isolates Auth, Data, Storage, Functions, and Push by app."
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
      )).events ?? [],
  });

  return (
    <Section
      title="Platform overview"
      description="A quick read on isolation, runtime readiness, and recent movement."
      action={<StatusBadge ok={Boolean(ready.data?.ready)} />}
    >
      <div className="grid gap-4 md:grid-cols-4">
        <Metric label="Apps" value={apps.length} icon={Boxes} />
        <Metric label="Selected app" value={app.display_name} icon={ShieldCheck} />
        <Metric label="Ready" value={ready.data?.status?.toString() ?? "checking"} icon={CheckCircle2} />
        <Metric label="Diagnostics" value={diagnostics.isError ? "needs attention" : "loaded"} icon={Wrench} />
      </div>
      <div className="grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
        <Panel title="Recent activity">
          <ActivityList events={activity.data ?? []} />
        </Panel>
        <Panel title="Platform signal">
          <JsonBlock value={diagnostics.data ?? ready.data ?? { status: "loading" }} />
        </Panel>
      </div>
    </Section>
  );
}

function AppsView({ apps }: { apps: AppSummary[] }) {
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
      toast.success("App created");
      setOpen(false);
      setName("");
      setDisplayName("");
      queryClient.invalidateQueries({ queryKey: ["apps"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Section
      title="Apps"
      description="Each app has isolated users, tables, buckets, functions, push subscriptions, and keys."
      action={
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogTrigger asChild>
            <Button><Plus className="h-4 w-4" />New app</Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader><DialogTitle>Create app</DialogTitle></DialogHeader>
            <div className="space-y-3">
              <Input placeholder="name, e.g. mobile-prod" value={name} onChange={(event) => setName(event.target.value)} />
              <Input placeholder="Display name" value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
              <Button className="w-full" onClick={() => createApp.mutate()} disabled={createApp.isPending}>Create</Button>
            </div>
          </DialogContent>
        </Dialog>
      }
    >
      <DataTableView
        columns={["Display name", "Name", "ID", "Created"]}
        rows={apps.map((app) => [app.display_name, app.name, app.id, app.created_at])}
      />
    </Section>
  );
}

function KeysView({ app }: { app: AppSummary }) {
  const queryClient = useQueryClient();
  const keys = useQuery({
    queryKey: ["keys", app.id],
    queryFn: async () => (await apiFetch<{ app_keys: Array<Record<string, unknown>> }>(`/api/apps/${app.id}/keys`)).app_keys,
  });
  const [keyType, setKeyType] = useState("server");
  const [name, setName] = useState("Server key");
  const createKey = useMutation({
    mutationFn: () =>
      apiFetch<{ key: string }>(`/api/apps/${app.id}/keys`, {
        method: "POST",
        body: JSON.stringify({ name, key_type: keyType }),
      }),
    onSuccess: (response) => {
      toast.success("Key created. Copy it now: " + response.key.slice(0, 18) + "...");
      queryClient.invalidateQueries({ queryKey: ["keys", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Section title="API keys" description="Create least-privilege keys for clients, servers, and admin automation.">
      <Panel title="Create key">
        <div className="grid gap-3 md:grid-cols-[1fr_180px_auto]">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Select value={keyType} onValueChange={setKeyType}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="client">Client</SelectItem>
              <SelectItem value="server">Server</SelectItem>
              <SelectItem value="admin">Admin</SelectItem>
            </SelectContent>
          </Select>
          <Button onClick={() => createKey.mutate()} disabled={createKey.isPending}>Create</Button>
        </div>
      </Panel>
      <DataTableView
        loading={keys.isLoading}
        columns={["Name", "Type", "Prefix", "Last used", "Status"]}
        rows={(keys.data ?? []).map((key) => [
          String(key.name ?? ""),
          String(key.key_type ?? ""),
          String(key.key_prefix ?? ""),
          String(key.last_used_at ?? "never"),
          key.revoked_at ? "revoked" : "active",
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
  const functions = useQuery({
    queryKey: ["functions", app.id],
    queryFn: async () => (await apiFetch<{ functions: FunctionSummary[] }>(`/api/apps/${app.id}/functions`)).functions,
  });
  return (
    <Section title="Functions" description="Edge-style TypeScript functions, versions, dry-runs, and invocation trails.">
      <Alert>
        <Code2 className="h-4 w-4" />
        <AlertTitle>Function workbench</AlertTitle>
        <AlertDescription>
          The console surfaces lint, dry-run, invoke, versions, and invocations through the app-scoped Functions API.
        </AlertDescription>
      </Alert>
      <DataTableView
        loading={functions.isLoading}
        columns={["Name", "Endpoint", "Runtime", "Enabled", "Updated"]}
        rows={(functions.data ?? []).map((fn) => [
          fn.name,
          fn.endpoint_slug,
          String((fn as unknown as { runtime?: string }).runtime ?? "deno"),
          String((fn as unknown as { enabled?: boolean }).enabled ?? true),
          fn.updated_at,
        ])}
      />
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
    queryFn: async () =>
      (await apiFetch<{ events?: ActivityEvent[]; activity?: ActivityEvent[] }>(
        `/api/apps/${app.id}/activity`,
      )).events ?? [],
  });
  return (
    <Section title="Activity" description="A single feed for app mutations across auth, data, storage, functions, push, and keys.">
      <ActivityList events={activity.data ?? []} loading={activity.isLoading} />
    </Section>
  );
}

function OpsView() {
  const ready = useQuery({
    queryKey: ["ready"],
    queryFn: () => apiFetch<Record<string, unknown>>("/api/ready", { auth: false }),
  });
  const diagnostics = useQuery({
    queryKey: ["ops", "diagnostics"],
    queryFn: () => apiFetch<Record<string, unknown>>("/api/admin/ops/diagnostics"),
  });
  const backups = useQuery({
    queryKey: ["ops", "backups"],
    queryFn: () => apiFetch<Record<string, unknown>>("/api/admin/backups"),
  });
  return (
    <Section title="Operations" description="Single-node production checks, backup posture, schema invariants, and service health.">
      <div className="grid gap-4 lg:grid-cols-3">
        <Panel title="Ready"><JsonBlock value={ready.data ?? { status: "loading" }} /></Panel>
        <Panel title="Diagnostics"><JsonBlock value={diagnostics.data ?? { status: "loading" }} /></Panel>
        <Panel title="Backups"><JsonBlock value={backups.data ?? { status: "loading" }} /></Panel>
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
              {event.resource_type} / {event.resource_id}
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
