# Peanut

<img width="1376" height="768" alt="1777174518806" src="https://github.com/user-attachments/assets/d4a00c2e-8b5d-46a4-86e6-38220cb6e3fd" />

Peanut is a small self-host backend runtime that ships as a single Rust binary.

It is intentionally narrow:
- SQLite for persistence
- local filesystem object storage
- JWT-based auth with admin approval flow
- API-first backend surface for external apps and operator tooling
- a simple ntfy-based push queue MVP

The goal is not to become a giant backend platform. The goal is to give a solo developer or small team a backend core that is easy to understand, easy to deploy, and easy to operate.

## Product philosophy

Peanut is built around a few constraints:

1. Single-binary deployment
   - the Rust server serves both the API and the embedded admin console
2. Low operational complexity
   - SQLite + local storage instead of mandatory external services
3. Honest feature scope
   - ship a complete small feature rather than a large half-implemented platform
4. Self-host first
   - one machine, one folder, one service is a valid production shape

## Current feature set

### Auth and admin
- `POST /api/register`
  - first user becomes active admin automatically
  - later users are created inactive and require admin approval
- `POST /api/login`
  - returns a typed JSON login response with short-lived bearer access token, refresh token, and expiry
- `POST /api/auth/refresh`
  - rotates a valid refresh token and returns a fresh access token + refresh token pair
- `POST /api/auth/logout`
  - revokes a refresh token so external apps can explicitly end a session
- `POST /api/auth/change-password`
  - authenticated password change flow
  - revokes existing refresh sessions for that user after success
- `POST /api/auth/forgot-password`
  - creates a password reset token for the matching user
  - delivery is controlled by `PASSWORD_RESET_DELIVERY`
  - `inline` returns the reset token in JSON for local/dev/self-host flows
  - `log` omits the token from the response and writes it to the server log for operator-managed delivery
- `POST /api/auth/reset-password`
  - one-time reset token flow for setting a new password
  - revokes existing refresh sessions for that user after success
- `GET /api/auth/sessions`
  - returns the current user's tracked auth sessions
- `GET /api/auth/events`
  - returns the current user's latest auth events for audit/debug visibility
- `DELETE /api/auth/sessions/:session_id`
  - revokes a single tracked auth session
- `POST /api/auth/sessions/revoke-all`
  - revokes all tracked auth sessions for the current user
- `GET /api/me`
  - returns the authenticated user as JSON
  - protected routes now re-check the current user record on every request, so deactivated users lose access immediately even if they still hold an unexpired token
- `GET /api/admin/users`
  - admin-only user list
- `PUT /api/admin/users/:user_id/activate`
  - admin-only activation flow
- `PUT /api/admin/users/:user_id/deactivate`
  - admin-only suspension flow that immediately blocks protected API access for that user

What this means for external frontend apps:
- Peanut can now act as the app's auth backend for signup, login, session refresh, logout, password change, and password reset
- access tokens stay short-lived while refresh tokens provide longer-lived sessions
- refresh sessions are server-tracked and revoked on logout, password change, password reset, or admin deactivation
- apps can inspect recent auth events through `GET /api/auth/events` to debug login/session/reset activity
- operators can optionally lock app-facing auth routes to specific browser origins and client ids
- see `docs/auth-client.md` for the integration guide and `examples/auth-client-web/` for a minimal browser example

### External auth client guide
- English guide: `docs/auth-client.md`
- Korean guide: `docs/auth-client.ko.md`
- browser example: `examples/auth-client-web/`

### Storage
- user-scoped object storage
- legacy simple endpoints remain available for authenticated users:
  - list their own keys
  - upload objects
  - fetch objects
  - delete objects
- S3-like path-style endpoints are now also available under `/api/s3/:bucket/*key`
- authenticated clients can now mint presigned S3-like URLs through `POST /api/s3/:bucket/*key/presign`
- S3-like object routes now accept either bearer auth, SigV4-style `Authorization` header auth, or SigV4-style query auth from presigned URLs
- S3-like object responses now include content-type, content-length, ETag, and last-modified metadata
- S3-like success/error responses now also include `x-amz-request-id` headers, and object `Last-Modified` headers are emitted as HTTP-date strings
- S3-like bucket listing supports `list-type=2`, `prefix`, `delimiter`, `max-keys`, and `continuation-token`
- continuation tokens are now opaque base64url-style tokens instead of raw object keys
- S3-like listing now emits `CommonPrefixes` XML blocks when `delimiter=/` is used
- S3-like storage errors now return XML error envelopes such as `NoSuchKey` and `InvalidRequest`
- storage keys remain automatically isolated per authenticated user

