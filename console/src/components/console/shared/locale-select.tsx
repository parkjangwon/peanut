"use client";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { ConsoleLocale } from "@/i18n/provider";

export function LocaleSelect({
  locale,
  onChange,
  label,
  english,
  korean,
  className,
}: {
  locale: ConsoleLocale;
  onChange: (locale: ConsoleLocale) => void;
  label: string;
  english: string;
  korean: string;
  className?: string;
}) {
  return (
    <Select value={locale} onValueChange={(value) => onChange(parseLocale(value))}>
      <SelectTrigger className={cn("w-[128px]", className)} aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="en">{english}</SelectItem>
        <SelectItem value="ko">{korean}</SelectItem>
      </SelectContent>
    </Select>
  );
}

export function parseLocale(value: string): ConsoleLocale {
  return value === "ko" ? "ko" : "en";
}