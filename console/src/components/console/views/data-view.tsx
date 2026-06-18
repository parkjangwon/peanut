"use client";

import { Download, Pencil, Save, Trash2 } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { apiFetch, AppSummary, DataTable } from "@/lib/api";
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

import { DataTableView } from "../shared/data-table-view";
import { Panel, Section } from "../shared/layout-primitives";
import { parseJsonInput } from "../utils/json";

export function DataView({ app }: { app: AppSummary }) {
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
  return (
    <Section title={t("title")} description={t("description")}>
      <Panel title={t("createTable")}>
        <div className="flex gap-3">
          <Input value={name} onChange={(event) => setName(event.target.value)} />
          <Button onClick={() => createTable.mutate()} disabled={createTable.isPending}>{common("create")}</Button>
        </div>
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
