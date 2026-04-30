use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscriptionsResponse {
    pub subscriptions: Vec<PushSubscription>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueSummary {
    pub total: i64,
    pub pending: i64,
    pub processing: i64,
    pub sent: i64,
    pub failed: i64,
    pub partial_success: i64,
    pub retry_scheduled: i64,
    pub retry_overdue: i64,
    pub ntfy_subscriptions: i64,
    pub web_push_subscriptions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueResponse {
    pub items: Vec<PushQueueEntry>,
    pub summary: PushQueueSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushReasonStat {
    pub reason: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueStatsResponse {
    pub window_hours: i64,
    pub limit: usize,
    pub retry_scheduled: i64,
    pub retry_overdue: i64,
    pub terminal_failure_reasons: Vec<PushReasonStat>,
    pub destination_failure_reasons: Vec<PushReasonStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VapidPublicKeyResponse {
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PushSubscription {
    pub id: i64,
    pub kind: String,
    pub topic: Option<String>,
    pub endpoint: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushDeliveryFailure {
    pub endpoint: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushQueueEntry {
    pub id: i64,
    pub user_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub partial_failure_count: i64,
    pub failed_destinations: Vec<PushDeliveryFailure>,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub processed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushQueueStatsParams {
    pub window_hours: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebPushSubscriptionKeysRequest {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CreateSubscriptionRequest {
    Ntfy {
        topic: String,
    },
    WebPush {
        endpoint: String,
        keys: WebPushSubscriptionKeysRequest,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnqueuePushRequest {
    pub title: String,
    pub body: String,
    pub user_id: Option<String>,
}
