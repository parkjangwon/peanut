"use client";

import { Info } from "lucide-react";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import {
  apiFetch,
  AppSummary,
  FunctionDetail,
  FunctionInvocation,
  FunctionSummary,
  FunctionVersionSummary,
  OpsMetrics,
} from "@/lib/api";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

import { Section } from "../shared/layout-primitives";
import {
  buildQueryString,
  parseFunctionTestPayload,
  parseJsonInput,
  safeBuildQueryString,
} from "../utils/json";
import { FunctionHistoryPanels } from "./functions/function-history-panels";
import { FunctionListPanel } from "./functions/function-list-panel";
import { FunctionTestConsole } from "./functions/function-test-console";
import { FunctionWorkspacePanels } from "./functions/function-workspace-panels";

export function FunctionsView({ app }: { app: AppSummary }) {
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
        <FunctionListPanel
          runtimeEnabled={runtimeEnabled}
          functions={functions.data}
          functionsLoading={functions.isLoading}
          activeName={activeName}
          detail={detail.data}
          onSelectFunction={setSelectedName}
          onLoadDraft={loadFunctionDraft}
          onApplyCachedDraft={() => {
            if (activeName && detail.data) applyFunctionDraft(detail.data);
          }}
        />
        <FunctionWorkspacePanels
          runtimeEnabled={runtimeEnabled}
          activeName={activeName}
          enabled={enabled}
          endpointUrl={endpointUrl}
          name={name}
          setName={setName}
          displayName={displayName}
          setDisplayName={setDisplayName}
          endpointSlug={endpointSlug}
          setEndpointSlug={setEndpointSlug}
          runtime={runtime}
          setRuntime={setRuntime}
          invokePolicy={invokePolicy}
          setInvokePolicy={setInvokePolicy}
          timeoutMs={timeoutMs}
          setTimeoutMs={setTimeoutMs}
          setEnabled={setEnabled}
          sourceCode={sourceCode}
          setSourceCode={setSourceCode}
          onCreate={() => createFunction.mutate()}
          onUpdate={() => updateFunction.mutate()}
          onLint={() => lintFunction.mutate()}
          onDryRun={() => dryRunFunction.mutate()}
          onInvoke={() => invokeFunction.mutate()}
          onDelete={() => {
            if (window.confirm(t("confirmDeleteFunction", { name: activeName }))) {
              deleteFunction.mutate();
            }
          }}
          createPending={createFunction.isPending}
          updatePending={updateFunction.isPending}
          lintPending={lintFunction.isPending}
          dryRunPending={dryRunFunction.isPending}
          invokePending={invokeFunction.isPending}
          deletePending={deleteFunction.isPending}
        />
      </div>

      <FunctionTestConsole
        requestMethod={requestMethod}
        setRequestMethod={setRequestMethod}
        queryJson={queryJson}
        setQueryJson={setQueryJson}
        inputJson={inputJson}
        setInputJson={setInputJson}
        output={output}
      />

      <FunctionHistoryPanels
        runtimeEnabled={runtimeEnabled}
        versions={versions.data}
        versionsLoading={versions.isLoading}
        invocations={invocations.data}
        invocationsLoading={invocations.isLoading}
        onRollback={(versionNumber) => rollbackVersion.mutate(versionNumber)}
        onRetry={(invocationId) => retryInvocation.mutate(invocationId)}
        rollbackPending={rollbackVersion.isPending}
        retryPending={retryInvocation.isPending}
      />
    </Section>
  );
}