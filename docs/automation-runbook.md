# Peanut automation runbook

This runbook connects Peanut service tokens with real operator automation.

Use this when you want to:
- run nightly exports
- run health/storage checks from cron
- automate bounded admin workflows without a browser session

See also:
- `docs/service-tokens.md`
- `examples/service-tokens/`
- `examples/operations-e2e/`

## Recommended pattern

1. log in once as an admin and mint a dedicated service token
2. store that plaintext token in a secure env file on the machine running automation
3. call Peanut protected APIs with `Authorization: Bearer pst_...`
4. revoke and replace the token when the automation no longer needs access

## Good fits for service-token automation

- Data API export jobs
- bounded table maintenance jobs
- storage metadata checks
- internal deploy hooks
- operator-only health/readiness probes that need protected endpoints

## What not to automate with this yet

- third-party customer app auth
- per-tenant delegation
- broad user impersonation
- unbounded SQL-style database access

## Environment file example

```bash
export BASE_URL=http://127.0.0.1:3000
export SERVICE_TOKEN='pst_...'
```

## Cron example: table export snapshot

```bash
#!/usr/bin/env bash
set -euo pipefail

source /opt/peanut/peanut.env
TIMESTAMP=$(date +%F-%H%M%S)
OUT_DIR=/opt/peanut/backups
mkdir -p "$OUT_DIR"

curl -s "$BASE_URL/api/data/tables/ops_todos/export" \
  -H "authorization: Bearer $SERVICE_TOKEN" \
  > "$OUT_DIR/ops_todos-$TIMESTAMP.json"
```

Example crontab:

```cron
15 2 * * * /opt/peanut/scripts/export-ops-todos.sh
```

## Cron example: protected storage HEAD check

```bash
#!/usr/bin/env bash
set -euo pipefail

source /opt/peanut/peanut.env
curl -fsSI "$BASE_URL/api/s3/assets/ops/hello.txt" \
  -H "authorization: Bearer $SERVICE_TOKEN"
```

Example crontab:

```cron
*/30 * * * * /opt/peanut/scripts/check-storage-head.sh
```

## Recommended rollout flow

For a full bootstrap path, follow:
1. `examples/service-tokens/create-token.sh` or `create-token-jq.sh`
2. `examples/operations-e2e/`
3. convert the validated sequence into your local cron/systemd/CI job

## Rotation guidance

- prefer one token per automation purpose
- keep token names explicit, e.g. `nightly-export`, `storage-head-check`, `deploy-hook`
- revoke old tokens instead of reusing one token for every job
- if a machine is retired, revoke all related tokens immediately
