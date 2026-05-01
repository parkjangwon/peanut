# Peanut service tokens

Peanut now supports a narrow server-to-server token model for operator automation.

Current scope:
- admin-managed opaque tokens
- one-time plaintext reveal at creation time
- hashed token storage in SQLite
- Bearer auth on existing protected APIs
- admin-only access mode for now

This is intentionally not:
- OAuth client credentials
- dynamic scope matrices
- per-app secret management
- end-user impersonation

See also:
- `README.md`
- `docs/automation-runbook.md`
- `examples/automation/`

## Endpoints

Admin endpoints:
- `GET /api/admin/service-tokens`
- `POST /api/admin/service-tokens`
- `DELETE /api/admin/service-tokens/:token_id`
- curl examples: `examples/service-tokens/`
- jq-assisted examples: `examples/service-tokens/create-token-jq.sh`
- combined runbook: `examples/operations-e2e/`

## Create a token

Request:

```json
{
  "name": "deploy-worker",
  "expires_in_days": 30
}
```

Response:

```json
{
  "service_token": {
    "id": "uuid",
    "name": "deploy-worker",
    "access_mode": "admin",
    "user_id": "uuid",
    "created_at": "2026-04-27 15:00:00",
    "last_used_at": null,
    "expires_at": "2026-05-27 15:00:00",
    "revoked_at": null
  },
  "token": "pst_..."
}
```

Important:
- copy the plaintext `token` immediately
- Peanut stores only the hash, so the raw token is not recoverable later
- see `examples/service-tokens/` for copy-pasteable curl files
- if `jq` is available, `examples/service-tokens/create-token-jq.sh` prints ready-to-paste export lines

## Use a token

Use the token as a normal bearer token against protected APIs:

```bash
curl -s "$BASE_URL/api/admin/users" \
  -H "authorization: Bearer pst_..."
```

The same token can also call other protected routes that require admin access, such as Data API admin operations.

## List and revoke

List tokens:

```bash
curl -s "$BASE_URL/api/admin/service-tokens" \
  -H "authorization: Bearer $ADMIN_JWT"
```

Revoke a token:

```bash
curl -s -X DELETE "$BASE_URL/api/admin/service-tokens/$TOKEN_ID" \
  -H "authorization: Bearer $ADMIN_JWT"
```

## Current rules

- only admins can create/list/revoke service tokens
- tokens currently have fixed `access_mode=admin`
- revoked tokens stop working immediately
- expired tokens stop working automatically
- `last_used_at` is updated on successful use

## Practical use cases

For cron/systemd/CI-style operator automation, see `docs/automation-runbook.md` and `examples/automation/`.


Good fits:
- deploy hooks
- backup/export workers
- internal admin automation
- cron jobs that need Data API or storage administration

Not a good fit yet:
- customer-facing third-party app auth
- workspace-scoped app client management
- fine-grained per-route permission control
