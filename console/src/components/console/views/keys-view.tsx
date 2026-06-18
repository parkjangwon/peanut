"use client";

import { Copy, RotateCcw, Trash2 } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { apiFetch, AppSummary } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

import { DataTableView } from "../shared/data-table-view";
import { Panel, Section } from "../shared/layout-primitives";
import { copyTextToClipboard, normalizeCopyError } from "../utils/clipboard";

export function KeysView({ app }: { app: AppSummary }) {
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
