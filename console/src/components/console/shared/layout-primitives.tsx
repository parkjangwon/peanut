"use client";

import { CheckCircle2, CircleAlert } from "lucide-react";
import type React from "react";
import { useTranslations } from "next-intl";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export function Section({
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

export function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card className="rounded-lg">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">{title}</CardTitle>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}

export function Metric({
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

export function ActionCard({
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

export function HealthRow({
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
export function StatusBadge({ ok }: { ok: boolean }) {
  const common = useTranslations("common");
  return (
    <Badge variant={ok ? "default" : "secondary"} className="h-8 gap-1.5">
      {ok ? <CheckCircle2 className="h-3.5 w-3.5" /> : <CircleAlert className="h-3.5 w-3.5" />}
      {ok ? common("ready") : common("checking")}
    </Badge>
  );
}