### Data API (SQLite-backed)
Peanut now exposes a constrained SQLite-backed data API for Peanut-managed logical tables.

Current capabilities:
- `GET /api/data/tables`
- `POST /api/data/tables`
- `GET /api/data/tables/:table`
- `PATCH /api/data/tables/:table`
- `GET /api/data/tables/:table/presets`
- `POST /api/data/tables/:table/presets`
- `GET /api/data/tables/:table/presets/:preset_id/run`
- `PATCH /api/data/tables/:table/presets/:preset_id`
- `DELETE /api/data/tables/:table/presets/:preset_id`
- `GET /api/data/tables/:table/export`
- `POST /api/data/tables/:table/import`
- `GET /api/data/tables/:table/rows`
- `POST /api/data/tables/:table/rows`
- `GET /api/data/tables/:table/events`
- `GET /api/data/tables/:table/events/stream`
- `GET /api/data/tables/:table/rows/:row_id`
- `PATCH /api/data/tables/:table/rows/:row_id`
- `DELETE /api/data/tables/:table/rows/:row_id`

Current model:
- admins define logical tables with JSON schema + fixed access policy
- rows are stored in Peanut-managed SQLite tables
- `owner_private` policy isolates rows per authenticated user
- row mutations are recorded in an internal event log
- admin APIs can replay row mutations from `GET /api/data/tables/:table/events?since_id=<event_id>` for resume/sync flows
- admin APIs can subscribe to row mutation events through `GET /api/data/tables/:table/events/stream` (SSE), with event ids included in each payload
- admin APIs can persist reusable query presets per table for repeated operator workflows
- table snapshots can be exported and re-imported through bounded admin APIs
- schema updates now follow safe evolution rules:
  - existing field types cannot change in place
  - non-empty tables cannot drop existing fields
  - new required fields on non-empty tables must provide defaults

What this still does not mean:
- Peanut does not expose raw SQL like `POST /api/sql`
- Peanut is not trying to be a full database-console-as-a-service
- query/filter support is intentionally narrow in this release

### Push (current release MVP)
Peanut currently ships a practical hybrid push layer:
- ntfy topic subscriptions for the simple self-host flow
- Web Push delivery for stored browser subscriptions when VAPID env vars are configured

Endpoints:
- `GET /api/push/subscriptions`
- `POST /api/push/subscriptions`
- `DELETE /api/push/subscriptions/:subscription_id`
- `GET /api/push/vapid-public-key`
- `POST /api/push/messages`
- `GET /api/push/queue`

Runtime settings:
- `NTFY_BASE_URL`
  - defaults to `https://ntfy.sh`
  - can point at a self-hosted ntfy server such as `https://push.example.com`
- `NTFY_AUTH_TOKEN`
  - optional bearer token for authenticated ntfy servers

What this means:
- users can subscribe an ntfy topic with `{ "topic": "alerts_main" }`
- browsers can register a Web Push subscription with `{ "endpoint": "...", "keys": { "p256dh": "...", "auth": "..." } }`
- clients can fetch `GET /api/push/vapid-public-key` to bootstrap browser `PushManager.subscribe(...)`
- push messages are queued in SQLite
- a background worker delivers queue items to ntfy or Web Push subscriptions
- queue status, retries, and last error are visible through the API and console

What this does not mean yet:
- Peanut still does not try to be a complete push platform
- there is no polished browser service-worker setup flow in the embedded console yet
- Web Push delivery requires VAPID runtime configuration

### Peanut Functions (JS/TS sandbox MVP)
Peanut now includes a minimal function runtime for small backend extensions.

Current capabilities:
- admin-managed functions stored in SQLite
- JavaScript or TypeScript source managed from the console/API
- per-function endpoint slug, invoke policy, env/secrets JSON, allowed origins, rate limit, and timeout
- secrets are stored per function version, never returned in API payloads, and only exposed as `secret_key_count`
- authenticated, public, admin-only, or api-key invoke policy through `POST /api/functions/endpoints/:endpoint_slug`
- same endpoint supports inline sync execution or queued async execution with `async_invoke: true`
- admin APIs expose version history through `GET /api/functions/:name/versions`
- admins can roll back the active runtime through `POST /api/functions/:name/versions/:version_number/rollback`
- admin APIs can subscribe to invocation lifecycle events through `GET /api/functions/:name/events` (SSE)
- separate Node subprocess execution with a temp working directory and bounded runtime timeout
- invocation logs stored in SQLite, with queued/running/succeeded/failed lifecycle, `invoke_mode`, `function_version_id`, `retry_count`, `parent_invocation_id`, detail lookup, attempt-chain lookup, and retry from the console/API
- bounded in-process Peanut host bindings for authenticated functions:
  - `ctx.peanut.storage.list/get/put/delete`
  - `ctx.peanut.push.enqueue`
  - `ctx.peanut.data.listRows/getRow/createRow/updateRow/deleteRow`
