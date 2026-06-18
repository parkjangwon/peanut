"use client";

import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
  apiFetch,
  AppSummary,
  clearSession,
  logoutAdmin,
  PeanutUser,
  refreshAdminSession,
  storeSession,
  storedUser,
} from "@/lib/api";
import { Separator } from "@/components/ui/separator";

import { PeanutLogo } from "@/components/console/brand";
import { AuthScreen } from "./layout/auth-screen";
import { ConsoleHeader } from "./layout/console-header";
import { ConsoleNav } from "./layout/console-nav";
import { ViewContent } from "./layout/view-content";
import { viewFromPath } from "./nav-config";
import type { View } from "./types";

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
