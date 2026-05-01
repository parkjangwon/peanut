"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { ConsoleI18nProvider } from "@/i18n/provider";

export function ConsoleProviders({ children }: { children: React.ReactNode }) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 20_000,
            retry: 1,
          },
        },
      }),
  );

  return (
    <ConsoleI18nProvider>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </ConsoleI18nProvider>
  );
}
