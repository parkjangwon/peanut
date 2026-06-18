"use client";

import type React from "react";

export function FeatureSignal({
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

export function Signal({
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