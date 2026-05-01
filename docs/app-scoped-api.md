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

## Workspace Control Plane

Peanut is self-hosted only. Workspaces are internal team/project boundaries
inside one Peanut instance:

- `POST /api/admin/workspace-invites` creates a limited-use setup invite.
- `GET /api/admin/workspace-invites` lists setup invites without plaintext codes.
- `POST /api/workspace-invites/accept` consumes an invite and creates a workspace owner.
- `GET /api/workspaces` lists workspaces visible to the signed-in console user.
- `GET /api/workspaces/:workspace_id/resource-usage` returns resource usage.
- `POST /api/workspaces/:workspace_id/resource-limits` adjusts one resource limit.
- `POST /api/admin/workspaces/:workspace_id/disable|enable` controls workspace access.
- `POST /api/admin/apps/:app_id/disable|enable` controls app access.

Apps carry `workspace_id`, `disabled_at`, and `disabled_reason`. `POST /api/apps`
accepts an optional `workspace_id`; when omitted, Peanut uses the default
workspace. App creation, app-user registration, data row creation, storage
writes, Function invocations, Push sends, and monthly SDK API requests are
blocked with `code: "resource_limit_exceeded"` when their workspace resource
limit is exhausted. SDK requests are blocked with `workspace_disabled` or
`app_disabled` when the corresponding boundary is disabled.

## Readiness

Use:

```bash
curl -fsS "$BASE_URL/api/ready"
curl -fsS "$BASE_URL/api/admin/ops/diagnostics" -H "Authorization: Bearer $ADMIN_TOKEN"
```

The diagnostics response is intended for future console use and should stay structured: each check has `name`, `ok`, and context fields.