- host bindings reuse Peanut's existing auth and policy checks, so owner-scoped data/storage access stays user-scoped inside functions too

Current constraints:
- functions must export `default` or named `handler`
- JSON input/output only
- no arbitrary package installation
- source containing blocked runtime escape patterns is rejected
- no direct outbound network access; functions extend Peanut through bounded host bindings instead
- this is a narrow sandboxed extension layer, not a full Lambda clone

### Console / operator surface
Peanut is currently running in API-first mode.

- the old embedded Next.js console source was removed
- the backend now serves a small landing page at `/` and keeps the product usable through `/api/...`
- a new operations console is planned as part of v2 and will be rebuilt separately from the backend core

## API contract summary

Request/response notes:
- every response now includes an `x-request-id` header for correlation
- JSON error responses use a structured envelope with `error`, `code`, and `request_id`

Example error body:

```json
{
  "error": "missing bearer token",
  "code": "unauthorized",
  "request_id": "req_123"
}
```

### `GET /api/health`
Returns localized JSON:

```json
{
  "status": "ok",
  "message": "Systems are operational."
}
```

### `GET /api/ready`
Returns backend readiness state for operators:

```json
{
  "status": "ready",
  "checks": [
    { "name": "database", "ok": true, "message": "database query succeeded" },
    { "name": "storage", "ok": true, "message": "storage directory is writable", "path": "data/storage" }
  ]
}
```

### `POST /api/register`
Request:

```json
{
  "email": "admin@example.com",
  "password": "secret123"
}
```

Response:

