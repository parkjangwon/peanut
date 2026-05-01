# Peanut Deployment Runbook

Peanut's release target is GitHub Releases plus GHCR images.

Default image:

```text
ghcr.io/parkjangwon/peanut:latest
```

Versioned image:

```text
ghcr.io/parkjangwon/peanut:<version>
```

## Required `.env` checklist

- `JWT_SECRET`: long random secret
- `DATABASE_URL`: usually `sqlite://data/peanut.db` in Docker
- `STORAGE_DIR`: usually `data/storage` in Docker
- `BIND_ADDR`: `0.0.0.0:3000` in Docker
- `TRUST_PROXY_HEADERS`: `true` only behind a trusted reverse proxy
- `FUNCTIONS_ENABLED`: set `false` when runtime extensions are not needed
- `FUNCTIONS_ALLOW_NETWORK`: keep `false` unless trusted admin functions need outbound network access
- `FUNCTIONS_MAX_CONCURRENT`: default `4`
- `FUNCTIONS_MEMORY_MB`: Deno/V8 heap cap per Function invocation, default `128`
- `FUNCTIONS_MAX_SOURCE_BYTES`: Function source-size cap, default `262144`
- `FUNCTIONS_MAX_OUTPUT_BYTES`: captured Function stderr/log cap, default `65536`
- `FUNCTIONS_SECRETS_MASTER_KEY`: optional dedicated encryption key for Function secrets; use a separate long secret in production
- `FUNCTIONS_WORK_DIR`: writable temp directory outside root, home, and DB directory

## Readiness and Metrics

- `GET /api/ready` checks SQLite, storage writability, pending restore markers, and the Functions runtime when enabled.
- `GET /api/admin/ops/metrics` returns admin-only operational counters for database size/page stats, storage object totals, stale multipart uploads, push backlog, Function failures/timeouts, and process uptime.
- In Docker or reverse-proxy deployments, monitor `/api/ready` from outside the container and alert when `status` is `not_ready`.

## Upgrade

Do not upgrade while a restore marker is pending.

```bash
curl -s "$BASE_URL/api/admin/backups/restore-pending" \
  -H "authorization: Bearer $ADMIN_JWT" | jq .

docker compose pull
docker compose up -d
docker compose logs --tail=200 peanut
curl -s "$BASE_URL/api/ready" | jq .
```

## Backup Before Upgrade

Back up the full `./data` directory in the default compose layout:

```bash
tar -czf peanut-data-$(date +%Y%m%d-%H%M%S).tgz data
```

You can also create a SQLite backup through the admin API:

```bash
curl -s -X POST "$BASE_URL/api/admin/backups" \
  -H "authorization: Bearer $ADMIN_JWT" | jq .
```

## Rollback

Pin a previous image tag in `.env`:

```text
PEANUT_IMAGE=ghcr.io/parkjangwon/peanut:<previous-version>
```

Then restart:

```bash
docker compose up -d
curl -s "$BASE_URL/api/ready" | jq .
```

## Restore Marker Flow

Schedule:

```bash
curl -s -X POST "$BASE_URL/api/admin/backups/<backup-name>/restore" \
  -H "authorization: Bearer $ADMIN_JWT" | jq .
```

Verify pending:

```bash
curl -s "$BASE_URL/api/admin/backups/restore-pending" \
  -H "authorization: Bearer $ADMIN_JWT" | jq .
```

Restart and verify:

```bash
docker compose restart peanut
curl -s "$BASE_URL/api/ready" | jq .
curl -s "$BASE_URL/api/admin/backups/restore-pending" \
  -H "authorization: Bearer $ADMIN_JWT" | jq .
```

Cancel pending restore:

```bash
curl -s -X DELETE "$BASE_URL/api/admin/backups/restore-pending" \
  -H "authorization: Bearer $ADMIN_JWT" | jq .
```
