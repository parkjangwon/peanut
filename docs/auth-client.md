# Peanut Auth Client Guide

Peanut auth is app-scoped. Client and server integrations must call
`/api/apps/:app_id/auth/...` with an `X-Peanut-Api-Key`; JWTs also carry the
same `app_id`, and app-mismatched bearer tokens are rejected.

## Bootstrap

Fresh installations have no admin token yet. Create the first platform admin
once:

```http
POST /api/bootstrap/admin
content-type: application/json

{
  "email": "owner@example.com",
  "password": "password123"
}
```

The response is the normal login response. After the first admin exists, this
endpoint returns `409`.

## App Auth Routes

- `POST /api/apps/:app_id/auth/register`
- `POST /api/apps/:app_id/auth/login`
- `POST /api/apps/:app_id/auth/refresh`
- `POST /api/apps/:app_id/auth/logout`
- `GET /api/apps/:app_id/auth/me`
- `GET /api/apps/:app_id/auth/public-config`
- `GET /api/apps/:app_id/auth/oauth/:provider/start`
- `GET /api/apps/:app_id/auth/oauth/:provider/callback`

All app auth routes except bootstrap require:

```http
X-Peanut-Api-Key: <client-or-server-key>
```

Protected routes also require:

```http
Authorization: Bearer <access_token>
```

## Login Response

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_at": "2026-05-01T12:00:00Z",
  "user": {
    "id": "uuid",
    "app_id": "default",
    "email": "owner@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

Use the newly returned refresh token after every refresh. If refresh fails,
discard the local session and send the user through login again.

## Client Policy

When `AUTH_ALLOWED_ORIGINS` or `AUTH_ALLOWED_CLIENT_IDS` is configured, browser
auth requests must send a matching `Origin` and/or `x-peanut-client-id`.
