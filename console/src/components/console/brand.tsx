import { cn } from "@/lib/utils";

export function PeanutMark({ className }: { className?: string }) {
  return (
    <div
      aria-hidden="true"
      className={cn(
        "relative h-9 w-9 rounded-full bg-primary shadow-sm",
        "before:absolute before:left-[7px] before:top-[5px] before:h-5 before:w-4 before:-rotate-12 before:rounded-[50%] before:bg-primary-foreground/88",
        "after:absolute after:right-[7px] after:bottom-[5px] after:h-5 after:w-4 after:rotate-12 after:rounded-[50%] after:bg-primary-foreground/82",
        className,
      )}
    />
  );
}

export function PeanutLogo() {
  return (
    <div className="flex items-center gap-3">
      <PeanutMark />
      <div className="leading-tight">
        <div className="text-base font-semibold">Peanut</div>
        <div className="text-xs text-muted-foreground">Admin Console</div>
      </div>
    </div>
  );
}
