# Push delivery

Peanut push combines **ntfy topics** and **Web Push (VAPID)** behind one queue and worker.

## Concepts

| Piece | Role |
| --- | --- |
| Subscription | Per-user delivery target (ntfy topic or browser endpoint) |
| Queue item | Durable message with retry, scheduling, and idempotency |
| Worker | Claims pending items, fans out to subscriptions, records partial failures |
| Webhook | Optional per-app callback when a message reaches a terminal state |

## Enqueue a message

`POST /api/apps/{app_id}/push/messages`

```json
{
  "title": "Order shipped",
  "body": "Package #42 is on the way",
  "user_id": "user_abc",
  "payload": {
    "url": "https://app.example/orders/42",
    "icon": "https://cdn.example/icon.png",
    "data": { "order_id": "42" },
    "priority": "high"
  },
  "scheduled_at": "2026-06-19T12:00:00Z",
  "idempotency_key": "order-42-shipped"
}
```

Response:

```json
{ "id": 17, "status": "pending" }
```

### Broadcast by tag

Set `broadcast_tag` instead of `user_id` to fan out to every ntfy subscription whose endpoint equals the tag. `broadcast_tag` and `user_id` are mutually exclusive.

### Batch enqueue

`POST /api/apps/{app_id}/push/messages/batch`

```json
{
  "messages": [
    { "title": "A", "body": "one" },
    { "title": "B", "body": "two", "user_id": "user_xyz" }
  ]
}
```

### Message status

`GET /api/apps/{app_id}/push/messages/{message_id}`

Returns queue state, retry metadata, and per-endpoint failure details.

## Rich payload delivery

`payload` is stored as JSON and applied at delivery time:

- **ntfy**: `Click`, `Icon`, `Priority` headers; optional JSON body when `data` or `badge` is present
- **Web Push**: encrypted JSON payload with `title`, `body`, `url`, `icon`, `badge`, `data`

## Subscriptions

### ntfy

```json
POST /push/subscriptions
{ "topic": "alerts_main" }
```

Configure the worker with:

- `NTFY_BASE_URL` (default `https://ntfy.sh`)
- `NTFY_AUTH_TOKEN` (optional bearer token)

Dead topics (`404` / `410`) are removed automatically.

### Web Push

```json
POST /push/subscriptions
{
  "endpoint": "https://fcm.googleapis.com/...",
  "keys": { "p256dh": "...", "auth": "..." }
}
```

Required env:

- `WEB_PUSH_VAPID_PRIVATE_KEY`
- `WEB_PUSH_VAPID_SUBJECT` (`mailto:` or `https://`)

Fetch the public key from `GET /push/vapid-public-key`.

## Delivery webhook

Per app, set:

- `apps.push_webhook_url`
- `apps.webhook_secret` (optional HMAC signer)

Terminal events (`sent`, `failed`) POST:

```json
{
  "event": "push.delivery",
  "app_id": "app_default",
  "message_id": 17,
  "status": "sent",
  "error": null
}
```

When `webhook_secret` is configured, Peanut sends `X-Peanut-Signature: sha256=<hex>` over the raw JSON body.

## JS SDK

```ts
import { PeanutClient } from "@peanut/sdk";

const peanut = new PeanutClient({ baseUrl, appId, apiKey });
await peanut.auth.login(email, password);

const { id } = await peanut.push.enqueueMessage({
  title: "Hello",
  body: "World",
  payload: { url: "/inbox" },
});

const status = await peanut.push.getMessageStatus(id);

const subscription = peanut.realtime.subscribeTable("orders", {
  onEvent: (event) => console.log(event.action, event.row_id),
  onError: (error) => console.error(error),
});

// later
subscription.close();
```

Realtime uses fetch-based SSE parsing so API keys and bearer tokens work in browsers and Node.

## Console

The embedded console exposes:

- subscription inventory
- test enqueue
- queue summary (pending, partial success, retries)
- 24h failure stats (`/push/queue/stats`)
- diagnostics (VAPID / ntfy configuration probes)

## Quotas

Each successful enqueue increments the workspace `push_sends_month` usage counter. Idempotent replays with the same `idempotency_key` do not create duplicate rows or usage.
