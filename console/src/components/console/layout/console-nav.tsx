"use client";

import { useTranslations } from "next-intl";

import { cn } from "@/lib/utils";

import { navItems, pathForView } from "../nav-config";
import type { View } from "../types";

export function ConsoleNav({ view, onChange }: { view: View; onChange: (view: View) => void }) {
  const t = useTranslations("nav");
  return (
    <nav className="space-y-1">
      {navItems.map((item) => (
        <button
          key={item.view}
          onClick={() => {
            onChange(item.view);
            window.history.replaceState(null, "", pathForView(item.view));
          }}
          className={cn(
            "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm text-sidebar-foreground transition-colors hover:bg-sidebar-accent",
            view === item.view && "bg-sidebar-accent font-medium text-sidebar-accent-foreground",
          )}
        >
          <item.icon className="h-4 w-4" />
          {t(item.labelKey)}
        </button>
      ))}
    </nav>
  );
}
