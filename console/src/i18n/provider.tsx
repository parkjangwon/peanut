"use client";

import { NextIntlClientProvider } from "next-intl";
import type React from "react";
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

import en from "./messages/en.json";
import ko from "./messages/ko.json";

export type ConsoleLocale = "en" | "ko";

const LOCALE_STORAGE_KEY = "peanut.locale";

const messages: Record<ConsoleLocale, typeof en> = { en, ko };

const LocaleContext = createContext<{
  locale: ConsoleLocale;
  setLocale: (locale: ConsoleLocale) => void;
}>({
  locale: "en",
  setLocale: () => undefined,
});

function storedLocale(): ConsoleLocale {
  const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY);
  if (stored === "en" || stored === "ko") return stored;
  return "en";
}

export function ConsoleI18nProvider({ children }: { children: React.ReactNode }) {
  const [locale, setLocaleState] = useState<ConsoleLocale>("en");

  const setLocale = useCallback((nextLocale: ConsoleLocale) => {
    setLocaleState(nextLocale);
    window.localStorage.setItem(LOCALE_STORAGE_KEY, nextLocale);
    document.documentElement.lang = nextLocale;
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => setLocaleState(storedLocale()), 0);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  const contextValue = useMemo(() => ({ locale, setLocale }), [locale, setLocale]);

  return (
    <LocaleContext.Provider value={contextValue}>
      <NextIntlClientProvider
        locale={locale}
        messages={messages[locale]}
        now={new Date(0)}
        timeZone="UTC"
      >
        {children}
      </NextIntlClientProvider>
    </LocaleContext.Provider>
  );
}

export function useConsoleLocale() {
  return useContext(LocaleContext);
}
