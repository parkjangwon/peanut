import { ConsoleApp } from "@/components/console/console-app";
import { ConsoleProviders } from "@/components/console/providers";

export default function Home() {
  return (
    <ConsoleProviders>
      <ConsoleApp />
    </ConsoleProviders>
  );
}
