"use client";

import {
  Archive,
  Bell,
  Cloud,
  Code2,
  Database,
  ShieldCheck,
  Users,
  Wrench,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslations } from "next-intl";
import { toast } from "sonner";

import { PeanutLogo, PeanutMark } from "@/components/console/brand";
import { bootstrapAdmin, loginAdmin, storeSession } from "@/lib/api";
import { useConsoleLocale } from "@/i18n/provider";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import { FeatureSignal, Signal } from "../shared/auth-marketing";
import { LocaleSelect } from "../shared/locale-select";

export function AuthScreen({
  bootstrapping,
  onModeChange,
  onAuthenticated,
}: {
  bootstrapping: boolean;
  onModeChange: (value: boolean) => void;
  onAuthenticated: (session: Awaited<ReturnType<typeof loginAdmin>>) => void;
}) {
  const t = useTranslations("auth");
  const common = useTranslations("common");
  const { locale, setLocale } = useConsoleLocale();
  const [email, setEmail] = useState("admin@peanut.local");
  const [password, setPassword] = useState("");
  const [rememberMe, setRememberMe] = useState(true);
  const initializedAuthLocale = useRef(false);

  useEffect(() => {
    if (initializedAuthLocale.current) return;
    initializedAuthLocale.current = true;
    if (locale !== "en") {
      setLocale("en");
    }
  }, [locale, setLocale]);

  const mutation = useMutation({
    mutationFn: () =>
      bootstrapping ? bootstrapAdmin(email, password) : loginAdmin(email, password),
    onSuccess: (session) => {
      toast.success(bootstrapping ? t("adminCreated") : t("signedIn"));
      storeSession(session, rememberMe);
      onAuthenticated(session);
    },
    onError: (error: Error & { status?: number }) => {
      if (!bootstrapping && error.status === 401) {
        toast.error(t("invalidCredentials"));
      } else if (bootstrapping && error.status === 409) {
        toast.error(t("adminExists"));
        onModeChange(false);
      } else {
        toast.error(error.message);
      }
    },
  });

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top_left,color-mix(in_oklch,var(--primary),white_70%),transparent_38%),linear-gradient(180deg,var(--background),white)]">
      <div className="mx-auto flex min-h-screen w-full max-w-[1360px] flex-col px-5 py-8">
        <div className="mb-8 flex items-center justify-between gap-4">
          <PeanutLogo />
          <LocaleSelect
            locale={locale}
            onChange={setLocale}
            label={common("language")}
            english={common("english")}
            korean={common("korean")}
            className="bg-card"
          />
        </div>
        <div className="grid flex-1 grid-cols-1 items-center gap-10 xl:grid-cols-[minmax(0,1.4fr)_minmax(440px,0.6fr)]">
          <section className="space-y-7">
            <div className="max-w-3xl space-y-5">
              <Badge className="bg-primary/10 text-primary hover:bg-primary/10">
                {t("badge")}
              </Badge>
              <h1 className="max-w-3xl whitespace-pre-line text-balance text-4xl font-semibold tracking-tight text-foreground sm:text-5xl xl:text-6xl">
                {t("headline")}
              </h1>
              <p className="max-w-3xl whitespace-pre-line text-pretty text-lg leading-8 text-muted-foreground">
                {t("description")}
              </p>
            </div>
            <div className="grid max-w-3xl grid-cols-1 gap-3 sm:grid-cols-3">
              <Signal icon={ShieldCheck} label={t("signalIsolation")} />
              <Signal icon={Database} label={t("signalData")} />
              <Signal icon={Cloud} label={t("signalOps")} />
            </div>
            <div className="grid max-w-[900px] grid-cols-1 gap-3 sm:grid-cols-2">
              <FeatureSignal icon={Users} title={t("featureAuth")} description={t("featureAuthDescription")} />
              <FeatureSignal icon={Database} title={t("featureData")} description={t("featureDataDescription")} />
              <FeatureSignal icon={Archive} title={t("featureStorage")} description={t("featureStorageDescription")} />
              <FeatureSignal icon={Code2} title={t("featureFunctions")} description={t("featureFunctionsDescription")} />
              <FeatureSignal icon={Bell} title={t("featurePush")} description={t("featurePushDescription")} />
              <FeatureSignal icon={Wrench} title={t("featureOps")} description={t("featureOpsDescription")} />
            </div>
          </section>
          <section className="w-full max-w-[520px] justify-self-center rounded-lg border bg-card p-6 shadow-sm xl:justify-self-end">
          <div className="mb-6 flex items-center justify-between gap-4">
            <div>
              <h2 className="text-xl font-semibold">
                {bootstrapping ? t("createFirstAdmin") : t("adminSignIn")}
              </h2>
              <p className="text-sm text-muted-foreground">
                {bootstrapping
                  ? t("freshInstallHelp")
                  : t("signInHelp")}
              </p>
            </div>
            <PeanutMark />
          </div>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              mutation.mutate();
            }}
          >
            <Input
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder={t("emailPlaceholder")}
              autoComplete="email"
            />
            <Input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder={t("passwordPlaceholder")}
              autoComplete={bootstrapping ? "new-password" : "current-password"}
            />
            {!bootstrapping ? (
              <label className="flex cursor-pointer items-center justify-between gap-3 rounded-md border bg-muted/20 px-3 py-2 text-sm">
                <span>
                  <span className="block font-medium">{t("rememberMe")}</span>
                  <span className="block text-xs text-muted-foreground">{t("rememberMeHelp")}</span>
                </span>
                <input
                  type="checkbox"
                  checked={rememberMe}
                  onChange={(event) => setRememberMe(event.target.checked)}
                  className="h-4 w-4 accent-primary"
                />
              </label>
            ) : null}
            <Button className="w-full" disabled={mutation.isPending}>
              {mutation.isPending ? t("working") : bootstrapping ? t("createAdmin") : t("signIn")}
            </Button>
          </form>
          <Button
            type="button"
            variant="ghost"
            className="mt-4 w-full"
            onClick={() => onModeChange(!bootstrapping)}
          >
            {bootstrapping ? t("alreadyHaveAdmin") : t("setUpFresh")}
          </Button>
          </section>
        </div>
      </div>
    </div>
  );
}
