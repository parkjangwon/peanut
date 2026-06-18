"use client";

import { Pencil, Plus, Save, Trash2 } from "lucide-react";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { apiFetch, AppSummary } from "@/lib/api";
import { Button } from "@/components/ui/button";
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

import { DataTableView } from "../shared/data-table-view";
import { Section } from "../shared/layout-primitives";
import { displayProjectName } from "../utils/display";

export function AppsView({ apps }: { apps: AppSummary[] }) {
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
