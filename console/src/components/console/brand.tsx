import Image from "next/image";
import { useTranslations } from "next-intl";

import { cn } from "@/lib/utils";

export function PeanutMark({ className }: { className?: string }) {
  return (
    <Image
      aria-hidden="true"
      src="/peanut-logo.png"
      alt=""
      width={36}
      height={36}
      className={cn(
        "h-9 w-9 object-contain drop-shadow-sm",
        className,
      )}
    />
  );
}

export function PeanutLogo() {
  const t = useTranslations("common");
  return (
    <div className="flex items-center gap-3">
      <PeanutMark />
      <div className="leading-tight">
        <div className="text-base font-semibold">Peanut</div>
        <div className="text-xs text-muted-foreground">{t("adminConsole")}</div>
      </div>
    </div>
  );
}
