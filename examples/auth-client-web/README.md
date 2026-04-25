# Peanut browser auth example

This example is a tiny dependency-free browser client for Peanut Auth.

It demonstrates:
- register
- login
- authenticated `GET /api/me`
- refresh token rotation
- logout
- session list
- revoke one session
- revoke all sessions
- forgot/reset password

## Run

1. Start Peanut locally.
2. Open `index.html` directly in a browser, or serve this directory with a static file server.
3. Set the Peanut base URL, usually `http://127.0.0.1:3000`.

## Why this example is intentionally simple

- no framework
- no build step
- no dependency install
- auth state stored in memory only

That makes the token flow easy to inspect.

## Production note

For production browser apps, prefer a BFF layer or secure cookie strategy for refresh tokens instead of long-lived browser storage.
