# Peanut App-Scoped API

Peanut is not carrying public legacy compatibility routes. Clients and operators should treat app-scoped URLs as the only stable API surface.

## Required App Boundary

Every runtime domain is scoped by `app_id`:

- End-user Auth users are unique by `(app_id, email)`.
- Data table names are unique by `(app_id, name)`.
- Function names and endpoint slugs are unique by app.
- Push subscriptions and queue items are delivered only within their app.
- Storage buckets and SDK object paths include `app_id`.

JWT access tokens include `app_id`. SDK middleware rejects requests where the app key, path app_id, and optional user bearer token do not all match.

## SDK Headers

SDK requests require:

```http
X-Peanut-Api-Key: <client-or-server-key>
Authorization: Bearer <user-access-token>
```

The bearer token is required for user-scoped operations such as `/auth/me`, client uploads, and push subscriptions.

## Routes

- `POST /api/apps/:app_id/auth/register`
- `POST /api/apps/:app_id/auth/login`
- `POST /api/apps/:app_id/auth/refresh`
- `POST /api/apps/:app_id/auth/logout`
- `GET /api/apps/:app_id/auth/me`
- `POST /api/apps/:app_id/auth/change-password`
- `POST /api/apps/:app_id/auth/forgot-password`
- `POST /api/apps/:app_id/auth/reset-password`
- `GET /api/apps/:app_id/auth/sessions`
- `GET /api/apps/:app_id/auth/events`
- `GET /api/apps/:app_id/data/tables`
- `GET|POST /api/apps/:app_id/data/tables/:table/rows`
- `GET|PUT|DELETE /api/apps/:app_id/storage/buckets/:bucket/objects/*key`
- `POST /api/apps/:app_id/functions/endpoints/:endpoint_slug`
- `GET|POST|DELETE /api/apps/:app_id/push/subscriptions`
- `POST /api/apps/:app_id/push/messages`

Admin APIs for app, key, provider, bucket, table, function, queue, diagnostics, and activity management also live under `/api/apps/:app_id/...` where the operation is app-specific.

## Public Beta Control Plane

Peanut's public beta surface is organization-based and invite-only:

- `POST /api/admin/beta-invites` creates a one-time or limited-use invite code.
- `GET /api/admin/beta-invites` lists beta invites without exposing plaintext invite codes.
- `POST /api/beta/signup` consumes an invite and creates an organization owner.
- `GET /api/orgs` lists organizations visible to the signed-in console user.
- `GET /api/orgs/:org_id/usage` returns the assigned plan and quota usage.
- `POST /api/orgs/:org_id/quotas` adjusts a quota for pilot operations.
- `POST /api/admin/orgs/:org_id/suspend|unsuspend` controls organization-level abuse response.
- `POST /api/admin/apps/:app_id/suspend|unsuspend` controls app-level abuse response.

Apps now carry `organization_id`, `suspended_at`, and `suspended_reason`.
`POST /api/apps` accepts an optional `organization_id`; when omitted, Peanut
uses the default organization for compatibility with first-install operations.
App creation is blocked with `code: "quota_exceeded"` when the organization's
`apps` quota is exhausted.
SDK requests are blocked with `organization_suspended` or `app_suspended` when
the corresponding tenant boundary is suspended.

## Readiness

Use:

```bash
curl -fsS "$BASE_URL/api/ready"
curl -fsS "$BASE_URL/api/admin/ops/diagnostics" -H "Authorization: Bearer $ADMIN_TOKEN"
```

The diagnostics response is intended for future console use and should stay structured: each check has `name`, `ok`, and context fields.
