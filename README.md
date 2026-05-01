# Peanut

Peanut is a self-hosted backend platform core packaged as a Rust service. It is
designed around app-level isolation: every app gets its own user namespace, data
tables, storage buckets, functions, push state, keys, and activity feed.

Peanut is intentionally operationally small:

- SQLite for persistence
- local filesystem object storage
- JWT auth with server-tracked refresh tokens
- app-scoped API keys with explicit scopes
- app-scoped Data, Storage, Push, and Functions APIs
- embedded Next.js admin console served by the Rust binary
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

## First Install

Create the first platform admin once:

```bash
curl -s -X POST "$BASE_URL/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

The response contains an admin access token and refresh token. After any admin
exists, bootstrap returns `409`.

The embedded admin console uses the same bootstrap flow on a fresh install. Once
an admin exists, sign in through the console or call:

```bash
curl -s -X POST "$BASE_URL/api/admin/auth/login" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

Create an app key:

```bash
curl -s -X POST "$BASE_URL/api/apps/default/keys" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"name":"server","key_type":"server"}'
```

Register and login an app user:

```bash
curl -s -X POST "$BASE_URL/api/apps/default/auth/register" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "content-type: application/json" \
  --data '{"email":"user@example.com","password":"password123"}'

curl -s -X POST "$BASE_URL/api/apps/default/auth/login" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "content-type: application/json" \
  --data '{"email":"user@example.com","password":"password123"}'
```

## Core Docs

- `docs/openapi.yaml`
- `docs/app-scoped-api.md`
- `docs/auth-client.md`
- `docs/data-api.md`
- `docs/production-ops-runbook.md`
- `docs/migration-backup-guide.md`
- `docs/deployment.md`

## Verification

Local Rust checks:

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Admin console checks:

```bash
cd console
npm run build
```

`npm run build` exports static assets to `console/out`. Release binaries embed
that directory and serve the console from `/`, while `/api/...` remains the API
surface.

Docker Compose smoke:

```bash
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
scripts/verify-compose.sh
```

For an existing install, provide `PEANUT_ADMIN_TOKEN` instead of bootstrap
credentials.
