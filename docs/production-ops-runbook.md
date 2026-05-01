# Peanut Production Ops Runbook

This runbook is the default operating path for the current single-node SQLite and local storage deployment.

## Pre-Upgrade Checklist

- Confirm readiness:
  - `curl -fsS "$BASE_URL/api/ready"`
- Create a SQLite backup through the admin API:
  - `curl -fsS -X POST "$BASE_URL/api/admin/backups" -H "Authorization: Bearer $ADMIN_TOKEN"`
- Confirm no restore is pending:
  - `curl -fsS "$BASE_URL/api/admin/backups/restore-pending" -H "Authorization: Bearer $ADMIN_TOKEN"`
- Back up the whole `./data` directory when using the default Docker compose layout.

## Docker Compose Verification

Run the compose verifier after changing Dockerfile, compose config, runtime settings, or deployment docs:

```bash
JWT_SECRET=replace-me docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
PEANUT_ADMIN_TOKEN=... scripts/verify-compose.sh
```

Without `PEANUT_ADMIN_TOKEN`, the verifier still checks compose startup, readiness, and Deno availability.

## Migration Failure Rollback

Peanut does not use down migrations in production. Rollback is backup based:

1. Stop writes by stopping the service or removing external traffic.
2. Restore the pre-upgrade `./data` directory, or schedule a DB restore:
   - `curl -fsS -X POST "$BASE_URL/api/admin/backups/<backup>.backup/restore" -H "Authorization: Bearer $ADMIN_TOKEN"`
3. Restart Peanut so the restore marker is applied.
4. Confirm `/api/ready` returns ready.
5. Pin `PEANUT_IMAGE` to the previous known-good image and run `docker compose up -d`.

Do not pull or deploy a newer image while `/api/admin/backups/restore-pending` reports a pending restore.

## Incident Checklist

- Capture logs:
  - `docker compose logs --tail=300 peanut`
- Check readiness and ops metrics:
  - `/api/ready`
  - `/api/admin/ops/metrics`
- For Functions issues:
  - confirm `deno --version` inside the container
  - run the Function editor lint endpoint with a minimal handler
- For Push issues:
  - check `/api/push/diagnostics`
  - inspect retry backlog and terminal failure reasons
- For Storage issues:
  - confirm `STORAGE_DIR` is writable
  - verify bucket policy before debugging object writes

## Recovery Defaults

- Prefer restoring the full `./data` directory for filesystem or SQLite corruption.
- Prefer API-managed backup restore for known-good SQLite backup rollback.
- Keep at least one previous image tag available in the deployment environment.
- After restore, run `scripts/verify-compose.sh` before reopening traffic.
