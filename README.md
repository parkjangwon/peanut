# Peanut

Peanut is a small self-host backend runtime that ships as a single Rust binary with an embedded web console.

It is intentionally narrow:
- SQLite for persistence
- local filesystem object storage
- JWT-based auth with admin approval flow
- an embedded static Next.js console
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
  - returns a typed JSON login response with bearer token and expiry
- `GET /api/me`
  - returns the authenticated user as JSON
  - protected routes now re-check the current user record on every request, so deactivated users lose access immediately even if they still hold an unexpired token
- `GET /api/admin/users`
  - admin-only user list
- `PUT /api/admin/users/:user_id/activate`
  - admin-only activation flow
- `PUT /api/admin/users/:user_id/deactivate`
  - admin-only suspension flow that immediately blocks protected API access for that user

### Storage
- user-scoped object storage
- authenticated users can:
  - list their own keys
  - upload objects
  - fetch objects
  - delete objects
- storage keys are automatically isolated per authenticated user

### Data API (SQLite-backed)
Peanut now exposes a constrained SQLite-backed data API for Peanut-managed logical tables.

Current capabilities:
- `GET /api/data/tables`
- `POST /api/data/tables`
- `GET /api/data/tables/:table`
- `GET /api/data/tables/:table/rows`
- `POST /api/data/tables/:table/rows`
- `GET /api/data/tables/:table/rows/:row_id`
- `PATCH /api/data/tables/:table/rows/:row_id`
- `DELETE /api/data/tables/:table/rows/:row_id`

Current model:
- admins define logical tables with JSON schema + fixed access policy
- rows are stored in Peanut-managed SQLite tables
- `owner_private` policy isolates rows per authenticated user
- row mutations are recorded in an internal event log

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
- authenticated, public, admin-only, or api-key invoke policy through `POST /api/functions/endpoints/:endpoint_slug`
- separate Node subprocess execution with a temp working directory and bounded runtime timeout
- invocation logs stored in SQLite, with detail lookup and retry from the console/API
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

### Console
The embedded console supports:
- health monitoring
- register/login/session inspection
- admin approval workflow
- user-scoped storage management
- ntfy push subscription management
- push queue inspection

The console is statically exported from Next.js and embedded into the Rust binary during build.

## API contract summary

### `GET /api/health`
Returns localized JSON:

```json
{
  "status": "ok",
  "message": "Systems are operational."
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
  "access_token": "jwt",
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
- `title_contains` and generic `filter_field/filter_op/filter_value` can be combined with `order_by`, `order`, and `limit`
- common console flow uses `contains`, `eq`, `ne`, `gt`, `gte`, `lt`, `lte`

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
- `DATABASE_URL` (default: `sqlite://peanut.db`)
- `STORAGE_DIR` (default: `data/storage`)
- `BIND_ADDR` (default: `127.0.0.1:3000`)
- `MAX_UPLOAD_BYTES` (default: `5242880`)
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
  -H 'authorization: Bearer <ACCESS_TOKEN>' \
  -H 'content-type: application/json' \
  -d '{"title":"buy milk"}'

# 5) query rows with filtering
curl -s "$BASE_URL/api/data/tables/todos/rows?filter_field=title&filter_op=contains&filter_value=milk&order_by=created_at&order=desc&limit=10" \
  -H 'authorization: Bearer <ACCESS_TOKEN>'
```

Quick notes:
- the first registered user becomes active admin automatically
- `owner_private` rows are scoped to the authenticated user
- the same bearer token works for storage, data, push, and session endpoints

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
