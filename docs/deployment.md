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
- `PEANUT_HOST_PORT`: host port published by Docker Compose, default `3492`
- `DATABASE_URL`: usually `sqlite://data/peanut.db` in Docker
- `STORAGE_DIR`: usually `data/storage` in Docker
- `BIND_ADDR`: `0.0.0.0:3000` in Docker
- `PASSWORD_RESET_DELIVERY`: use `log` for operator-managed installs
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
- In default Docker Compose deployments, monitor `http://127.0.0.1:3492/api/ready` from the host or `/api/ready` through the reverse proxy and alert when `status` is `not_ready`.

## OCI VM Notes

- Open the OCI security list or NSG for the host port you publish with `PEANUT_HOST_PORT`, default `3492`, or place Peanut behind a TLS reverse proxy on `80/443`.
- Keep `JWT_SECRET` and `FUNCTIONS_SECRETS_MASTER_KEY` stable and backed up outside the VM.
- Back up the full `./data` directory, not only the SQLite database, because local object storage also lives there.
- Set `TRUST_PROXY_HEADERS=true` only when Peanut is reachable exclusively through a trusted reverse proxy.

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
