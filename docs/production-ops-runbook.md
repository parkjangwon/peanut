# Peanut Production Ops Runbook

This runbook is the default operating path for the current single-node SQLite and local storage deployment.

## Pre-Upgrade Checklist

- Confirm readiness:
  - `curl -fsS "$BASE_URL/api/ready"`
- Confirm platform diagnostics:
  - `curl -fsS "$BASE_URL/api/admin/ops/diagnostics" -H "Authorization: Bearer $ADMIN_TOKEN"`
- Create a SQLite backup through the admin API:
  - `curl -fsS -X POST "$BASE_URL/api/admin/backups" -H "Authorization: Bearer $ADMIN_TOKEN"`
- Confirm no restore is pending:
  - `curl -fsS "$BASE_URL/api/admin/backups/restore-pending" -H "Authorization: Bearer $ADMIN_TOKEN"`
- Back up the whole `./data` directory when using the default Docker compose layout.

The account used for backup download or restore scheduling must have the
`owner` admin role. Developer, operator, and viewer roles must not be used for
restore operations.

## Docker Compose Verification

Run the compose verifier after changing Dockerfile, compose config, runtime
settings, deployment docs, or before promoting a new image:

```bash
COMPOSE_FILES="docker-compose.yml docker-compose.build.yml" \
JWT_SECRET=replace-with-a-long-random-secret \
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret \
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
scripts/verify-compose.sh
```

Without `PEANUT_ADMIN_TOKEN` or bootstrap credentials, the verifier only checks
compose startup, readiness, and Deno availability. A production gate run must
include either `PEANUT_ADMIN_TOKEN` or `PEANUT_BOOTSTRAP_EMAIL` plus
`PEANUT_BOOTSTRAP_PASSWORD`.

A passing production gate proves:

- app A/B can coexist with isolated auth users, keys, data, storage, functions, and push state
- the same email can exist in two apps, while duplicate email in one app is rejected
- app A credentials cannot read or write app B Data, Storage, or Function endpoints
- disabling an app blocks SDK traffic and enabling it restores SDK traffic
- backup create/download and restore-pending schedule/read/clear work
- `/api/ready` is clean after restore-pending is cleared

## App Isolation Minimum

Peanut is pre-public and does not expose legacy global API routes. Production traffic should use only app-scoped routes:

- Auth: `/api/apps/:app_id/auth/...`
- Data: `/api/apps/:app_id/data/...`
- Storage: `/api/apps/:app_id/storage/...`
- Push: `/api/apps/:app_id/push/...`
- Functions: `/api/apps/:app_id/functions/...`

Before opening traffic after a deploy, confirm `/api/ready` reports `"ready": true` and `/api/admin/ops/diagnostics` reports `"ok": true`. These checks verify the default app, app_id columns, app-scoped unique indexes, and duplicate invariant checks.

The embedded console is part of the binary and should be treated as the primary
self-hosted operator surface. The Functions tab must support
create/edit/lint/dry-run/invoke/version rollback/invocation retry, and the
Operations tab must expose readiness, diagnostics, metrics, backup download,
and pending restore state without requiring ad hoc JSON inspection.

The console supports English and Korean. Operators should verify both locales
after console changes because the static export is embedded into the Rust
binary.

## Workspace Operations

Run workspace setup as invite-only:

1. Create a workspace invite with `POST /api/admin/workspace-invites`.
2. Have the workspace owner call `POST /api/workspace-invites/accept`.
3. Confirm the workspace appears in `GET /api/workspaces`.
4. Confirm `GET /api/workspaces/:workspace_id/resource-usage` returns the `self_hosted_default` limit profile.
5. Adjust only the specific resource limit needed with `POST /api/workspaces/:workspace_id/resource-limits`.

If a workspace hits `resource_limit_exceeded`, prefer increasing the narrow
resource limit rather than raising every limit. Keep the JSON response in
incident notes because it includes `resource_key`, `used`, and `limit`.

## Migration Failure Rollback

Peanut does not use down migrations in production. Rollback is backup based:

1. Stop writes by stopping the service or removing external traffic.
2. Restore the pre-upgrade `./data` directory, or schedule a DB restore:
   - `curl -fsS -X POST "$BASE_URL/api/admin/backups/<backup>.backup/restore" -H "Authorization: Bearer $ADMIN_TOKEN" -H "Content-Type: application/json" --data '{"confirmation":"<backup>.backup","reason":"rollback"}'`
3. Restart Peanut so the restore marker is applied.
4. Confirm `/api/ready` returns ready.
5. Pin `PEANUT_IMAGE` to the previous known-good image and run `docker compose up -d`.

Do not pull or deploy a newer image while `/api/admin/backups/restore-pending` reports a pending restore.

Restore scheduling requires the confirmation field to exactly match the backup
file name. This is intentional friction so an operator cannot schedule a
restore by accidentally clicking or replaying a stale request.

## Backup Cadence and Restore Drill

- Create an API backup before every deploy.
- Archive the whole `./data` directory before schema-changing upgrades.
- Run a restore drill at least monthly on a non-production copy of the data.
- After every restore drill, run `scripts/verify-compose.sh` before declaring the drill complete.
- Store `JWT_SECRET` and `FUNCTIONS_SECRETS_MASTER_KEY` outside the host so a host rebuild can recover sessions and encrypted function secrets when intended.

## Incident Checklist

- Capture logs:
  - `docker compose logs --tail=300 peanut`
- Check readiness and ops metrics:
  - `/api/ready`
  - `/api/admin/ops/metrics`
  - `/api/admin/ops/diagnostics`
- For Functions issues:
  - confirm `deno --version` inside the container
  - run the Function editor lint endpoint with a minimal handler
- For Push issues:
  - check `/api/apps/:app_id/push/diagnostics`
  - inspect retry backlog and terminal failure reasons
- For Storage issues:
  - confirm `STORAGE_DIR` is writable
  - verify bucket policy before debugging object writes

## Recovery Defaults

- Prefer restoring the full `./data` directory for filesystem or SQLite corruption.
- Prefer API-managed backup restore for known-good SQLite backup rollback.
- Keep at least one previous image tag available in the deployment environment.
- After restore, run `scripts/verify-compose.sh` before reopening traffic.
