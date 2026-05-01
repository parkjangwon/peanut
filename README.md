# Peanut

Peanut is a self-hosted backend platform core packaged as a Rust service. It is
designed around app-level isolation: every app gets its own user namespace, data
tables, storage buckets, functions, push state, keys, and activity feed.

Peanut is intentionally operationally small:

- SQLite for persistence
- local filesystem object storage
- JWT auth with server-tracked refresh tokens
- app-scoped API keys with explicit scopes
- invite-only public beta signup with organizations and membership records
- built-in free beta plan quotas and usage counters
- platform admin roles: owner, developer, operator, viewer
- app-scoped Data, Storage, Push, and Functions APIs
- embedded Next.js admin console served by the Rust binary
- console workbenches for Auth, Data, Storage, Functions, Push, activity, and operations
- English and Korean console locale switching
- single-node production runbooks and diagnostics

## API Shape

The public application API is app-scoped:

- Auth: `/api/apps/:app_id/auth/...`
- Data: `/api/apps/:app_id/data/...`
- Storage: `/api/apps/:app_id/storage/...`
- Push: `/api/apps/:app_id/push/...`
- Functions: `/api/apps/:app_id/functions/...`

Application calls require `X-Peanut-Api-Key`. User-protected calls also require
`Authorization: Bearer <access_token>`. The JWT contains `app_id`, and Peanut
rejects bearer tokens used against a different app path.

Legacy global paths such as `/api/register`, `/api/login`, `/api/data`,
`/api/storage`, `/api/s3`, `/api/push`, and `/api/functions` are not mounted.

## Quick Start

For local development, build the embedded console and run the Rust service:

```bash
cd console && npm install && npm run build && cd ..
JWT_SECRET="$(openssl rand -hex 32)" cargo run
```

Then open `http://127.0.0.1:3000` and create the first platform admin from the
console.

For Docker Compose, create a `.env` with `JWT_SECRET`, then run:

```bash
docker compose up -d
```

Detailed setup, verification, first-admin, beta-invite, and deployment steps are
in the guide documents below.

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
- `docs/quotas-and-limits.md`
- `docs/quotas-and-limits.ko.md`
- `docs/auth-client.md`
- `docs/data-api.md`
- `docs/production-ops-runbook.md`
- `docs/migration-backup-guide.md`
- `docs/deployment.md`
