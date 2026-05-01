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
- `examples/automation/`

## Recommended pattern

1. log in once as an admin and mint a dedicated service token
2. store that plaintext token in a secure env file on the machine running automation
3. call Peanut protected APIs with `Authorization: Bearer <pst_...>`
4. revoke and replace the token when the automation no longer needs access

## Good fits for service-token automation

- Data API export jobs
- bounded table maintenance jobs
- storage metadata checks
- internal deploy hooks
- operator-only health/readiness probes that need protected endpoints

## What not to automate with this yet

- third-party customer app auth
- workspace delegation
- broad user impersonation
- unbounded SQL-style database access

## Environment file example

Use the committed sample as a starting point:
- `examples/automation/peanut.env.sample`

Example machine-local file:

```bash
BASE_URL=http://127.0.0.1:3000
SERVICE_TOKEN=pst_replace_me
TABLE_NAME=ops_todos
STORAGE_BUCKET=assets
STORAGE_KEY=ops/hello.txt
AUTOMATION_OUT_DIR=/opt/peanut/backups
```

## Runnable automation examples

Committed scripts:
- `examples/automation/export-ops-todos.sh`
- `examples/automation/check-storage-head.sh`

Example:

```bash
cp examples/automation/peanut.env.sample /opt/peanut/peanut.env
$EDITOR /opt/peanut/peanut.env

PEANUT_ENV_FILE=/opt/peanut/peanut.env \
  ./examples/automation/export-ops-todos.sh

PEANUT_ENV_FILE=/opt/peanut/peanut.env \
  ./examples/automation/check-storage-head.sh
```

## Cron example

```cron
15 2 * * * PEANUT_ENV_FILE=/opt/peanut/peanut.env /opt/peanut/examples/automation/export-ops-todos.sh
*/30 * * * * PEANUT_ENV_FILE=/opt/peanut/peanut.env /opt/peanut/examples/automation/check-storage-head.sh
```

## Recommended rollout flow

For a full bootstrap path, follow:
1. `examples/service-tokens/create-token.sh` or `create-token-jq.sh`
2. `examples/operations-e2e/`
3. copy `examples/automation/peanut.env.sample` to a machine-local secret file and paste the plaintext token
4. convert the validated sequence into your local cron/systemd/CI job

## Rotation guidance

- prefer one token per automation purpose
- keep token names explicit, e.g. `nightly-export`, `storage-head-check`, `deploy-hook`
- revoke old tokens instead of reusing one token for every job
- if a machine is retired, revoke all related tokens immediately
