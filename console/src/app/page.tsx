import { ConsoleApp } from "@/components/console/console-app";
import { ConsoleErrorBoundary } from "@/components/console/error-boundary";
import { ConsoleProviders } from "@/components/console/providers";

export default function Home() {
  return (
    <ConsoleProviders>
      <ConsoleErrorBoundary>
        <ConsoleApp />
      </ConsoleErrorBoundary>
    </ConsoleProviders>
  );
}
