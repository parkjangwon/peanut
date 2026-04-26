# Peanut Auth Client Guide

Peanut can now act as the auth backend for an external frontend app.

This guide shows:
- which endpoints to call
- how to manage access + refresh tokens
- how to wire a minimal browser client
- how to handle logout, password reset, and session revocation

See also:
- `README.md`
- `docs/auth-client.ko.md`
- `examples/auth-client-web/`

## 1. Auth model

Peanut currently provides:
- `POST /api/register`
- `POST /api/login`
- `POST /api/auth/refresh`
- `POST /api/auth/logout`
- `GET /api/me`
- `POST /api/auth/change-password`
- `POST /api/auth/forgot-password`
- `POST /api/auth/reset-password`
- `GET /api/auth/sessions`
- `GET /api/auth/events`
- `DELETE /api/auth/sessions/:session_id`
- `POST /api/auth/sessions/revoke-all`

Current behavior:
- access tokens are short-lived JWT bearer tokens
- refresh tokens are server-tracked opaque tokens
- refresh tokens rotate on `POST /api/auth/refresh`
- refresh sessions are revoked on logout, password change, password reset, and admin deactivation
- auth events are recorded for register, login, refresh, reset, session revoke, and admin activation/deactivation flows

## 2. Recommended frontend integration shape

### Good default for a simple prototype
- keep the access token in memory
- keep the refresh token in memory if you are building a local demo or internal tool
- call `/api/auth/refresh` when `/api/me` or another protected request returns `401`

### Better production shape
- keep the access token in memory only
- do not put long-lived refresh tokens in `localStorage`
- prefer a small BFF / backend-for-frontend layer that stores the refresh token in a secure cookie or server session
- let the frontend talk to that BFF for login, refresh, and logout

Peanut does not yet set auth cookies for you. The current API is token-oriented, so browser apps should decide whether they are:
- a direct SPA talking to Peanut, or
- a BFF-backed app wrapping Peanut auth

## 3. Core request flow

### Register

```http
POST /api/register
content-type: application/json

{
  "email": "admin@example.com",
  "password": "correct horse battery staple"
}
```

### Login

```http
POST /api/login
content-type: application/json

{
  "email": "admin@example.com",
  "password": "correct horse battery staple"
}
```

Typical response:

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_at": "2026-04-26T12:00:00Z",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### Call a protected route

```http
GET /api/me
authorization: Bearer <access_token>
```

### Refresh

```http
POST /api/auth/refresh
content-type: application/json

{
  "refresh_token": "..."
}
```

Important:
- use the newly returned refresh token after every refresh
- discard the previous refresh token immediately
- if refresh fails with `401`, force the user to sign in again

### Logout

```http
POST /api/auth/logout
content-type: application/json

{
  "refresh_token": "..."
}
```

### Sessions and auth events

```http
GET /api/auth/sessions
authorization: Bearer ***
```

```http
GET /api/auth/events
authorization: Bearer ***
```

Typical event response:

```json
{
  "events": [
    {
      "id": "uuid",
      "user_id": "uuid",
      "actor_user_id": "uuid",
      "action": "user_deactivated",
      "metadata": null,
      "created_at": "2026-04-26 12:00:00"
    }
  ]
}
```

```http
DELETE /api/auth/sessions/<session_id>
authorization: Bearer ***
```

```http
POST /api/auth/sessions/revoke-all
authorization: Bearer ***
```

## 4. Password flows

### Change password

Requires a valid access token:

```http
POST /api/auth/change-password
authorization: Bearer <access_token>
content-type: application/json

{
  "current_password": "old-password",
  "new_password": "new-password-123"
}
```

Effect:
- the password is updated
- existing refresh sessions are revoked
- the current app should send the user through login again

### Forgot / reset password

```http
POST /api/auth/forgot-password
content-type: application/json

{
  "email": "admin@example.com"
}
```

Typical response:

```json
{
  "message": "if the user exists, a reset token was created",
  "reset_token": "...",
  "delivery": "inline"
}
```

Current self-host-first behavior:
- Peanut supports a delivery abstraction controlled by `PASSWORD_RESET_DELIVERY`
- `inline` returns a `reset_token` in the JSON response
- `log` returns an empty token and writes the reset token to the server log instead
- later this can be extended to email, webhook, or an operator flow

```http
POST /api/auth/reset-password
content-type: application/json

{
  "reset_token": "...",
  "new_password": "new-password-123"
}
```

Effect:
- the password is reset
- existing refresh sessions are revoked
- the user must log in again

## 5. Minimal browser client pattern

The example in `examples/auth-client-web/app.js` uses this shape:

```js
const authState = {
  accessToken: null,
  refreshToken: null,
  user: null,
};
```

Recommended behavior:
1. login stores both tokens in memory
2. protected requests send `Authorization: Bearer <access_token>`
3. when a protected request returns `401`, attempt one refresh
4. retry the original request with the new access token
5. if refresh fails, clear auth state and return to sign-in

## 6. Example helper

```js
async function api(path, init = {}, retry = true) {
  const headers = new Headers(init.headers || {});
  if (authState.accessToken) {
    headers.set('Authorization', `Bearer ${authState.accessToken}`);
  }
  if (!headers.has('Content-Type') && init.body) {
    headers.set('Content-Type', 'application/json');
  }

  const response = await fetch(`${baseUrl}${path}`, {
    ...init,
    headers,
  });

  if (response.status === 401 && retry && authState.refreshToken) {
    await refreshSession();
    return api(path, init, false);
  }

  return response;
}
```

## 7. Operational notes for app developers

- The first registered user becomes the active admin automatically.
- Later users may require admin activation before they can log in.
- If an admin deactivates a user, protected Peanut APIs block that user immediately, even if their access token has not expired yet.
- The current API surface is enough for email/password apps, internal tools, and self-host dashboards.
- OAuth, magic link, and MFA are future layers, not part of the current contract.

## 8. What the example demonstrates

`examples/auth-client-web/` demonstrates:
- register
- login
- `GET /api/me`
- refresh-on-401
- logout
- list sessions
- revoke one session
- revoke all sessions
- forgot/reset password

It is intentionally dependency-free so the auth flow is easy to inspect.

## 9. Local demo

1. Start Peanut.
2. Open `examples/auth-client-web/index.html` in a browser.
3. Set the Peanut base URL, usually `http://127.0.0.1:3000`.
4. Register the first admin user or log in with an existing user.
5. Exercise refresh, session management, and password reset flows.

If your browser blocks `file://` fetch calls in your environment, serve the example with any static file server.
