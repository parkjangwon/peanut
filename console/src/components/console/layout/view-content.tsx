"use client";

import { Boxes } from "lucide-react";
import dynamic from "next/dynamic";
import { useTranslations } from "next-intl";

import type { AppSummary } from "@/lib/api";
import { Skeleton } from "@/components/ui/skeleton";

import { EmptyState } from "../shared/empty-state";
import type { View } from "../types";

function ViewLoading() {
  return <Skeleton className="h-80 w-full" />;
}

const OverviewView = dynamic(
  () => import("../views/overview-view").then((mod) => mod.OverviewView),
  { loading: ViewLoading },
);
const AppsView = dynamic(
  () => import("../views/apps-view").then((mod) => mod.AppsView),
  { loading: ViewLoading },
);
const KeysView = dynamic(
  () => import("../views/keys-view").then((mod) => mod.KeysView),
  { loading: ViewLoading },
);
const AuthView = dynamic(
  () => import("../views/auth-view").then((mod) => mod.AuthView),
  { loading: ViewLoading },
);
const DataView = dynamic(
  () => import("../views/data-view").then((mod) => mod.DataView),
  { loading: ViewLoading },
);
const StorageView = dynamic(
  () => import("../views/storage-view").then((mod) => mod.StorageView),
  { loading: ViewLoading },
);
const FunctionsView = dynamic(
  () => import("../views/functions-view").then((mod) => mod.FunctionsView),
  { loading: ViewLoading },
);
const PushView = dynamic(
  () => import("../views/push-view").then((mod) => mod.PushView),
  { loading: ViewLoading },
);
const ActivityView = dynamic(
  () => import("../views/activity-view").then((mod) => mod.ActivityView),
  { loading: ViewLoading },
);
const OpsView = dynamic(
  () => import("../views/ops-view").then((mod) => mod.OpsView),
  { loading: ViewLoading },
);

export function ViewContent({
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