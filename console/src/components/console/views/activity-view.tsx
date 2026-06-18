"use client";

import { useQuery } from "@tanstack/react-query";
import { useTranslations } from "next-intl";

import { ActivityEvent, apiFetch, AppSummary } from "@/lib/api";

import { ActivityList } from "../shared/activity-list";
import { Section } from "../shared/layout-primitives";

export function ActivityView({ app }: { app: AppSummary }) {
  const t = useTranslations("activityView");
  const activity = useQuery({
    queryKey: ["activity", app.id],
    queryFn: async () => {
      const response = await apiFetch<{ events?: ActivityEvent[]; activity?: ActivityEvent[] }>(
        `/api/apps/${app.id}/activity`,
      );
      return response.events ?? response.activity ?? [];
    },
  });
  return (
    <Section title={t("title")} description={t("description")}>
      <ActivityList events={activity.data ?? []} loading={activity.isLoading} />
    </Section>
  );
}
