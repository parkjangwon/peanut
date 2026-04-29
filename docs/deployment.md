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
- `FUNCTIONS_WORK_DIR`: writable temp directory outside root, home, and DB directory

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
