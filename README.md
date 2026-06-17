# Peanut

<img width="1376" height="768" alt="Peanut admin console" src="https://github.com/user-attachments/assets/ae31aa2a-4793-4465-b58c-e19b3bebf2db" />

Peanut is a self-hosted BaaS packaged as a Rust single binary. It serves the
backend API and an embedded Next.js admin console from the same process, using
SQLite and local filesystem storage by default.

Peanut is not a SaaS product. It is built for teams that want a small,
inspectable backend platform they can run themselves, while still keeping
workspace and app isolation for internal teams or multiple projects.

## What Peanut Provides

- App-scoped Auth with isolated user namespaces
- App-scoped Data tables, rows, import/export, query presets, and row events
- App-scoped Storage buckets and objects backed by the local filesystem
- App-scoped Functions powered by a local Deno runtime
- App-scoped Push subscriptions, queue, diagnostics, and test messages
- API keys with client, server, and admin scopes
- Workspace setup invites, membership, resource limits, and usage counters
- Platform admin roles: owner, developer, operator, and viewer
- Backup, restore-pending, readiness, diagnostics, and ops metrics
- Embedded Next.js admin console with English and Korean locales
- Docker Compose production gate for self-hosted release verification

## Architecture

Peanut is a layered monolith:

- `src/main.rs` loads configuration, applies pending restores, initializes
  SQLite, configures local storage, starts background workers, and serves Axum.
- `src/app.rs` mounts the HTTP surface and keeps legacy global application
  routes out of the runtime API.
- `src/api/` contains the app, workspace, auth, data, storage, functions, push,
  backup, audit, and ops handlers.
- `src/middleware/` enforces bearer auth, SDK app-key auth, app mismatch checks,
  request IDs, rate limits, function availability, and auth client policy.
- `src/storage/local/` owns local object storage behavior.
- `src/functions/` runs trusted admin-managed functions through Deno.
- `console/` contains the Next.js admin console. `console/out` is embedded into
  the Rust binary with `rust-embed`.
- `sdks/` contains client SDKs and examples for app-scoped API usage.

SQLite is the source of truth for platform data. Object bytes live under the
configured storage directory. Background work such as push delivery, multipart
cleanup, and scheduled backups runs inside the same process.

## API Shape

Application APIs are app-scoped:

- Auth: `/api/apps/:app_id/auth/...`
- Data: `/api/apps/:app_id/data/...`
- Storage: `/api/apps/:app_id/storage/...`
- Push: `/api/apps/:app_id/push/...`
- Functions management: `/api/apps/:app_id/functions/...`
- Function invoke: `/api/apps/:app_id/function-endpoints/:endpoint_slug`

Application calls require `X-Peanut-Api-Key`. User-protected calls also require
`Authorization: Bearer <access_token>`. JWTs include `app_id`, and Peanut rejects
bearer tokens used against a different app path.

Platform operation routes remain behind admin bearer auth. Legacy global
application routes are not part of the runtime API surface.

## Local Development

Build the embedded console first, then run the Rust service:

```bash
cd console
npm install
npm run build
cd ..

export JWT_SECRET="$(openssl rand -hex 32)"
cargo run
```

Open `http://127.0.0.1:3000` and create the first platform admin from the
console.

For a backend-only release build:

```bash
scripts/build.sh
```

`scripts/build.sh` currently builds the Rust binary only. If the embedded
console needs to be fresh, run `npm run build` in `console/` before packaging or
use the Docker build path below.

## Docker Compose

Create `.env` next to `docker-compose.yml`:

```env
PEANUT_HOST_PORT=3492
JWT_SECRET=replace-with-a-long-random-secret
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret
DATABASE_URL=sqlite://data/peanut.db
STORAGE_DIR=data/storage
BIND_ADDR=0.0.0.0:3000
PASSWORD_RESET_DELIVERY=log
```

Start Peanut:

```bash
docker compose up -d
```

The Dockerfile builds the console, copies `console/out` into the Rust build
stage, installs Deno in the runtime image, and serves everything from the Peanut
binary.

Docker Compose publishes host port `3492` by default while the app listens on
container port `3000`. Open `http://127.0.0.1:3492`, create or sign in as a
platform admin, then run
the production gate before trusting a deployment:

```bash
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

To verify a locally built image with the same gate:

```bash
COMPOSE_FILES="docker-compose.yml docker-compose.build.yml" \
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
JWT_SECRET="$(openssl rand -hex 32)" \
FUNCTIONS_SECRETS_MASTER_KEY="$(openssl rand -hex 32)" \
scripts/verify-compose.sh
```

## Production Gate

`scripts/verify-compose.sh` is the self-hosted release acceptance gate. It
checks readiness, Deno availability, workspace setup, app A/B isolation,
same-email-per-app auth, cross-app denial for Data/Storage/Functions, disabled
app block/re-enable behavior, Data CRUD, Storage CRUD, Function lint/create/invoke,
Push diagnostics/test message, backup download, restore scheduling, restore
marker clearing, and clean readiness after restore-pending is cleared.

CI also runs Rust formatting, Clippy, compiled tests, full tests, console lint,
console build, OpenAPI checks, release smoke checks, and the Docker Compose gate.

## Runtime Boundaries

Peanut is designed for small self-hosted deployments, not horizontally scaled
multi-node infrastructure. SQLite and local filesystem storage keep operations
simple, but they also make volume backup and single-node availability part of
the operator's responsibility.

Peanut Functions are trusted admin-managed extensions. They use a local Deno
subprocess with bounded host bindings and configurable limits. This is
process-level hardening, not a hostile-tenant sandbox. Installations that do not
need Functions should set:

```bash
FUNCTIONS_ENABLED=false
```

`FUNCTIONS_ALLOW_NETWORK=false` keeps common network APIs unavailable, and
`FUNCTIONS_MAX_CONCURRENT` caps simultaneous function invocations.

## Core Docs

- `docs/openapi.yaml`
- `docs/local-development.md`
- `docs/local-development.ko.md`
- `docs/docker-compose-deployment.md`
- `docs/docker-compose-deployment.ko.md`
- `docs/app-scoped-api.md`
- `docs/app-scoped-api.ko.md`
- `docs/getting-started.md`
- `docs/getting-started.ko.md`
- `docs/resource-limits.md`
- `docs/resource-limits.ko.md`
- `docs/auth-client.md`
- `docs/auth-client.ko.md`
- `docs/data-api.md`
- `docs/data-api.ko.md`
- `docs/production-ops-runbook.md`
- `docs/production-ops-runbook.ko.md`
- `docs/migration-backup-guide.md`
