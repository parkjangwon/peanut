"use client";

import { LogOut, Menu } from "lucide-react";
import { useTranslations } from "next-intl";

import { PeanutLogo } from "@/components/console/brand";
import type { AppSummary, PeanutUser } from "@/lib/api";
import { useConsoleLocale } from "@/i18n/provider";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Sheet, SheetContent, SheetTrigger } from "@/components/ui/sheet";

import { ConsoleNav } from "./console-nav";
import { LocaleSelect } from "../shared/locale-select";
import type { View } from "../types";
import { displayProjectName } from "../utils/display";

export function ConsoleHeader({
  apps,
  selectedAppId,
  user,
  view,
  onAppChange,
  onViewChange,
  onLogout,
}: {
  apps: AppSummary[];
  selectedAppId: string;
  user: PeanutUser;
  view: View;
  onAppChange: (appId: string) => void;
  onViewChange: (view: View) => void;
  onLogout: () => void;
}) {
  const t = useTranslations("common");
  const { locale, setLocale } = useConsoleLocale();
  return (
    <header className="sticky top-0 z-20 border-b bg-background/92 backdrop-blur">
      <div className="flex h-16 items-center justify-between gap-3 px-4 sm:px-6 lg:px-8">
        <div className="flex min-w-0 items-center gap-3">
          <Sheet>
            <SheetTrigger asChild>
              <Button size="icon" variant="ghost" className="lg:hidden" aria-label={t("openNavigation")}>
                <Menu className="h-5 w-5" />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-72">
              <PeanutLogo />
              <Separator className="my-4" />
              <ConsoleNav view={view} onChange={onViewChange} />
            </SheetContent>
          </Sheet>
          <Select value={selectedAppId} onValueChange={onAppChange}>
            <SelectTrigger className="w-[230px] max-w-[58vw] bg-card" aria-label={t("selectApp")}>
              <SelectValue placeholder={t("selectApp")} />
            </SelectTrigger>
            <SelectContent>
              {apps.map((app) => (
                <SelectItem key={app.id} value={app.id}>
                  {displayProjectName(app)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-3">
          <LocaleSelect
            locale={locale}
            onChange={setLocale}
            label={t("language")}
            english={t("english")}
            korean={t("korean")}
            className="h-9 bg-card"
          />
          <div className="hidden text-right text-sm sm:block">
            <div className="font-medium">{user.email}</div>
            <div className="text-xs text-muted-foreground">{t("platformRole", { role: user.admin_role })}</div>
          </div>
          <Button size="icon" variant="outline" onClick={onLogout} aria-label={t("signOut")}>
            <LogOut className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </header>
  );
}
