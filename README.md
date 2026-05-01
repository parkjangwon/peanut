# Peanut

Peanut is a self-hosted BaaS packaged as a Rust single binary. It serves the
backend API and the embedded admin console from the same process, using SQLite
and local filesystem storage by default.

Peanut is not a SaaS product. It is designed for teams that want a small,
inspectable backend platform they can run themselves, while still keeping
workspace and app isolation for internal teams or multiple projects.

## What Peanut Provides

- App-scoped Auth with isolated user namespaces
- App-scoped Data tables and rows
- App-scoped Storage buckets and objects
- App-scoped Functions powered by Deno
- App-scoped Push subscriptions, queue, and diagnostics
- API keys with client, server, and admin scopes
- Workspace setup invites, membership, resource limits, and usage counters
- Platform admin roles: owner, developer, operator, viewer
- Backup, restore-pending, readiness, diagnostics, and ops metrics
- Embedded Next.js admin console with English and Korean locales
- Docker Compose production gate for self-hosted release verification

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

Legacy global application routes are not part of the runtime API surface.

## Local Development

Build the embedded console and run the Rust service:

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

## Docker Compose

Create `.env` next to `docker-compose.yml`:

```env
JWT_SECRET=replace-with-a-long-random-secret
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret
```

Start Peanut:

```bash
docker compose up -d
```

Open `http://127.0.0.1:3000`, create or sign in as a platform admin, then run
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
