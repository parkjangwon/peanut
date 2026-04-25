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
- `GET /api/admin/users`
  - admin-only user list
- `PUT /api/admin/users/:user_id/activate`
  - admin-only activation flow

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
4. activate a second user
5. upload/read/delete a storage object
6. subscribe an ntfy topic and enqueue a push message

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
