import {
  Activity,
  Archive,
  Bell,
  Boxes,
  Code2,
  Database,
  KeyRound,
  LayoutDashboard,
  Users,
  Wrench,
} from "lucide-react";
import type React from "react";

import type { View } from "./types";

export const navItems: Array<{ view: View; labelKey: string; icon: React.ComponentType<{ className?: string }> }> = [
  { view: "overview", labelKey: "overview", icon: LayoutDashboard },
  { view: "apps", labelKey: "apps", icon: Boxes },
  { view: "keys", labelKey: "keys", icon: KeyRound },
  { view: "auth", labelKey: "auth", icon: Users },
  { view: "data", labelKey: "data", icon: Database },
  { view: "storage", labelKey: "storage", icon: Archive },
  { view: "functions", labelKey: "functions", icon: Code2 },
  { view: "push", labelKey: "push", icon: Bell },
  { view: "activity", labelKey: "activity", icon: Activity },
  { view: "ops", labelKey: "ops", icon: Wrench },
];

export function viewFromPath(pathname: string): View {
  const segment = pathname.split("/").filter(Boolean).at(-1);
  if (segment && navItems.some((item) => item.view === segment)) return segment as View;
  return "overview";
}

export function pathForView(view: View) {
  return view === "overview" ? "/" : `/${view}`;
}