```json
{
  "message": "First user registered as active admin.",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

Validation rules:
- email is required and must look like a valid address
- password must be at least 8 characters

### `POST /api/login`
Response:

```json
{
  "access_token": "***",
  "refresh_token": "***",
  "token_type": "Bearer",
  "expires_at": "2026-04-25T00:00:00Z",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### `GET /api/me`
Response:

```json
{
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### `GET /api/storage`
Response:

```json
{
  "keys": ["notes/welcome.txt"]
}
```

### S3-like storage endpoints
- `GET /api/s3/:bucket?list-type=2&prefix=notes/&max-keys=100&continuation-token=...`
- `HEAD /api/s3/:bucket/*key`
- `GET /api/s3/:bucket/*key`
- `PUT /api/s3/:bucket/*key`
- `DELETE /api/s3/:bucket/*key`

### `POST /api/push/subscriptions`
Request:

```json
{
  "topic": "alerts_main"
}
```

### `POST /api/data/tables`
Typical admin request:

```json
{
  "name": "todos",
  "display_name": "Todos",
  "schema": {
    "fields": {
      "title": { "type": "string", "required": true, "max_length": 200 },
      "done": { "type": "boolean", "required": false, "default": false }
    }
  },
  "access_policy": {
    "mode": "owner_private"
  }
}
```

Typical response:

```json
{
  "table": {
    "name": "todos",
    "display_name": "Todos",
    "schema": {
      "fields": {
        "title": { "type": "string", "required": true, "max_length": 200, "default": null },
        "done": { "type": "boolean", "required": false, "max_length": null, "default": false }
      }
    },
    "access_policy": {
      "mode": "owner_private"
    }
  }
}
```

### `POST /api/data/tables/:table/rows`
Typical authenticated request:

```json
{
  "title": "buy milk"
}
```

Typical response:

```json
{
  "row": {
    "id": "uuid",
    "owner_user_id": "uuid",
    "data": {
      "title": "buy milk",
      "done": false
    },
    "created_at": "2026-04-25 01:05:58",
    "updated_at": "2026-04-25 01:05:58"
  }
}
```

### `GET /api/data/tables/:table/rows`
Example query:

```text
/api/data/tables/todos/rows?filter_field=title&filter_op=contains&filter_value=milk&order_by=created_at&order=desc&limit=10
```

What to expect:
- `search` scans declared string fields while keeping the query surface bounded
- `title_contains` and generic `filter_field/filter_op/filter_value` can be combined with `order_by`, `order`, `limit`, and `offset`
- common console flow uses `contains`, `starts_with`, `ends_with`, `eq`, `ne`, `gt`, `gte`, `lt`, `lte`

### `GET /api/data/tables/:table/export`
Admin snapshot export:
- returns table metadata plus normalized rows
- includes `metadata.export_version`, `metadata.row_count`, and `metadata.checksum_sha256`
- checksum is calculated over the exported table+rows artifact for backup verification
- useful for backups, migration between environments, or fixture generation

### `GET /api/data/tables/:table/events`
### `GET /api/data/tables/:table/events/checkpoint`
Admin row event log:
- supports `limit`, `row_id`, `action`, and `since_id`
- `GET /api/data/tables/:table/events/checkpoint` returns the latest durable row-event id for resume checkpoints
- default mode returns latest events first for audit/debugging
- `since_id` switches to ascending replay order for resume/sync workers

### `GET /api/data/tables/:table/events/stream`
Admin row realtime stream:
- SSE endpoint for row mutation events
- emits insert, update, delete events as they happen
- each payload includes the durable event `id` so clients can resume with `since_id`
- useful for operator dashboards or live sync workers

### `GET /api/data/tables/:table/presets`
### `POST /api/data/tables/:table/presets`
### `GET /api/data/tables/:table/presets/:preset_id/run`
### `PATCH /api/data/tables/:table/presets/:preset_id`
### `DELETE /api/data/tables/:table/presets/:preset_id`
Admin saved query presets:
- store reusable bounded row-query params per table
- useful for repeated operator filters like "open items", "recent failures", or "buy-* tasks"
- presets persist `search`, filters, ordering, limit, and offset
- `GET /api/data/tables/:table/presets/:preset_id/run` executes the saved preset and returns bounded row results directly

### `POST /api/data/tables/:table/import`
Admin snapshot import:
- accepts `{ "mode": "append" | "replace", "rows": [...] }`
- `restore_table: true` can also restore `display_name`, `schema`, and `access_policy` before rows are inserted
- `verify_checksum: true` with `metadata` validates the incoming artifact before import mutates rows
- checksum verification expects export-style artifact fields (`table.created_by`, `table.created_at`, row ids, `created_at`, `updated_at`)
- imported rows are normalized against the current schema before insert
- owner-private tables require `owner_user_id` per imported row

## Repository layout

```text
.
├── Cargo.toml
├── Cargo.lock
├── Dockerfile
├── docker-compose.yml
├── build.rs
├── migrations/
├── src/
│   ├── main.rs
│   ├── db.rs
│   ├── console.rs
│   ├── i18n.rs
│   ├── api/
│   │   ├── admin.rs
│   │   ├── auth.rs
│   │   ├── common.rs
│   │   ├── health.rs
│   │   ├── push.rs
│   │   ├── storage.rs
│   │   └── mod.rs
│   ├── auth/
│   ├── middleware/
│   ├── push/
│   └── storage/
├── locales/
└── peanut-console/
```

## Configuration

Peanut reads configuration from environment variables.

Required:
- `JWT_SECRET`

Optional:
- `DATABASE_URL` (default: `sqlite://peanut.db`; must use `sqlite:`)
- `STORAGE_DIR` (default: `data/storage`; must not be empty)
- `BIND_ADDR` (default: `127.0.0.1:3000`; must be a valid socket address)
- `MAX_UPLOAD_BYTES` (default: `5242880`; must be a positive integer)
- `PASSWORD_RESET_DELIVERY` (default: `inline`; `inline` or `log`)
- `AUTH_ALLOWED_ORIGINS` (comma-separated origins; when set, auth routes require a matching `Origin` header)
- `AUTH_ALLOWED_CLIENT_IDS` (comma-separated client ids; when set, auth routes require a matching `x-peanut-client-id` header)
- `RUST_LOG` (default: `info`)
- `WEB_PUSH_VAPID_PRIVATE_KEY` (required only for Web Push delivery)
- `WEB_PUSH_VAPID_SUBJECT` (required only for Web Push delivery; `mailto:` or `https://`)

See `.env.example` for a starter config.

## API quickstart with curl

This is the fastest way to exercise Peanut without opening the console.

```bash
export BASE_URL=http://127.0.0.1:3000

# 1) register first admin
curl -s -X POST "$BASE_URL/api/register" \
  -H 'content-type: application/json' \
  -d '{"email":"admin@example.com","password":"your-password"}'

# 2) login
curl -s -X POST "$BASE_URL/api/login" \
  -H 'content-type: application/json' \
  -d '{"email":"admin@example.com","password":"your-password"}'

# 3) copy access_token from the login response, then use it below
curl -s -X POST "$BASE_URL/api/data/tables" \
  -H 'authorization: Bearer <ACCESS_TOKEN>' \
  -H 'content-type: application/json' \
  -d '{
    "name": "todos",
    "display_name": "Todos",
    "schema": {
      "fields": {
        "title": { "type": "string", "required": true, "max_length": 200 },
        "done": { "type": "boolean", "required": false, "default": false }
      }
    },
    "access_policy": { "mode": "owner_private" }
  }'

# 4) insert a row
curl -s -X POST "$BASE_URL/api/data/tables/todos/rows" \
  -H "content-type: application/json" \
  -H "authorization: Bearer YOUR_ACCESS_TOKEN" \
  -d '{"title":"buy milk"}'

# 5) query rows with filtering, search, and offset
curl -s "$BASE_URL/api/data/tables/todos/rows?search=buy&filter_field=title&filter_op=starts_with&filter_value=buy&order_by=title&order=asc&limit=10&offset=0" \
  -H "authorization: Bearer YOUR_ACCESS_TOKEN"
```

Quick notes:
- the first registered user becomes active admin automatically
- `owner_private` rows are scoped to the authenticated user
- the same bearer token works for storage, data, push, and session endpoints
- for a full external frontend auth flow, see `docs/auth-client.md` and `examples/auth-client-web/`

## Local development

### Prerequisites
- Rust toolchain
- Node.js + npm

### Run tests

```bash
cargo test
```

### Build the console only

```bash
cd peanut-console
npm install
npm run lint
npm run build
```

### Build the full project

```bash
./scripts/build.sh
```

### Run the binary

```bash
export JWT_SECRET='replace-this'
./target/release/peanut
```

Then open:
- `http://127.0.0.1:3000`

## Docker

```bash
cp .env.example .env
# edit JWT_SECRET in .env

docker compose up --build
```

### Docker Compose operations guide

Operational notes for the provided `docker-compose.yml`:
- container port `3000` is published to host `3000`
- `./data` is mounted into `/app/data`
- the default SQLite path is `sqlite://data/peanut.db`
- the default storage path is `data/storage`
- restart policy is `always`

Recommended day-1 flow:

```bash
cp .env.example .env
# set JWT_SECRET
# optionally set WEB_PUSH_VAPID_PRIVATE_KEY / WEB_PUSH_VAPID_SUBJECT

docker compose up --build -d
docker compose logs -f peanut
```

Recommended day-2 operations:

```bash
# restart after config change
docker compose up -d --build

# inspect current logs
docker compose logs --tail=200 peanut

# stop without deleting data
docker compose stop

# start again
docker compose start
```

Backup and restore notes:
- back up `./data/peanut.db` and `./data/storage/`
- if you keep the default compose layout, backing up the entire `./data/` directory is enough
- restore by stopping the container, replacing `./data/`, and starting the stack again

## Local browser Web Push experiment guide

Use this when you want to verify the browser-facing Web Push path end to end on your own machine.

1. Set runtime env:
   - `JWT_SECRET`
   - `WEB_PUSH_VAPID_PRIVATE_KEY`
   - `WEB_PUSH_VAPID_SUBJECT`
2. Start Peanut and open `http://127.0.0.1:3000`
3. Register the first admin user and log in
4. In the Push section, confirm that the VAPID public key field is auto-filled
5. Click `Register browser Web Push`
6. Allow browser notification permission when prompted
7. Confirm that a `web_push` subscription appears in the console
8. Enqueue a push message
9. Confirm the queue item moves to `sent` or inspect `last_error` if delivery fails

Notes:
- automatic browser registration requires notification permission in the real browser
- the manual Web Push subscription form is useful for validating the backend API path even when browser permission prompts are blocked
- if `GET /api/push/vapid-public-key` returns 404, check the VAPID env vars first

## Release checklist

Before shipping a change, verify:

```bash
cargo test
cd peanut-console && npm run lint && npm run build && cd ..
./scripts/build.sh
```

Manual smoke test:
1. open the console
2. register first admin
3. login
4. create a data table
5. create and update a row
6. verify title filter or generic field filter works
7. subscribe an ntfy topic and enqueue a push message
8. if VAPID is configured, confirm the public key auto-loads and try browser or manual Web Push subscription

## Backups and operations

For a simple single-node deployment, back up:
- the SQLite database file
- the storage directory

In the default docker-compose layout that means backing up `./data/`.

## Current non-goals

Peanut is intentionally not trying to be:
- a large multi-tenant backend cloud
- a plugin/orchestration framework
- a Supabase/Firebase replacement
- a full Web Push platform yet

## License

No license file has been chosen yet.
If you plan to distribute Peanut publicly, add an explicit license before release.
