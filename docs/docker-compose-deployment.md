# Peanut Docker Compose Deployment Guide

This guide covers single-node Docker Compose deployment with SQLite and local
filesystem storage. It is the default production-minimum path for Peanut.

## Compose Files

Use the published image:

```bash
docker compose up -d
```

Build the image from the current checkout:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

The default service is named `peanut`, exposes port `3000`, and persists state
under `./data`.

## Required `.env`

Create `.env` next to `docker-compose.yml`:

```env
PEANUT_IMAGE=ghcr.io/parkjangwon/peanut:latest
JWT_SECRET=replace-with-a-long-random-secret
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret
DATABASE_URL=sqlite://data/peanut.db
STORAGE_DIR=data/storage
BIND_ADDR=0.0.0.0:3000
MAX_UPLOAD_BYTES=5242880
FUNCTIONS_ENABLED=true
FUNCTIONS_ALLOW_NETWORK=false
FUNCTIONS_MAX_CONCURRENT=4
FUNCTIONS_MEMORY_MB=128
FUNCTIONS_MAX_SOURCE_BYTES=262144
FUNCTIONS_MAX_OUTPUT_BYTES=65536
FUNCTIONS_WORK_DIR=/tmp/peanut-functions
BACKUP_ON_STARTUP=false
TRUST_PROXY_HEADERS=false
MULTIPART_STALE_HOURS=24
MULTIPART_CLEANUP_INTERVAL_SECONDS=3600
RUST_LOG=info
```

Generate secrets with:

```bash
openssl rand -hex 32
```

Keep `JWT_SECRET` stable across restarts unless you intentionally want to
invalidate all sessions. Keep `FUNCTIONS_SECRETS_MASTER_KEY` stable if you use
Function secrets.

## Start and Inspect

```bash
mkdir -p data
docker compose up -d
docker compose ps
docker compose logs -f peanut
```

Check readiness:

```bash
curl -fsS http://127.0.0.1:3000/api/ready
```

Open:

```text
http://127.0.0.1:3000
```

## First Admin

Use the console setup flow, or bootstrap by API:

```bash
curl -s -X POST "http://127.0.0.1:3000/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

Save the returned admin token for initial automation:

```bash
ADMIN_TOKEN="..."
```

## Compose Smoke Verification

Fresh install with bootstrap credentials:

```bash
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
scripts/verify-compose.sh
```

Existing install with an admin token:

```bash
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

The verifier checks readiness, Deno availability, beta invite/signup, app auth,
Data CRUD, Storage CRUD, Functions, Push diagnostics, backup download, and
restore-pending safety.

## Reverse Proxy Notes

Put a TLS reverse proxy in front of Peanut for public traffic. Only set:

```env
TRUST_PROXY_HEADERS=true
```

when Peanut is reachable only through a trusted proxy that correctly sets
`X-Forwarded-For`. Leave it `false` for direct exposure or uncertain proxy
chains.

## Upgrade

Before upgrading, create a backup:

```bash
curl -fsS -X POST "$BASE_URL/api/admin/backups" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
tar -czf peanut-data-$(date +%Y%m%d-%H%M%S).tgz data
```

Confirm no restore is pending:

```bash
curl -fsS "$BASE_URL/api/admin/backups/restore-pending" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

Upgrade:

```bash
docker compose pull
docker compose up -d
docker compose logs --tail=200 peanut
curl -fsS "$BASE_URL/api/ready"
```

When using local builds:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

## Restore and Rollback

Schedule a SQLite backup restore:

```bash
curl -fsS -X POST "$BASE_URL/api/admin/backups/<backup>.backup/restore" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"confirmation":"<backup>.backup","reason":"rollback"}'
```

Restart so Peanut applies the restore marker:

```bash
docker compose restart peanut
curl -fsS "$BASE_URL/api/ready"
```

For a full filesystem rollback, stop the service and restore the whole `./data`
directory from a known-good archive.

## Stop

Stop the service but keep data:

```bash
docker compose down
```

Remove local data only when you are certain it is not production data:

```bash
docker compose down
rm -rf data
```
