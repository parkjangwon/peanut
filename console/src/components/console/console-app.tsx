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
  Copy,
  Database,
  Download,
  FlaskConical,
  History,
  Info,
  KeyRound,
  LayoutDashboard,
  LogOut,
  Menu,
  Pencil,
  Play,
  Plus,
  Power,
  PowerOff,
  RotateCcw,
  Save,
  ShieldCheck,
  Trash2,
  Users,
  Wrench,
} from "lucide-react";
import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
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
import type { ConsoleLocale } from "@/i18n/provider";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
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
              onViewChange={setView}
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
  const common = useTranslations("common");
  const { locale, setLocale } = useConsoleLocale();
  const [email, setEmail] = useState("admin@peanut.local");
  const [password, setPassword] = useState("");
  const [rememberMe, setRememberMe] = useState(true);
  const initializedAuthLocale = useRef(false);

  useEffect(() => {
    if (initializedAuthLocale.current) return;
    initializedAuthLocale.current = true;
    if (locale !== "en") {
      setLocale("en");
    }
  }, [locale, setLocale]);

  const mutation = useMutation({
    mutationFn: () =>
      bootstrapping ? bootstrapAdmin(email, password) : loginAdmin(email, password),
    onSuccess: (session) => {
      toast.success(bootstrapping ? t("adminCreated") : t("signedIn"));
      storeSession(session, rememberMe);
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
      <div className="mx-auto flex min-h-screen w-full max-w-[1360px] flex-col px-5 py-8">
        <div className="mb-8 flex items-center justify-between gap-4">
          <PeanutLogo />
          <LocaleSelect
            locale={locale}
            onChange={setLocale}
            label={common("language")}
            english={common("english")}
            korean={common("korean")}
            className="bg-card"
          />
        </div>
        <div className="grid flex-1 grid-cols-1 items-center gap-10 xl:grid-cols-[minmax(0,1.4fr)_minmax(440px,0.6fr)]">
          <section className="space-y-7">
            <div className="max-w-3xl space-y-5">
              <Badge className="bg-primary/10 text-primary hover:bg-primary/10">
                {t("badge")}
              </Badge>
              <h1 className="max-w-3xl whitespace-pre-line text-balance text-4xl font-semibold tracking-tight text-foreground sm:text-5xl xl:text-6xl">
                {t("headline")}
              </h1>
              <p className="max-w-3xl whitespace-pre-line text-pretty text-lg leading-8 text-muted-foreground">
                {t("description")}
              </p>
            </div>
            <div className="grid max-w-3xl grid-cols-1 gap-3 sm:grid-cols-3">
              <Signal icon={ShieldCheck} label={t("signalIsolation")} />
              <Signal icon={Database} label={t("signalData")} />
              <Signal icon={Cloud} label={t("signalOps")} />
            </div>
            <div className="grid max-w-[900px] grid-cols-1 gap-3 sm:grid-cols-2">
              <FeatureSignal icon={Users} title={t("featureAuth")} description={t("featureAuthDescription")} />
              <FeatureSignal icon={Database} title={t("featureData")} description={t("featureDataDescription")} />
              <FeatureSignal icon={Archive} title={t("featureStorage")} description={t("featureStorageDescription")} />
              <FeatureSignal icon={Code2} title={t("featureFunctions")} description={t("featureFunctionsDescription")} />
              <FeatureSignal icon={Bell} title={t("featurePush")} description={t("featurePushDescription")} />
              <FeatureSignal icon={Wrench} title={t("featureOps")} description={t("featureOpsDescription")} />
            </div>
          </section>
          <section className="w-full max-w-[520px] justify-self-center rounded-lg border bg-card p-6 shadow-sm xl:justify-self-end">
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
            {!bootstrapping ? (
              <label className="flex cursor-pointer items-center justify-between gap-3 rounded-md border bg-muted/20 px-3 py-2 text-sm">
                <span>
                  <span className="block font-medium">{t("rememberMe")}</span>
                  <span className="block text-xs text-muted-foreground">{t("rememberMeHelp")}</span>
                </span>
                <input
                  type="checkbox"
                  checked={rememberMe}
                  onChange={(event) => setRememberMe(event.target.checked)}
                  className="h-4 w-4 accent-primary"
                />
              </label>
            ) : null}
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
    </div>
  );
}

function FeatureSignal({
  icon: Icon,
  title,
  description,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
}) {
  return (
    <div className="rounded-lg border bg-card/70 p-4">
      <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-md bg-primary/10 text-primary">
        <Icon className="h-5 w-5" />
      </div>
      <div className="font-medium">{title}</div>
      <div className="mt-1 text-sm leading-6 text-muted-foreground">{description}</div>
    </div>
  );
}

function LocaleSelect({
  locale,
  onChange,
  label,
  english,
  korean,
  className,
}: {
  locale: ConsoleLocale;
  onChange: (locale: ConsoleLocale) => void;
  label: string;
  english: string;
  korean: string;
  className?: string;
}) {
  return (
    <Select value={locale} onValueChange={(value) => onChange(parseLocale(value))}>
      <SelectTrigger className={cn("w-[128px]", className)} aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="en">{english}</SelectItem>
        <SelectItem value="ko">{korean}</SelectItem>
      </SelectContent>
    </Select>
  );
}

function parseLocale(value: string): ConsoleLocale {
  return value === "ko" ? "ko" : "en";
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
              <Button size="icon" variant="ghost" className="lg:hidden" aria-label={t("openNavigation")}>
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
            <SelectTrigger className="w-[230px] max-w-[58vw] bg-card" aria-label={t("selectApp")}>
              <SelectValue placeholder={t("selectApp")} />
            </SelectTrigger>
            <SelectContent>
              {apps.map((app) => (
                <SelectItem key={app.id} value={app.id}>
                  {displayProjectName(app)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-3">
          <LocaleSelect
            locale={locale}
            onChange={setLocale}
            label={t("language")}
            english={t("english")}
            korean={t("korean")}
            className="h-9 bg-card"
          />
          <div className="hidden text-right text-sm sm:block">
            <div className="font-medium">{user.email}</div>
            <div className="text-xs text-muted-foreground">{t("platformRole", { role: user.admin_role })}</div>
          </div>
          <Button size="icon" variant="outline" onClick={onLogout} aria-label={t("signOut")}>
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
  onViewChange,
}: {
  view: View;
  apps: AppSummary[];
  appsLoading: boolean;
  selectedApp?: AppSummary;
  onViewChange: (view: View) => void;
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
      return <OverviewView apps={apps} app={selectedApp!} onViewChange={onViewChange} />;
  }
}

function OverviewView({
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
        columns={[
          t("columnsDisplay"),
          t("columnsName"),
          t("columnsId"),
          t("columnsUpdated"),
          t("columnsActions"),
        ]}
        rows={apps.map((app) => [
          displayProjectName(app),
          app.name,
          app.id,
          app.updated_at,
          <AppRowActions key={app.id} app={app} />,
        ])}
        emptyTitle={t("emptyTitle")}
        emptyDescription={t("emptyDescription")}
      />
    </Section>
  );
}

function AppRowActions({ app }: { app: AppSummary }) {
  const t = useTranslations("apps");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [displayName, setDisplayName] = useState(app.display_name);
  const updateApp = useMutation({
    mutationFn: () =>
      apiFetch<{ app: AppSummary }>(`/api/apps/${app.id}`, {
        method: "PATCH",
        body: JSON.stringify({ display_name: displayName }),
      }),
    onSuccess: () => {
      toast.success(t("updated"));
      setOpen(false);
      queryClient.invalidateQueries({ queryKey: ["apps"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteApp = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${app.id}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("deleted"));
      queryClient.invalidateQueries({ queryKey: ["apps"] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <div className="flex justify-end gap-1">
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button variant="outline" size="sm">
            <Pencil className="h-4 w-4" /> {common("edit")}
          </Button>
        </DialogTrigger>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("editTitle")}</DialogTitle>
            <DialogDescription>{t("editDescription")}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Input value={app.name} disabled />
            <Input
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              placeholder={t("displayNamePlaceholder")}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>{common("cancel")}</Button>
            <Button onClick={() => updateApp.mutate()} disabled={updateApp.isPending}>
              <Save className="h-4 w-4" /> {common("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Button
        variant="destructive"
        size="sm"
        onClick={() => {
          if (window.confirm(t("confirmDelete", { name: app.display_name }))) {
            deleteApp.mutate();
          }
        }}
        disabled={app.id === "default" || deleteApp.isPending}
      >
        <Trash2 className="h-4 w-4" /> {common("delete")}
      </Button>
    </div>
  );
}

function displayProjectName(app: AppSummary) {
  const displayName = app.display_name || app.name;
  return displayName.replace(/\bApp\b/g, "Project").replace(/\bApps\b/g, "Projects");
}

async function copyTextToClipboard(text: string): Promise<void> {
  if (copyTextWithTextArea(text)) {
    return;
  }

  if (typeof navigator !== "undefined" && navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch (error) {
      throw normalizeCopyError(error);
    }
  }

  throw new Error("Clipboard copy is not available in this browser.");
}

function copyTextWithTextArea(text: string) {
  if (typeof document === "undefined" || !document.body) {
    return false;
  }

  const selection = document.getSelection();
  const previousRange = selection && selection.rangeCount > 0 ? selection.getRangeAt(0).cloneRange() : null;
  const textArea = document.createElement("textarea");
  textArea.value = text;
  textArea.readOnly = true;
  textArea.setAttribute("aria-hidden", "true");
  textArea.style.position = "fixed";
  textArea.style.insetBlockStart = "0";
  textArea.style.insetInlineStart = "0";
  textArea.style.width = "1px";
  textArea.style.height = "1px";
  textArea.style.opacity = "0";

  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();

  try {
    return document.execCommand("copy");
  } finally {
    textArea.remove();
    if (selection && previousRange) {
      selection.removeAllRanges();
      selection.addRange(previousRange);
    }
  }
}

function normalizeCopyError(error: unknown) {
  return error instanceof Error ? error : new Error("Clipboard copy failed.");
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
  const [visibleKey, setVisibleKey] = useState<{ name: string; key: string } | null>(null);
  const createKey = useMutation({
    mutationFn: () =>
      apiFetch<{ key: string }>(`/api/apps/${app.id}/keys`, {
        method: "POST",
        body: JSON.stringify({ name, key_type: keyType }),
    }),
    onSuccess: (response) => {
      setVisibleKey({ name, key: response.key });
      toast.success(t("created"));
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
      {visibleKey ? (
        <Panel title={t("newKeyTitle")}>
          <div className="space-y-3">
            <div className="text-sm font-medium">{visibleKey.name}</div>
            <p className="text-sm text-muted-foreground">{t("newKeyHelp")}</p>
            <div className="grid gap-2 md:grid-cols-[1fr_auto]">
              <div className="flex min-h-10 items-center rounded-md border border-input bg-muted/40 px-3 font-mono text-xs text-muted-foreground">
                <span className="sr-only">{t("fullKey")}</span>
                <span aria-hidden="true">********************</span>
              </div>
              <Button
                variant="outline"
                onClick={() => {
                  copyTextToClipboard(visibleKey.key)
                    .then(() => toast.success(common("copied")))
                    .catch((error: unknown) => toast.error(normalizeCopyError(error).message));
                }}
              >
                <Copy className="h-4 w-4" /> {common("copy")}
              </Button>
            </div>
          </div>
        </Panel>
      ) : null}
      <DataTableView
        loading={keys.isLoading}
        columns={[
          t("columnsName"),
          t("columnsType"),
          t("columnsPrefix"),
          t("columnsLastUsed"),
          t("columnsStatus"),
          t("columnsActions"),
        ]}
        rows={(keys.data ?? []).map((key) => [
          String(key.name ?? ""),
          String(key.key_type ?? ""),
          String(key.key_prefix ?? ""),
          String(key.last_used_at ?? common("never")),
          key.revoked_at ? common("revoked") : common("active"),
          <KeyRowActions
            key={String(key.id)}
            appId={app.id}
            keyId={String(key.id)}
            name={String(key.name ?? key.key_prefix ?? "")}
            revoked={Boolean(key.revoked_at)}
            onKeyRotated={(rotatedKey) => setVisibleKey(rotatedKey)}
          />,
        ])}
        emptyTitle={t("emptyTitle")}
        emptyDescription={t("emptyDescription")}
      />
    </Section>
  );
}

function KeyRowActions({
  appId,
  keyId,
  name,
  revoked,
  onKeyRotated,
}: {
  appId: string;
  keyId: string;
  name: string;
  revoked: boolean;
  onKeyRotated: (rotatedKey: { name: string; key: string }) => void;
}) {
  const t = useTranslations("keys");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const revokeKey = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${appId}/keys/${keyId}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("revoked"));
      queryClient.invalidateQueries({ queryKey: ["keys", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const rotateKey = useMutation({
    mutationFn: () =>
      apiFetch<{ key: string }>(`/api/apps/${appId}/keys/${keyId}/rotate`, { method: "POST" }),
    onSuccess: (response) => {
      onKeyRotated({ name, key: response.key });
      toast.success(t("rotated"));
      queryClient.invalidateQueries({ queryKey: ["keys", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <div className="flex justify-end gap-1">
      <Button
        variant="outline"
        size="sm"
        disabled={revoked || rotateKey.isPending}
        onClick={() => {
          if (window.confirm(t("confirmRotate", { name }))) {
            rotateKey.mutate();
          }
        }}
      >
        <RotateCcw className="h-4 w-4" /> {common("rotate")}
      </Button>
      <Button
        variant="destructive"
        size="sm"
        disabled={revoked || revokeKey.isPending}
        onClick={() => {
          if (window.confirm(t("confirmRevoke", { name }))) {
            revokeKey.mutate();
          }
        }}
      >
        <Trash2 className="h-4 w-4" /> {common("revoke")}
      </Button>
    </div>
  );
}

function AuthView({ app }: { app: AppSummary }) {
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

function DataView({ app }: { app: AppSummary }) {
  const t = useTranslations("dataView");
  const common = useTranslations("common");
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
  const [sql, setSql] = useState("select * from notes order by created_at desc limit 20;");
  const [sqlResult, setSqlResult] = useState<unknown>(null);
  const createTable = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/data/tables`, {
        method: "POST",
        body: JSON.stringify({
          name,
          display_name: name,
          schema: { fields: { title: { type: "string", required: true } } },
          access_policy: { mode: "admin_only" },
        }),
    }),
    onSuccess: () => {
      toast.success(t("tableCreated"));
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
      toast.success(t("rowCreated"));
      queryClient.invalidateQueries({ queryKey: ["data", "rows", app.id, activeTable] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const executeSql = useMutation({
    mutationFn: () =>
      apiFetch<unknown>(`/api/apps/${app.id}/data/query`, {
        method: "POST",
        body: JSON.stringify({ sql }),
      }),
    onSuccess: (result) => {
      setSqlResult(result);
      toast.success(t("sqlExecuted"));
      queryClient.invalidateQueries({ queryKey: ["data", "rows", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <Panel title={t("createTable")}>
        <div className="flex gap-3">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Button onClick={() => createTable.mutate()} disabled={createTable.isPending}>{common("create")}</Button>
        </div>
      </Panel>
      <Panel title={t("sqlConsole")}>
        <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_260px]">
          <Textarea
            value={sql}
            onChange={(event) => setSql(event.target.value)}
            className="min-h-36 font-mono text-xs"
          />
          <div className="space-y-3">
            <Button onClick={() => executeSql.mutate()} disabled={executeSql.isPending || !sql.trim()}>
              <Play className="h-4 w-4" /> {t("runSql")}
            </Button>
            <div className="rounded-md border bg-muted/20 p-3 text-xs leading-5 text-muted-foreground">
              {t("sqlHelp")}
            </div>
          </div>
        </div>
        <JsonBlock value={sqlResult ?? { status: t("sqlEmptyResult") }} minHeight={180} />
      </Panel>
      <Tabs defaultValue="tables">
        <TabsList>
          <TabsTrigger value="tables">{t("tables")}</TabsTrigger>
          <TabsTrigger value="rows">{t("rows")}</TabsTrigger>
        </TabsList>
        <TabsContent value="tables">
          <DataTableView
            loading={tables.isLoading}
            columns={[t("name"), t("display"), t("policy"), t("created"), common("actions")]}
            rows={(tables.data ?? []).map((table) => [
              table.name,
              String((table as unknown as { display_name?: string }).display_name ?? table.name),
              String((table as unknown as { policy_mode?: string }).policy_mode ?? "admin"),
              table.created_at,
              <DataTableActions key={table.name} appId={app.id} tableName={table.name} />,
            ])}
            emptyTitle={t("emptyTablesTitle")}
            emptyDescription={t("emptyTablesDescription")}
          />
        </TabsContent>
        <TabsContent value="rows" className="space-y-4">
          <Panel title={t("rowEditor")}>
            <div className="grid gap-3 lg:grid-cols-[220px_1fr_auto]">
              <Select value={activeTable} onValueChange={setSelectedTable}>
                <SelectTrigger><SelectValue placeholder={t("selectTable")} /></SelectTrigger>
                <SelectContent>
                  {(tables.data ?? []).map((table) => (
                    <SelectItem key={table.name} value={table.name}>{table.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Textarea value={rowJson} onChange={(event) => setRowJson(event.target.value)} className="min-h-28 font-mono text-xs" />
              <Button onClick={() => createRow.mutate()} disabled={!activeTable || createRow.isPending}>{t("createRow")}</Button>
            </div>
          </Panel>
          <DataTableView
            loading={rows.isLoading}
            columns={["ID", t("data"), t("created"), t("updated"), common("actions")]}
            rows={(rows.data ?? []).map((row) => [
              String(row.id ?? ""),
              JSON.stringify(row.data ?? {}),
              String(row.created_at ?? ""),
              String(row.updated_at ?? ""),
              <DataRowActions
                key={String(row.id)}
                appId={app.id}
                tableName={activeTable}
                rowId={String(row.id ?? "")}
                data={row.data ?? {}}
              />,
            ])}
            emptyTitle={activeTable ? t("emptyRowsTitle") : t("emptyRowsNoTableTitle")}
            emptyDescription={activeTable ? t("emptyRowsDescription") : t("emptyRowsNoTableDescription")}
          />
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function DataTableActions({ appId, tableName }: { appId: string; tableName: string }) {
  const t = useTranslations("dataView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [displayName, setDisplayName] = useState(tableName);
  const [schemaJson, setSchemaJson] = useState("");
  const [policyMode, setPolicyMode] = useState("admin_only");

  const openEditor = async () => {
    setOpen(true);
    setLoadingDraft(true);
    try {
      const response = await apiFetch<{ table: Record<string, unknown> }>(
        `/api/apps/${appId}/data/tables/${tableName}`,
      );
      setDisplayName(String(response.table.display_name ?? tableName));
      setSchemaJson(JSON.stringify(response.table.schema ?? { fields: {} }, null, 2));
      const policy = response.table.access_policy as { mode?: string } | undefined;
      setPolicyMode(policy?.mode ?? "admin_only");
    } catch (error) {
      toast.error((error as Error).message);
    } finally {
      setLoadingDraft(false);
    }
  };

  const updateTable = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/data/tables/${tableName}`, {
        method: "PATCH",
        body: JSON.stringify({
          display_name: displayName,
          schema: parseJsonInput(schemaJson, common("inputJsonInvalid")),
          access_policy: { mode: policyMode },
        }),
      }),
    onSuccess: () => {
      toast.success(t("tableUpdated"));
      setOpen(false);
      queryClient.invalidateQueries({ queryKey: ["data", "tables", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteTable = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${appId}/data/tables/${tableName}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("tableDeleted"));
      queryClient.invalidateQueries({ queryKey: ["data", "tables", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const exportTable = useMutation({
    mutationFn: () => apiFetch<Record<string, unknown>>(`/api/apps/${appId}/data/tables/${tableName}/export`),
    onSuccess: (payload) => {
      const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${tableName}.json`;
      link.click();
      URL.revokeObjectURL(url);
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <div className="flex justify-end gap-1">
      <Dialog open={open} onOpenChange={setOpen}>
        <Button variant="outline" size="sm" onClick={openEditor}><Pencil className="h-4 w-4" /> {common("edit")}</Button>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("editTable")}</DialogTitle>
            <DialogDescription>{tableName}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            <Input value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
            <Select value={policyMode} onValueChange={setPolicyMode}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="admin_only">admin_only</SelectItem>
                <SelectItem value="owner_private">owner_private</SelectItem>
                <SelectItem value="authenticated_shared_rw">authenticated_shared_rw</SelectItem>
              </SelectContent>
            </Select>
            <Textarea value={schemaJson} onChange={(event) => setSchemaJson(event.target.value)} className="min-h-56 font-mono text-xs" />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>{common("cancel")}</Button>
            <Button onClick={() => updateTable.mutate()} disabled={updateTable.isPending || loadingDraft}>
              <Save className="h-4 w-4" /> {common("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Button variant="outline" size="sm" onClick={() => exportTable.mutate()} disabled={exportTable.isPending}>
        <Download className="h-4 w-4" /> {t("export")}
      </Button>
      <Button
        variant="destructive"
        size="sm"
        disabled={deleteTable.isPending}
        onClick={() => {
          if (window.confirm(t("confirmDeleteTable", { name: tableName }))) {
            deleteTable.mutate();
          }
        }}
      >
        <Trash2 className="h-4 w-4" /> {common("delete")}
      </Button>
    </div>
  );
}

function DataRowActions({
  appId,
  tableName,
  rowId,
  data,
}: {
  appId: string;
  tableName: string;
  rowId: string;
  data: unknown;
}) {
  const t = useTranslations("dataView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [rowJson, setRowJson] = useState(JSON.stringify(data, null, 2));
  const updateRow = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/data/tables/${tableName}/rows/${rowId}`, {
        method: "PATCH",
        body: JSON.stringify({ data: parseJsonInput(rowJson, common("inputJsonInvalid")) }),
      }),
    onSuccess: () => {
      toast.success(t("rowUpdated"));
      setOpen(false);
      queryClient.invalidateQueries({ queryKey: ["data", "rows", appId, tableName] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteRow = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${appId}/data/tables/${tableName}/rows/${rowId}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("rowDeleted"));
      queryClient.invalidateQueries({ queryKey: ["data", "rows", appId, tableName] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <div className="flex justify-end gap-1">
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button variant="outline" size="sm"><Pencil className="h-4 w-4" /> {common("edit")}</Button>
        </DialogTrigger>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{t("editRow")}</DialogTitle>
            <DialogDescription>{rowId}</DialogDescription>
          </DialogHeader>
          <Textarea value={rowJson} onChange={(event) => setRowJson(event.target.value)} className="min-h-72 font-mono text-xs" />
          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>{common("cancel")}</Button>
            <Button onClick={() => updateRow.mutate()} disabled={updateRow.isPending}>
              <Save className="h-4 w-4" /> {common("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Button
        variant="destructive"
        size="sm"
        disabled={deleteRow.isPending}
        onClick={() => {
          if (window.confirm(t("confirmDeleteRow"))) {
            deleteRow.mutate();
          }
        }}
      >
        <Trash2 className="h-4 w-4" /> {common("delete")}
      </Button>
    </div>
  );
}

function StorageView({ app }: { app: AppSummary }) {
  const t = useTranslations("storageView");
  const common = useTranslations("common");
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
  const [contentType, setContentType] = useState("text/plain");
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
      toast.success(t("bucketCreated"));
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", app.id] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const uploadObject = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${app.id}/storage/buckets/${activeBucket}/objects/${objectKey}`, {
        method: "PUT",
        headers: { "Content-Type": contentType || "text/plain" },
        body: objectBody,
      }),
    onSuccess: () => {
      toast.success(t("objectUploaded"));
      queryClient.invalidateQueries({ queryKey: ["storage", "objects", app.id, activeBucket] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <Panel title={t("createBucket")}>
        <div className="flex gap-3">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Button onClick={() => createBucket.mutate()} disabled={createBucket.isPending}>{common("create")}</Button>
        </div>
      </Panel>
      <Tabs defaultValue="buckets">
        <TabsList>
          <TabsTrigger value="buckets">{t("buckets")}</TabsTrigger>
          <TabsTrigger value="objects">{t("objects")}</TabsTrigger>
        </TabsList>
        <TabsContent value="buckets">
          <DataTableView
            loading={buckets.isLoading}
            columns={[t("name"), t("publicRead"), t("clientUploads"), t("maxBytes"), t("updated"), t("columnsActions")]}
            rows={(buckets.data ?? []).map((bucket) => [
              bucket.name,
              bucket.public_read ? common("yes") : common("no"),
              bucket.allow_client_uploads ? common("yes") : common("no"),
              bucket.max_object_bytes ? formatBytes(bucket.max_object_bytes) : common("no"),
              bucket.updated_at,
              <BucketActions key={bucket.name} appId={app.id} bucket={bucket} />,
            ])}
            emptyTitle={t("emptyBucketsTitle")}
            emptyDescription={t("emptyBucketsDescription")}
          />
        </TabsContent>
        <TabsContent value="objects" className="space-y-4">
          <Panel title={t("overwriteObject")}>
            <div className="grid gap-3 lg:grid-cols-[220px_220px_180px_1fr_auto]">
              <Select value={activeBucket} onValueChange={setSelectedBucket}>
                <SelectTrigger><SelectValue placeholder={t("selectBucket")} /></SelectTrigger>
                <SelectContent>
                  {(buckets.data ?? []).map((bucket) => (
                    <SelectItem key={bucket.name} value={bucket.name}>{bucket.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Input value={objectKey} onChange={(event) => setObjectKey(event.target.value)} />
              <Input value={contentType} onChange={(event) => setContentType(event.target.value)} placeholder={t("contentType")} />
              <Input value={objectBody} onChange={(event) => setObjectBody(event.target.value)} placeholder={t("objectBody")} />
              <Button onClick={() => uploadObject.mutate()} disabled={!activeBucket || uploadObject.isPending}>{common("upload")}</Button>
            </div>
          </Panel>
          <DataTableView
            loading={objects.isLoading}
            columns={[t("key"), t("size"), t("contentType"), t("updated"), t("columnsActions")]}
            rows={(objects.data ?? []).map((object) => [
              object.key,
              object.size,
              object.content_type ?? "",
              object.updated_at,
              <ObjectActions key={object.key} appId={app.id} bucket={activeBucket} objectKey={object.key} />,
            ])}
            emptyTitle={activeBucket ? t("emptyObjectsTitle") : t("emptyObjectsNoBucketTitle")}
            emptyDescription={activeBucket ? t("emptyObjectsDescription") : t("emptyObjectsNoBucketDescription")}
          />
        </TabsContent>
      </Tabs>
    </Section>
  );
}

function BucketActions({ appId, bucket }: { appId: string; bucket: StorageBucket }) {
  const t = useTranslations("storageView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [publicRead, setPublicRead] = useState(bucket.public_read ? "true" : "false");
  const [clientUploads, setClientUploads] = useState(bucket.allow_client_uploads ? "true" : "false");
  const [maxBytes, setMaxBytes] = useState(bucket.max_object_bytes ? String(bucket.max_object_bytes) : "");
  const [mimeTypes, setMimeTypes] = useState(storageMimeTypes(bucket).join(", "));

  const updateBucket = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/storage/buckets/${bucket.name}`, {
        method: "PATCH",
        body: JSON.stringify({
          public_read: publicRead === "true",
          allow_client_uploads: clientUploads === "true",
          max_object_bytes: maxBytes.trim() ? Number(maxBytes) : null,
          allowed_mime_types: mimeTypes
            .split(",")
            .map((value) => value.trim())
            .filter(Boolean),
        }),
      }),
    onSuccess: () => {
      toast.success(t("bucketUpdated"));
      setOpen(false);
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const deleteBucket = useMutation({
    mutationFn: () => apiFetch(`/api/apps/${appId}/storage/buckets/${bucket.name}`, { method: "DELETE" }),
    onSuccess: () => {
      toast.success(t("bucketDeleted"));
      queryClient.invalidateQueries({ queryKey: ["storage", "buckets", appId] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <div className="flex justify-end gap-1">
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button variant="outline" size="sm">
            <Pencil className="h-4 w-4" /> {common("edit")}
          </Button>
        </DialogTrigger>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("editBucket")}</DialogTitle>
            <DialogDescription>{bucket.name}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <Select value={publicRead} onValueChange={setPublicRead}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="false">{t("publicRead")}: {common("no")}</SelectItem>
                <SelectItem value="true">{t("publicRead")}: {common("yes")}</SelectItem>
              </SelectContent>
            </Select>
            <Select value={clientUploads} onValueChange={setClientUploads}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="false">{t("clientUploads")}: {common("no")}</SelectItem>
                <SelectItem value="true">{t("clientUploads")}: {common("yes")}</SelectItem>
              </SelectContent>
            </Select>
            <Input
              value={maxBytes}
              onChange={(event) => setMaxBytes(event.target.value)}
              placeholder={t("emptyNoLimit")}
            />
            <Input
              value={mimeTypes}
              onChange={(event) => setMimeTypes(event.target.value)}
              placeholder={t("mimeTypesPlaceholder")}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>{common("cancel")}</Button>
            <Button onClick={() => updateBucket.mutate()} disabled={updateBucket.isPending}>
              <Save className="h-4 w-4" /> {common("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Button
        variant="destructive"
        size="sm"
        disabled={deleteBucket.isPending}
        onClick={() => {
          if (window.confirm(t("confirmDeleteBucket", { name: bucket.name }))) {
            deleteBucket.mutate();
          }
        }}
      >
        <Trash2 className="h-4 w-4" /> {common("delete")}
      </Button>
    </div>
  );
}

function ObjectActions({ appId, bucket, objectKey }: { appId: string; bucket: string; objectKey: string }) {
  const t = useTranslations("storageView");
  const common = useTranslations("common");
  const queryClient = useQueryClient();
  const deleteObject = useMutation({
    mutationFn: () =>
      apiFetch(`/api/apps/${appId}/storage/buckets/${bucket}/objects/${objectKey}`, {
        method: "DELETE",
      }),
    onSuccess: () => {
      toast.success(t("objectDeleted"));
      queryClient.invalidateQueries({ queryKey: ["storage", "objects", appId, bucket] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Button
      variant="destructive"
      size="sm"
      disabled={deleteObject.isPending}
      onClick={() => {
        if (window.confirm(t("confirmDeleteObject", { key: objectKey }))) {
          deleteObject.mutate();
        }
      }}
    >
      <Trash2 className="h-4 w-4" /> {common("delete")}
    </Button>
  );
}

function FunctionsView({ app }: { app: AppSummary }) {
  const t = useTranslations("functionsView");
  const common = useTranslations("common");
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
  const [requestMethod, setRequestMethod] = useState("POST");
  const [queryJson, setQueryJson] = useState('{\n}');
  const [inputJson, setInputJson] = useState('{\n  "input": {\n    "message": "Hello Peanut"\n  }\n}');
  const [output, setOutput] = useState<Record<string, unknown> | null>(null);
  const [browserOrigin] = useState(() => (typeof window === "undefined" ? "" : window.location.origin));
  const metrics = useQuery({
    queryKey: ["ops", "metrics"],
    queryFn: () => apiFetch<OpsMetrics>("/api/admin/ops/metrics"),
  });
  const runtimeEnabled = metrics.data?.functions.enabled ?? true;
  const functions = useQuery({
    queryKey: ["functions", app.id],
    queryFn: async () => (await apiFetch<{ functions: FunctionSummary[] }>(`/api/apps/${app.id}/functions`)).functions,
    enabled: runtimeEnabled,
  });
  const activeName = selectedName || functions.data?.[0]?.name || "";
  const detail = useQuery({
    queryKey: ["functions", "detail", app.id, activeName],
    queryFn: async () =>
      (await apiFetch<{ function: FunctionDetail }>(`/api/apps/${app.id}/functions/${activeName}`)).function,
    enabled: runtimeEnabled && Boolean(activeName),
  });
  const versions = useQuery({
    queryKey: ["functions", "versions", app.id, activeName],
    queryFn: async () =>
      (await apiFetch<{ versions: FunctionVersionSummary[] }>(
        `/api/apps/${app.id}/functions/${activeName}/versions`,
      )).versions,
    enabled: runtimeEnabled && Boolean(activeName),
  });
  const invocations = useQuery({
    queryKey: ["functions", "invocations", app.id, activeName],
    queryFn: async () =>
      (await apiFetch<{ invocations: FunctionInvocation[] }>(
        `/api/apps/${app.id}/functions/${activeName}/invocations`,
      )).invocations,
    enabled: runtimeEnabled && Boolean(activeName),
  });
  const queryString = safeBuildQueryString(queryJson);
  const endpointPath = `/api/apps/${app.id}/function-endpoints/${endpointSlug || "{endpoint-slug}"}${queryString ? `?${queryString}` : ""}`;
  const endpointUrl = browserOrigin ? `${browserOrigin}${endpointPath}` : endpointPath;

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
      toast.success(t("functionCreated"));
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
      toast.success(t("functionSaved"));
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
      toast.success(t("functionDeleted"));
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
      toast.success(t("lintFinished"));
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const dryRunFunction = useMutation({
    mutationFn: () => {
      const testPayload = parseFunctionTestPayload(inputJson, common("inputJsonInvalid"));
      const query = parseJsonInput(queryJson, common("inputJsonInvalid"));
      return apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/functions/editor/dry-run`, {
        method: "POST",
        body: JSON.stringify({
          runtime,
          source_code: sourceCode,
          function_name: name || activeName || "editor",
          method: requestMethod,
          input: testPayload.input,
          query,
          body: testPayload.requestBody,
          timeout_ms: Number(timeoutMs),
        }),
      });
    },
    onSuccess: (response) => {
      setOutput(response);
      toast.success(t("dryRunFinished"));
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const invokeFunction = useMutation({
    mutationFn: () => {
      const testPayload = parseFunctionTestPayload(inputJson, common("inputJsonInvalid"));
      const query = buildQueryString(queryJson, common("inputJsonInvalid"));
      return apiFetch<Record<string, unknown>>(`/api/apps/${app.id}/function-endpoints/${endpointSlug}${query ? `?${query}` : ""}`, {
        method: requestMethod,
        body: requestMethod === "GET" || requestMethod === "HEAD" ? undefined : JSON.stringify(testPayload.requestBody),
      });
    },
    onSuccess: (response) => {
      setOutput(response);
      queryClient.invalidateQueries({ queryKey: ["functions", "invocations", app.id, activeName] });
      toast.success(t("invocationRecorded"));
    },
    onError: (error: Error) => toast.error(error.message),
  });
  const rollbackVersion = useMutation({
    mutationFn: (versionNumber: number) =>
      apiFetch(`/api/apps/${app.id}/functions/${activeName}/versions/${versionNumber}/rollback`, {
        method: "POST",
      }),
    onSuccess: () => {
      toast.success(t("versionRestored"));
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
      toast.success(t("invocationRetried"));
    },
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <Section title={t("title")} description={t("description")}>
      {!runtimeEnabled && (
        <Alert>
          <Info className="h-4 w-4" />
          <AlertTitle>{t("runtimeDisabledTitle")}</AlertTitle>
          <AlertDescription>{t("runtimeDisabledDescription")}</AlertDescription>
        </Alert>
      )}
      <div className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)]">
        <Panel title={t("functions")}>
          <div className="space-y-4">
            <Select
              value={activeName}
              onValueChange={(value) => {
                setSelectedName(value);
                loadFunctionDraft(value).catch((error: Error) => toast.error(error.message));
              }}
            >
              <SelectTrigger><SelectValue placeholder={t("selectFunction")} /></SelectTrigger>
              <SelectContent>
                {(functions.data ?? []).map((fn) => (
                  <SelectItem key={fn.name} value={fn.name}>{fn.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              className="w-full"
              onClick={() => {
                if (activeName && detail.data) applyFunctionDraft(detail.data);
              }}
              disabled={!runtimeEnabled || !detail.data}
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
            ) : functions.isLoading ? (
              <Skeleton className="h-44 w-full" />
            ) : (functions.data ?? []).length ? (
              <div className="space-y-2">
                {(functions.data ?? []).map((fn) => (
                  <button
                    key={fn.id}
                    type="button"
                    onClick={() => {
                      setSelectedName(fn.name);
                      loadFunctionDraft(fn.name).catch((error: Error) => toast.error(error.message));
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

        <div className="space-y-4">
          <Panel title={t("endpoint")}>
            <div className="grid gap-3 xl:grid-cols-[1fr_auto]">
              <div className="min-w-0">
                <div className="mb-1 flex items-center gap-2">
                  <Badge variant={enabled === "true" ? "default" : "secondary"}>
                    {enabled === "true" ? t("enabled") : t("disabled")}
                  </Badge>
                  <span className="text-xs text-muted-foreground">{t("endpointHelp")}</span>
                </div>
                <div className="truncate rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs">
                  {endpointUrl}
                </div>
              </div>
              <Button
                variant="outline"
                onClick={() => {
                  copyTextToClipboard(endpointUrl)
                    .then(() => toast.success(common("copied")))
                    .catch((error: unknown) => toast.error(normalizeCopyError(error).message));
                }}
                disabled={!runtimeEnabled}
              >
                <Copy className="h-4 w-4" /> {t("copyEndpoint")}
              </Button>
            </div>
          </Panel>

          <Panel title={t("editor")}>
            <div className="grid gap-4 2xl:grid-cols-[minmax(0,1fr)_340px]">
              <div className="space-y-4">
                <div className="grid gap-3 lg:grid-cols-3">
                  <FunctionField label={t("name")} help={t("nameHelp")}>
                    <Input value={name} onChange={(event) => setName(event.target.value)} placeholder={t("name")} />
                  </FunctionField>
                  <FunctionField label={t("displayName")} help={t("displayNameHelp")}>
                    <Input value={displayName} onChange={(event) => setDisplayName(event.target.value)} placeholder={t("displayName")} />
                  </FunctionField>
                  <FunctionField label={t("endpointSlug")} help={t("endpointSlugHelp")}>
                    <Input value={endpointSlug} onChange={(event) => setEndpointSlug(event.target.value)} placeholder={t("endpointSlug")} />
                  </FunctionField>
                </div>
                <div className="overflow-hidden rounded-lg border bg-muted/20">
                  <div className="flex items-center justify-between border-b px-3 py-2">
                    <div>
                      <div className="text-sm font-medium">{t("code")}</div>
                      <div className="text-xs text-muted-foreground">{t("codeHelp")}</div>
                    </div>
                    <Badge variant="outline">{runtime}</Badge>
                  </div>
                  <CodeEditor value={sourceCode} onChange={setSourceCode} />
                </div>
              </div>
              <div className="space-y-4">
                <div className="rounded-lg border bg-background p-4">
                  <div className="mb-4">
                    <div className="text-sm font-medium">{t("runtimeSettings")}</div>
                    <div className="text-xs text-muted-foreground">{t("runtimeSettingsHelp")}</div>
                  </div>
                  <div className="space-y-4">
                    <FunctionField label={t("runtime")} help={t("runtimeHelp")}>
                      <Select value={runtime} onValueChange={setRuntime}>
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>
                          <SelectItem value="javascript">JavaScript</SelectItem>
                          <SelectItem value="typescript">TypeScript</SelectItem>
                        </SelectContent>
                      </Select>
                    </FunctionField>
                    <FunctionField label={t("invokePolicy")} help={t("invokePolicyHelp")}>
                      <Select value={invokePolicy} onValueChange={setInvokePolicy}>
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>
                          <SelectItem value="authenticated">{t("authenticated")}</SelectItem>
                          <SelectItem value="public">{t("public")}</SelectItem>
                          <SelectItem value="api_key">API key</SelectItem>
                        </SelectContent>
                      </Select>
                    </FunctionField>
                    <FunctionField label={t("timeoutMs")} help={t("timeoutHelp")}>
                      <Input value={timeoutMs} onChange={(event) => setTimeoutMs(event.target.value)} inputMode="numeric" />
                    </FunctionField>
                    <FunctionField label={t("state")} help={t("stateHelp")}>
                      <Select value={enabled} onValueChange={setEnabled}>
                        <SelectTrigger><SelectValue /></SelectTrigger>
                        <SelectContent>
                          <SelectItem value="true">{t("enabled")}</SelectItem>
                          <SelectItem value="false">{t("disabled")}</SelectItem>
                        </SelectContent>
                      </Select>
                    </FunctionField>
                  </div>
                </div>
                <div className="grid gap-2">
                  <Button onClick={() => createFunction.mutate()} disabled={!runtimeEnabled || createFunction.isPending}>
                    <Plus className="h-4 w-4" /> {t("create")}
                  </Button>
                  <Button variant="outline" onClick={() => updateFunction.mutate()} disabled={!runtimeEnabled || !activeName || updateFunction.isPending}>
                    <Save className="h-4 w-4" /> {t("save")}
                  </Button>
                  <Button variant="outline" onClick={() => lintFunction.mutate()} disabled={!runtimeEnabled || lintFunction.isPending}>
                    <Code2 className="h-4 w-4" /> {t("lint")}
                  </Button>
                  <Button variant="outline" onClick={() => dryRunFunction.mutate()} disabled={!runtimeEnabled || dryRunFunction.isPending}>
                    <FlaskConical className="h-4 w-4" /> {t("dryRun")}
                  </Button>
                  <Button variant="outline" onClick={() => invokeFunction.mutate()} disabled={!runtimeEnabled || !activeName || invokeFunction.isPending}>
                    <Play className="h-4 w-4" /> {t("invoke")}
                  </Button>
                  <Button
                    variant="destructive"
                    onClick={() => {
                      if (window.confirm(t("confirmDeleteFunction", { name: activeName }))) {
                        deleteFunction.mutate();
                      }
                    }}
                    disabled={!runtimeEnabled || !activeName || deleteFunction.isPending}
                  >
                    <Trash2 className="h-4 w-4" /> {t("delete")}
                  </Button>
                </div>
              </div>
            </div>
          </Panel>
        </div>
      </div>

      <Panel title={t("testConsole")}>
        <div className="overflow-hidden rounded-lg border bg-background">
          <div className="grid gap-4 border-b bg-muted/20 p-4 xl:grid-cols-[240px_minmax(0,1fr)]">
            <div>
              <div className="mb-2 text-sm font-medium">{t("httpMethod")}</div>
              <Select value={requestMethod} onValueChange={setRequestMethod}>
                <SelectTrigger className="bg-background"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="GET">GET</SelectItem>
                  <SelectItem value="POST">POST</SelectItem>
                  <SelectItem value="PUT">PUT</SelectItem>
                  <SelectItem value="PATCH">PATCH</SelectItem>
                  <SelectItem value="DELETE">DELETE</SelectItem>
                </SelectContent>
              </Select>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">{t("httpMethodHelp")}</p>
            </div>
            <div>
              <div className="mb-2 text-sm font-medium">{t("runtimeRequest")}</div>
              <div className="grid gap-2 md:grid-cols-2 2xl:grid-cols-4">
                {["method", "query", "body", "input"].map((field) => (
                  <div key={field} className="rounded-md border bg-card px-3 py-2 font-mono text-xs text-muted-foreground">
                    ctx.request.{field}
                  </div>
                ))}
              </div>
            </div>
          </div>
          <div className="grid gap-0 xl:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)_minmax(0,1.15fr)]">
            <CodeWorkbenchPane title={t("queryParams")} description={t("queryParamsHelp")}>
              <CodeEditor value={queryJson} onChange={setQueryJson} minHeight={340} />
            </CodeWorkbenchPane>
            <CodeWorkbenchPane title={t("requestBody")} description={t("requestBodyHelp")}>
              <CodeEditor value={inputJson} onChange={setInputJson} minHeight={340} />
            </CodeWorkbenchPane>
            <CodeWorkbenchPane title={t("executionResult")} description={t("executionResultHelp")}>
              <JsonBlock value={output ?? { status: "idle" }} minHeight={340} />
            </CodeWorkbenchPane>
          </div>
        </div>
      </Panel>

      <Panel title={t("versions")}>
        <DataTableView
          loading={versions.isLoading}
          columns={[t("columnsVersion"), t("columnsRuntime"), t("columnsActive"), t("columnsAction")]}
          rows={(versions.data ?? []).map((version) => [
            version.version_number,
            version.runtime,
            version.is_active ? common("yes") : common("no"),
            version.is_active ? "" : (
              <Button
                key={version.id}
                variant="outline"
                size="sm"
                onClick={() => rollbackVersion.mutate(version.version_number)}
                disabled={!runtimeEnabled || rollbackVersion.isPending}
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
          loading={invocations.isLoading}
          columns={[t("columnsId"), t("columnsStatus"), t("columnsMode"), t("columnsDuration"), t("columnsRetry")]}
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
              disabled={!runtimeEnabled || retryInvocation.isPending}
            >
              <History className="h-4 w-4" /> {t("retry")}
            </Button>,
          ])}
          emptyTitle={runtimeEnabled ? t("emptyInvocationsTitle") : t("runtimeDisabledEmptyTitle")}
          emptyDescription={runtimeEnabled ? t("emptyInvocationsDescription") : t("runtimeDisabledEmptyDescription")}
        />
      </Panel>
    </Section>
  );
}

function FunctionField({
  label,
  help,
  children,
}: {
  label: string;
  help: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <span className="text-sm font-medium">{label}</span>
      {children}
      <span className="block text-xs leading-5 text-muted-foreground">{help}</span>
    </div>
  );
}

function CodeWorkbenchPane({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-w-0 border-t p-4 xl:border-l xl:border-t-0 first:xl:border-l-0">
      <div className="mb-3 min-h-14">
        <div className="text-sm font-medium">{title}</div>
        <div className="mt-1 text-xs leading-5 text-muted-foreground">{description}</div>
      </div>
      {children}
    </div>
  );
}

function CodeEditor({
  value,
  onChange,
  minHeight = 480,
  readOnly = false,
}: {
  value: string;
  onChange?: (value: string) => void;
  minHeight?: number;
  readOnly?: boolean;
}) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const gutterRef = useRef<HTMLDivElement | null>(null);
  const historyRef = useRef([{ value, selectionStart: 0, selectionEnd: 0 }]);
  const historyIndexRef = useRef(0);
  const [currentLine, setCurrentLine] = useState(1);
  const [scrollTop, setScrollTop] = useState(0);
  const lineHeight = 20;
  const lines = Math.max(1, value.split("\n").length);

  useEffect(() => {
    const currentSnapshot = historyRef.current[historyIndexRef.current];
    if (currentSnapshot && currentSnapshot.value !== value) {
      historyRef.current = [{ value, selectionStart: 0, selectionEnd: 0 }];
      historyIndexRef.current = 0;
    }
  }, [value]);

  const syncCursorLine = () => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    setCurrentLine(value.slice(0, textarea.selectionStart).split("\n").length);
  };

  const pushHistory = (nextValue: string, nextStart: number, nextEnd = nextStart) => {
    const current = historyRef.current[historyIndexRef.current];
    if (current?.value === nextValue && current.selectionStart === nextStart && current.selectionEnd === nextEnd) {
      return;
    }
    const nextHistory = historyRef.current.slice(0, historyIndexRef.current + 1);
    nextHistory.push({ value: nextValue, selectionStart: nextStart, selectionEnd: nextEnd });
    if (nextHistory.length > 100) {
      nextHistory.shift();
    }
    historyRef.current = nextHistory;
    historyIndexRef.current = nextHistory.length - 1;
  };

  const updateValueAndSelection = (nextValue: string, nextStart: number, nextEnd = nextStart) => {
    if (!onChange || readOnly) return;
    pushHistory(nextValue, nextStart, nextEnd);
    onChange(nextValue);
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.selectionStart = nextStart;
      textarea.selectionEnd = nextEnd;
      textarea.focus();
      setCurrentLine(nextValue.slice(0, nextStart).split("\n").length);
    });
  };

  const applyHistorySnapshot = (snapshot: { value: string; selectionStart: number; selectionEnd: number }) => {
    if (!onChange || readOnly) return;
    onChange(snapshot.value);
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.selectionStart = snapshot.selectionStart;
      textarea.selectionEnd = snapshot.selectionEnd;
      textarea.focus();
      setCurrentLine(snapshot.value.slice(0, snapshot.selectionStart).split("\n").length);
    });
  };

  const undo = () => {
    if (historyIndexRef.current <= 0) return;
    historyIndexRef.current -= 1;
    applyHistorySnapshot(historyRef.current[historyIndexRef.current]);
  };

  const redo = () => {
    if (historyIndexRef.current >= historyRef.current.length - 1) return;
    historyIndexRef.current += 1;
    applyHistorySnapshot(historyRef.current[historyIndexRef.current]);
  };

  const handleTab = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (readOnly) return;
    const isModifierPressed = event.metaKey || event.ctrlKey;
    const key = event.key.toLowerCase();
    if (isModifierPressed && key === "z") {
      event.preventDefault();
      if (event.shiftKey) {
        redo();
      } else {
        undo();
      }
      return;
    }
    if (isModifierPressed && key === "y") {
      event.preventDefault();
      redo();
      return;
    }
    if (event.key !== "Tab") return;
    event.preventDefault();

    const textarea = event.currentTarget;
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const lineStart = value.lastIndexOf("\n", start - 1) + 1;

    if (event.shiftKey) {
      const selectedBlock = value.slice(lineStart, end);
      let removedBeforeStart = 0;
      let removedTotal = 0;
      const unindented = selectedBlock.replace(/^( {1,2}|\t)/gm, (match, indent: string, offset: number) => {
        const removed = indent.length;
        removedTotal += removed;
        if (offset < start - lineStart) {
          removedBeforeStart += removed;
        }
        return "";
      });
      const nextValue = value.slice(0, lineStart) + unindented + value.slice(end);
      updateValueAndSelection(
        nextValue,
        Math.max(lineStart, start - removedBeforeStart),
        Math.max(lineStart, end - removedTotal),
      );
      return;
    }

    if (start !== end && value.slice(start, end).includes("\n")) {
      const selectedBlock = value.slice(lineStart, end);
      const indented = selectedBlock.replace(/^/gm, "  ");
      const addedLines = selectedBlock.split("\n").length;
      const nextValue = value.slice(0, lineStart) + indented + value.slice(end);
      updateValueAndSelection(nextValue, start + 2, end + addedLines * 2);
      return;
    }

    const nextValue = value.slice(0, start) + "  " + value.slice(end);
    updateValueAndSelection(nextValue, start + 2);
  };

  return (
    <div
      className="relative grid grid-cols-[3.5rem_minmax(0,1fr)] overflow-hidden rounded-lg border bg-background shadow-xs"
      style={{ minHeight }}
    >
      <div
        ref={gutterRef}
        className="select-none overflow-hidden border-r bg-muted/40 py-3 text-right font-mono text-xs leading-5 text-muted-foreground"
        aria-hidden="true"
      >
        {Array.from({ length: lines }, (_, index) => (
          <div
            key={index + 1}
            className={cn(
              "px-3 tabular-nums",
              currentLine === index + 1 && "font-semibold text-primary",
            )}
          >
            {index + 1}
          </div>
        ))}
      </div>
      <div className="relative min-w-0">
        <div
          className="pointer-events-none absolute left-0 right-0 bg-primary/10"
          style={{
            height: lineHeight,
            top: (currentLine - 1) * lineHeight + 12 - scrollTop,
          }}
        />
        <textarea
          ref={textareaRef}
          value={value}
          readOnly={readOnly}
          spellCheck={false}
          onChange={(event) => {
            if (!onChange || readOnly) return;
            const nextValue = event.target.value;
            const nextStart = event.target.selectionStart;
            const nextEnd = event.target.selectionEnd;
            pushHistory(nextValue, nextStart, nextEnd);
            onChange(nextValue);
            setCurrentLine(nextValue.slice(0, nextStart).split("\n").length);
          }}
          onKeyDown={handleTab}
          onClick={syncCursorLine}
          onKeyUp={syncCursorLine}
          onScroll={(event) => {
            const nextScrollTop = event.currentTarget.scrollTop;
            setScrollTop(nextScrollTop);
            if (gutterRef.current) {
              gutterRef.current.scrollTop = nextScrollTop;
            }
          }}
          className="relative z-10 w-full resize-y bg-transparent px-4 py-3 font-mono text-xs leading-5 text-foreground outline-none selection:bg-primary/20"
          style={{ minHeight }}
        />
      </div>
    </div>
  );
}

function PushView({ app }: { app: AppSummary }) {
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
    },
    onError: (error: Error) => toast.error(error.message),
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <Tabs defaultValue="subscriptions">
        <TabsList>
          <TabsTrigger value="subscriptions">{t("subscriptions")}</TabsTrigger>
          <TabsTrigger value="test">{t("testMessage")}</TabsTrigger>
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
      <div className="grid gap-3 md:grid-cols-4">
        <Metric label={t("queuedTotal")} value={summary?.total ?? items.length} icon={Bell} />
        <Metric label={t("queuedPending")} value={summary?.pending ?? 0} icon={History} />
        <Metric label={t("queuedFailed")} value={summary?.failed ?? 0} icon={CircleAlert} />
        <Metric label={t("queuedSent")} value={summary?.sent ?? 0} icon={CheckCircle2} />
      </div>
      <DataTableView
        columns={[t("columnsId"), t("columnsTitle"), t("columnsStatus"), t("columnsRetry"), t("columnsCreated")]}
        rows={items.map((item) => [
          item.id,
          item.title,
          item.status,
          item.retry_count,
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

function ActivityView({ app }: { app: AppSummary }) {
  const t = useTranslations("activityView");
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
    <Section title={t("title")} description={t("description")}>
      <ActivityList events={activity.data ?? []} loading={activity.isLoading} />
    </Section>
  );
}

function OpsView() {
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

type DiagnosticCheck = {
  name: string;
  ok: boolean;
  message?: string;
  severity?: string;
};

type DiagnosticGroup = {
  id: string;
  label: string;
  description: string;
  ok: boolean;
  severity?: string;
};

type PushQueueEntry = {
  id: number;
  user_id: string;
  title: string;
  body: string;
  status: string;
  retry_count: number;
  last_error?: string | null;
  partial_failure_count: number;
  next_retry_at?: string | null;
  created_at: string;
  processed_at?: string | null;
};

type PushQueueSummary = {
  total: number;
  pending: number;
  processing: number;
  sent: number;
  failed: number;
  partial_success: number;
  retry_scheduled: number;
  retry_overdue: number;
  ntfy_subscriptions: number;
  web_push_subscriptions: number;
};

type PushQueueResponse = {
  items: PushQueueEntry[];
  summary: PushQueueSummary;
};

function groupDiagnosticChecks(
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

function diagnosticBucket(name: string) {
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

function ActionCard({
  icon: Icon,
  title,
  description,
  action,
  onClick,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
  action: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group rounded-lg border bg-background p-4 text-left transition-colors hover:border-primary/60 hover:bg-muted/40"
    >
      <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-md bg-primary/10 text-primary">
        <Icon className="h-5 w-5" />
      </div>
      <div className="font-medium">{title}</div>
      <div className="mt-1 min-h-10 text-sm leading-5 text-muted-foreground">{description}</div>
      <div className="mt-3 text-sm font-medium text-primary">{action}</div>
    </button>
  );
}

function HealthRow({
  label,
  value,
  ok,
  muted,
}: {
  label: string;
  value: React.ReactNode;
  ok: boolean;
  muted?: boolean;
}) {
  const common = useTranslations("common");
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
      <div className="min-w-0">
        <div className="text-sm font-medium">{label}</div>
        <div className="truncate text-xs text-muted-foreground">{value}</div>
      </div>
      <Badge variant={ok ? (muted ? "secondary" : "default") : "destructive"}>
        {ok ? common("ok") : common("needsAttention")}
      </Badge>
    </div>
  );
}

function DataTableView({
  columns,
  rows,
  loading,
  emptyTitle,
  emptyDescription,
  emptyAction,
}: {
  columns: string[];
  rows: React.ReactNode[][];
  loading?: boolean;
  emptyTitle?: string;
  emptyDescription?: string;
  emptyAction?: React.ReactNode;
}) {
  const common = useTranslations("common");
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
              <TableCell colSpan={columns.length} className="h-36">
                <div className="flex flex-col items-center justify-center px-4 py-8 text-center">
                  <div className="font-medium">{emptyTitle ?? common("noRecords")}</div>
                  {emptyDescription && (
                    <div className="mt-1 max-w-md text-sm leading-5 text-muted-foreground">{emptyDescription}</div>
                  )}
                  {emptyAction && <div className="mt-4">{emptyAction}</div>}
                </div>
              </TableCell>
            </TableRow>
          ) : (
            rows.map((row, index) => (
              <TableRow key={index}>
                {row.map((cell, cellIndex) => (
                  <TableCell
                    key={cellIndex}
                    className={cn(
                      "max-w-[260px] text-xs",
                      typeof cell === "string" || typeof cell === "number"
                        ? "truncate font-mono"
                        : "font-sans",
                    )}
                  >
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

function activityActionLabel(action: string, t: ReturnType<typeof useTranslations>) {
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

function activityResourceLabel(resourceType: string | null | undefined, t: ReturnType<typeof useTranslations>) {
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

function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  compact,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
  action?: React.ReactNode;
  compact?: boolean;
}) {
  return (
    <div className={cn("flex flex-col items-center justify-center rounded-lg border border-dashed bg-card p-8 text-center", compact ? "min-h-40" : "min-h-64")}>
      <Icon className="mb-3 h-8 w-8 text-primary" />
      <h2 className="font-semibold">{title}</h2>
      <p className="mt-1 max-w-md text-sm text-muted-foreground">{description}</p>
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

function JsonBlock({ value, minHeight = 288 }: { value: unknown; minHeight?: number }) {
  return <CodeEditor value={JSON.stringify(value, null, 2)} minHeight={minHeight} readOnly />;
}

function browserSafeOrigin() {
  if (typeof window === "undefined") return "";
  return window.location.origin;
}

function parseJsonInput(value: string, invalidMessage: string) {
  try {
    return JSON.parse(value);
  } catch {
    throw new Error(invalidMessage);
  }
}

function parseFunctionTestPayload(value: string, invalidMessage: string) {
  const parsed = parseJsonInput(value, invalidMessage);
  if (isRecord(parsed) && Object.hasOwn(parsed, "input")) {
    return {
      requestBody: parsed,
      input: parsed.input,
    };
  }
  return {
    requestBody: { input: parsed },
    input: parsed,
  };
}

function safeBuildQueryString(value: string) {
  try {
    return buildQueryString(value, "");
  } catch {
    return "";
  }
}

function buildQueryString(value: string, invalidMessage: string) {
  const parsed = parseJsonInput(value, invalidMessage);
  if (!isRecord(parsed)) {
    if (invalidMessage) throw new Error(invalidMessage);
    return "";
  }
  return new URLSearchParams(
    Object.entries(parsed).flatMap(([key, entry]) => {
      if (entry === null || typeof entry === "undefined") return [];
      if (Array.isArray(entry)) {
        return entry.map((item) => [key, String(item)] as [string, string]);
      }
      return [[key, String(entry)] as [string, string]];
    }),
  ).toString();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function storageMimeTypes(bucket: StorageBucket) {
  if (Array.isArray(bucket.allowed_mime_types)) return bucket.allowed_mime_types;
  if (!bucket.allowed_mime_types_json) return [];
  try {
    const parsed = JSON.parse(bucket.allowed_mime_types_json);
    return Array.isArray(parsed) ? parsed.map(String) : [];
  } catch {
    return [];
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
  const common = useTranslations("common");
  return (
    <Badge variant={ok ? "default" : "secondary"} className="h-8 gap-1.5">
      {ok ? <CheckCircle2 className="h-3.5 w-3.5" /> : <CircleAlert className="h-3.5 w-3.5" />}
      {ok ? common("ready") : common("checking")}
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
