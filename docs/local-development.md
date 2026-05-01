# Peanut Local Development Guide

This guide runs Peanut directly on your machine. It is the right workflow for
backend API work, console development, and local smoke testing before building a
Docker image.

## Requirements

- Rust stable toolchain
- Node.js and npm for the embedded console
- SQLite support through SQLx
- Deno when `FUNCTIONS_ENABLED=true`

Check the basics:

```bash
cargo --version
node --version
npm --version
deno --version
```

## Environment

Use explicit environment variables so local behavior matches Docker:

```bash
export JWT_SECRET="$(openssl rand -hex 32)"
export FUNCTIONS_SECRETS_MASTER_KEY="$(openssl rand -hex 32)"
export DATABASE_URL="sqlite://peanut.dev.db"
export STORAGE_DIR="data/storage"
export BIND_ADDR="127.0.0.1:3000"
export MAX_UPLOAD_BYTES="5242880"
export FUNCTIONS_ENABLED="true"
export FUNCTIONS_ALLOW_NETWORK="false"
export FUNCTIONS_WORK_DIR="/tmp/peanut-functions"
export TRUST_PROXY_HEADERS="false"
export RUST_LOG="info"
```

`JWT_SECRET` signs access tokens. Changing it invalidates existing sessions.
`FUNCTIONS_SECRETS_MASTER_KEY` encrypts Function secrets; use a stable value if
you need previously saved secrets to keep decrypting.

## Build the Embedded Console

Peanut serves the exported Next.js console from the Rust binary. Rebuild it
before checking the real single-binary experience:

```bash
cd console
npm install
npm run build
cd ..
```

During UI-only work you may also run:

```bash
cd console
npm run dev
```

The Next dev server is only for UI iteration. The production path is
`npm run build` plus the Rust service serving `/`.

## Run the Server

```bash
cargo run
```

Open:

```text
http://127.0.0.1:3000
```

The first visit can create the platform admin through the console. The same
bootstrap is available by API:

```bash
curl -s -X POST "http://127.0.0.1:3000/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

After an admin exists, bootstrap returns `409`. Sign in through the console or:

```bash
curl -s -X POST "http://127.0.0.1:3000/api/admin/auth/login" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

## Public Beta Flow

Create a beta invite as a platform admin:

```bash
curl -s -X POST "$BASE_URL/api/admin/beta-invites" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"label":"local pilot","max_uses":1}'
```

Use the returned `invite_code` to create an organization owner:

```bash
curl -s -X POST "$BASE_URL/api/beta/signup" \
  -H "content-type: application/json" \
  --data '{"invite_code":"pbi_...","organization_name":"Local Pilot","email":"founder@example.com","password":"password123"}'
```

Every organization receives the `beta_free` plan. Usage and quota inspection:

```bash
curl -s "$BASE_URL/api/orgs/$ORG_ID/usage" \
  -H "authorization: Bearer $ADMIN_TOKEN"
```

## App Key and SDK Smoke

Create a server key:

```bash
curl -s -X POST "$BASE_URL/api/apps/default/keys" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"name":"local server","key_type":"server"}'
```

Register and log in an app user:

```bash
curl -s -X POST "$BASE_URL/api/apps/default/auth/register" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "content-type: application/json" \
  --data '{"email":"user@example.com","password":"password123"}'

curl -s -X POST "$BASE_URL/api/apps/default/auth/login" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "content-type: application/json" \
  --data '{"email":"user@example.com","password":"password123"}'
```

## Verification

Run the full backend and console checks before committing:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings

cd console
npm run lint
npm run build
cd ..

bash -n scripts/verify-compose.sh
```

For a browser sanity check of the exported console:

```bash
python3 -m http.server 4174 -d console/out
```

Open `http://localhost:4174/index.html`, then stop the server with `Ctrl-C`.

## Reset Local State

Stop the server first, then remove local state:

```bash
rm -f peanut.dev.db peanut.dev.db-*
rm -rf data/storage /tmp/peanut-functions
```

Do not run destructive cleanup against a production `DATABASE_URL` or
`STORAGE_DIR`.
