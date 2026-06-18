"use client";

import {
  Code2,
  Copy,
  FlaskConical,
  Play,
  Plus,
  Save,
  Trash2,
} from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

import { CodeEditor, FunctionField } from "../../shared/code-editor";
import { Panel } from "../../shared/layout-primitives";
import { copyTextToClipboard, normalizeCopyError } from "../../utils/clipboard";

export function FunctionWorkspacePanels({
  runtimeEnabled,
  activeName,
  enabled,
  endpointUrl,
  name,
  setName,
  displayName,
  setDisplayName,
  endpointSlug,
  setEndpointSlug,
  runtime,
  setRuntime,
  invokePolicy,
  setInvokePolicy,
  timeoutMs,
  setTimeoutMs,
  setEnabled,
  sourceCode,
  setSourceCode,
  onCreate,
  onUpdate,
  onLint,
  onDryRun,
  onInvoke,
  onDelete,
  createPending,
  updatePending,
  lintPending,
  dryRunPending,
  invokePending,
  deletePending,
}: {
  runtimeEnabled: boolean;
  activeName: string;
  enabled: string;
  endpointUrl: string;
  name: string;
  setName: Dispatch<SetStateAction<string>>;
  displayName: string;
  setDisplayName: Dispatch<SetStateAction<string>>;
  endpointSlug: string;
  setEndpointSlug: Dispatch<SetStateAction<string>>;
  runtime: string;
  setRuntime: Dispatch<SetStateAction<string>>;
  invokePolicy: string;
  setInvokePolicy: Dispatch<SetStateAction<string>>;
  timeoutMs: string;
  setTimeoutMs: Dispatch<SetStateAction<string>>;
  setEnabled: Dispatch<SetStateAction<string>>;
  sourceCode: string;
  setSourceCode: Dispatch<SetStateAction<string>>;
  onCreate: () => void;
  onUpdate: () => void;
  onLint: () => void;
  onDryRun: () => void;
  onInvoke: () => void;
  onDelete: () => void;
  createPending: boolean;
  updatePending: boolean;
  lintPending: boolean;
  dryRunPending: boolean;
  invokePending: boolean;
  deletePending: boolean;
}) {
  const t = useTranslations("functionsView");
  const common = useTranslations("common");

  return (
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
              <Button onClick={onCreate} disabled={!runtimeEnabled || createPending}>
                <Plus className="h-4 w-4" /> {t("create")}
              </Button>
              <Button variant="outline" onClick={onUpdate} disabled={!runtimeEnabled || !activeName || updatePending}>
                <Save className="h-4 w-4" /> {t("save")}
              </Button>
              <Button variant="outline" onClick={onLint} disabled={!runtimeEnabled || lintPending}>
                <Code2 className="h-4 w-4" /> {t("lint")}
              </Button>
              <Button variant="outline" onClick={onDryRun} disabled={!runtimeEnabled || dryRunPending}>
                <FlaskConical className="h-4 w-4" /> {t("dryRun")}
              </Button>
              <Button variant="outline" onClick={onInvoke} disabled={!runtimeEnabled || !activeName || invokePending}>
                <Play className="h-4 w-4" /> {t("invoke")}
              </Button>
              <Button
                variant="destructive"
                onClick={onDelete}
                disabled={!runtimeEnabled || !activeName || deletePending}
              >
                <Trash2 className="h-4 w-4" /> {t("delete")}
              </Button>
            </div>
          </div>
        </div>
      </Panel>
    </div>
  );
}