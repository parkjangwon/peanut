# Peanut Migration And Backup Guide

Peanut is still pre-public, so migrations are optimized for a fresh app-scoped schema instead of carrying global legacy compatibility tables.

## Before Migration

1. Stop or drain external writes.
2. Create an API backup:
   ```bash
   curl -fsS -X POST "$BASE_URL/api/admin/backups" -H "Authorization: Bearer $ADMIN_TOKEN"
   ```
3. Back up the full data directory when using local storage:
   ```bash
   tar -czf peanut-data-pre-migration.tgz ./data
   ```
4. Record the image or git SHA being replaced.

## After Migration

Run:

```bash
curl -fsS "$BASE_URL/api/ready"
curl -fsS "$BASE_URL/api/admin/ops/diagnostics" -H "Authorization: Bearer $ADMIN_TOKEN"
scripts/verify-compose.sh
```

The platform diagnostics must show no duplicate user emails, table names, function names, or endpoint slugs within an app.

## Rollback

Peanut uses backup-based rollback:

1. Stop traffic.
2. Restore the pre-migration data directory or schedule an API backup restore.
3. Restart Peanut.
4. Confirm readiness and diagnostics pass.
5. Reopen traffic only after a smoke run passes.
