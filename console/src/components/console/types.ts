export type View =
  | "overview"
  | "apps"
  | "keys"
  | "auth"
  | "data"
  | "storage"
  | "functions"
  | "push"
  | "activity"
  | "ops";

export type DiagnosticCheck = {
  name: string;
  ok: boolean;
  message?: string;
  severity?: string;
};

export type DiagnosticGroup = {
  id: string;
  label: string;
  description: string;
  ok: boolean;
  severity?: string;
};

export type PushQueueEntry = {
  id: number;
  user_id: string;
  title: string;
  body: string;
  status: string;
  retry_count: number;
  last_error?: string | null;
  partial_failure_count: number;
  failed_destinations?: Array<{ endpoint: string; error: string }>;
  next_retry_at?: string | null;
  created_at: string;
  processed_at?: string | null;
};

export type PushQueueStatsResponse = {
  window_hours: number;
  limit: number;
  retry_scheduled: number;
  retry_overdue: number;
  terminal_failure_reasons: Array<{ reason: string; count: number }>;
  destination_failure_reasons: Array<{ reason: string; count: number }>;
};

export type PushQueueSummary = {
  total: number;
  pending: number;
  processing: number;
  sent: number;
  failed: number;
  partial_success: number;
  retry_scheduled: number;
  retry_overdue: number;
  ntfy_subscriptions: number;
  web_push_subscriptions: number;
};

export type PushQueueResponse = {
  items: PushQueueEntry[];
  summary: PushQueueSummary;
};
