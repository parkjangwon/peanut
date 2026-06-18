"use client";

import type { Dispatch, SetStateAction } from "react";
import { useTranslations } from "next-intl";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

import { CodeEditor, CodeWorkbenchPane, JsonBlock } from "../../shared/code-editor";
import { Panel } from "../../shared/layout-primitives";

export function FunctionTestConsole({
  requestMethod,
  setRequestMethod,
  queryJson,
  setQueryJson,
  inputJson,
  setInputJson,
  output,
}: {
  requestMethod: string;
  setRequestMethod: Dispatch<SetStateAction<string>>;
  queryJson: string;
  setQueryJson: Dispatch<SetStateAction<string>>;
  inputJson: string;
  setInputJson: Dispatch<SetStateAction<string>>;
  output: Record<string, unknown> | null;
}) {
  const t = useTranslations("functionsView");

  return (
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
  );
}