# Peanut browser auth example

This example is a tiny dependency-free browser client for Peanut Auth.

It demonstrates:
- register
- login
- app-scoped auth routes under `/api/apps/:app_id/auth/...`
- `x-peanut-api-key` for Peanut SDK auth
- optional `x-peanut-client-id` auth header for auth-client policy testing
- authenticated `GET /api/apps/:app_id/auth/me`
- refresh token rotation
- logout
- session list
- revoke one session
- revoke all sessions
- forgot/reset password

## Run

1. Start Peanut locally.
2. Open `index.html` directly in a browser, or serve this directory with a static file server.
3. Set the Peanut base URL, usually `http://127.0.0.1:3000` for direct local development or `http://127.0.0.1:3492` for the default Docker Compose package.
4. Set the app id, usually `default` for a fresh local install.
5. Create a client or server app key in the Peanut console/API and paste it into the app key field.
6. If Peanut uses `AUTH_ALLOWED_CLIENT_IDS`, also set the client id field before using auth routes.
7. If Peanut uses `AUTH_ALLOWED_ORIGINS`, serve this example from one of those allowed origins instead of opening it as `file://`.

## Why this example is intentionally simple

- no framework
- no build step
- no dependency install
- auth state stored in memory only

That makes the token flow easy to inspect.

## Production note

For production browser apps, prefer a BFF layer or secure cookie strategy for refresh tokens instead of long-lived browser storage.
