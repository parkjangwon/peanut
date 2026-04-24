# Peanut

Peanut is an early-stage single-binary backend platform prototype built with Rust and a small embedded Next.js console.
The project combines:

- an Axum HTTP API
- SQLite persistence through SQLx
- Argon2 password hashing and JWT-based authentication
- localized health responses (English/Korean)
- a background push worker intended for ntfy/Web Push delivery
- a static admin console exported from Next.js and embedded into the Rust binary

Important: the repository is currently a prototype in progress. The code expresses the intended architecture clearly, but the current `master` branch does not build successfully yet.

## What Peanut is trying to do

Peanut appears to target a very simple self-hostable backend runtime with an opinionated shape:

1. run as a single service on port `3000`
2. persist users and tokens in SQLite
3. store uploaded objects on the local filesystem
4. expose authenticated API routes for account and storage operations
5. serve a built-in web console from the same binary
6. process push notifications in the background

In short: Peanut is aiming for “small backend platform, minimal moving parts, single binary deployment”.

## Current repository status

The repository already contains meaningful backend and frontend code, but several integration points are unfinished.

### What already exists

Backend:
- health endpoint with i18n support
- register/login flow
- password hashing with Argon2
- JWT creation and verification
- SQLite initialization + migration runner
- background push worker for queue polling
- embedded static file server for console assets

Frontend:
- minimal dark dashboard UI in Next.js App Router
- static export configuration (`next.config.mjs`)
- basic dashboard cards for system/storage/push queue

Infra:
- multi-stage Dockerfile
- `docker-compose.yml`
- `scripts/build.sh` for frontend + backend build flow

### What is incomplete or inconsistent

The codebase does not currently compile because the implementation is ahead of the checked-in modules/assets.

Confirmed build issues from `cargo test`:

1. missing `storage` module
   - `src/main.rs` declares `mod storage;`
   - no `src/storage.rs` or `src/storage/mod.rs` exists

2. missing `api::storage` handlers
   - `src/main.rs` routes `/storage/*key`
   - `src/api/mod.rs` only exports `health` and `auth`

3. embedded console output directory is missing before Rust build
   - `src/console.rs` embeds `peanut-console/out/`
   - that folder is not present until the Next.js export step runs

4. router state types are inconsistent
   - auth handlers currently extract `State<SqlitePool>`
   - the app is initialized with a custom `AppState`
   - this causes an Axum state mismatch at compile time

5. database schema is incomplete for push features
   - migrations create `users` and `refresh_tokens`
   - push worker expects `push_queue` and `push_subscriptions`
   - those tables are not defined yet

6. environment handling is only partially wired
   - `docker-compose.yml` sets `DATABASE_URL`
   - `src/main.rs` currently hardcodes `sqlite://peanut.db`
   - `dotenvy` is listed as a dependency but not actually used in startup

Because of those gaps, the repository should currently be treated as a solid architectural prototype rather than a runnable release.

## Architecture overview

### 1. HTTP server and routing

The server is started from `src/main.rs`.

Public routes:
- `GET /api/health`
- `POST /api/register`
- `POST /api/login`

Protected routes (JWT middleware):
- `GET /api/me`
- intended storage routes under `/api/storage/*key`

Fallback:
- all other paths are handled by the embedded console asset server in `src/console.rs`

### 2. Authentication model

Authentication is implemented in three layers:

- `src/auth/hash.rs`
  - hashes passwords with Argon2
  - verifies password hashes

- `src/auth/jwt.rs`
  - creates 15-minute JWT access tokens
  - embeds `sub`, `exp`, and `is_admin`

- `src/middleware/auth.rs`
  - reads `Authorization: Bearer <token>`
  - validates using a hardcoded secret (`temp_secret`)
  - injects claims into request extensions

Notable behavior:
- the first registered user becomes admin and active automatically
- later users are created inactive and appear to require approval
- JWT secret management is not production-ready yet because the secret is hardcoded

### 3. Database layer

`src/db.rs` initializes SQLite with:
- WAL journal mode
- `synchronous = NORMAL`
- foreign keys enabled
- SQLx migrations from `./migrations`

Current migration contents:
- `users`
- `refresh_tokens`

This gives Peanut a simple local-first persistence model with low operational overhead.

### 4. Push delivery design

Push-related code lives in `src/push/`.

Files:
- `worker.rs` — polls pending jobs from the database every 5 seconds
- `ntfy.rs` — sends notifications to `https://ntfy.sh/<topic>`
- `webpush.rs` — placeholder for real Web Push support

Observed intent:
- queue notifications in the DB
- fetch per-user subscriptions
- deliver through ntfy now, Web Push later
- mark jobs as `sent` or `failed`

Current limitation:
- queue/subscription tables are not yet migrated
- `worker.rs` treats `endpoint` as the ntfy topic, which is practical for prototyping but not a full Web Push model yet

### 5. Embedded console

The console lives in `peanut-console/` and is a small Next.js app.

Current UI characteristics:
- dark, minimalist dashboard
- static summary cards
- no live API integration yet
- exported static assets are intended to be bundled into the Rust binary via `rust-embed`

This is a good fit for Peanut’s single-binary idea: build the frontend once, then serve it directly from the backend binary.

## Repository layout

```text
.
├── Cargo.toml                  # Rust app and dependencies
├── Dockerfile                  # Multi-stage frontend/backend image build
├── docker-compose.yml          # Local container run config
├── migrations/                 # SQLite schema migrations
├── scripts/build.sh            # Build frontend export + Rust release binary
├── src/
│   ├── main.rs                 # App bootstrap and route wiring
│   ├── db.rs                   # SQLite init + migrations
│   ├── console.rs              # Embedded static asset server
│   ├── i18n.rs                 # Translation helper
│   ├── api/
│   │   ├── auth.rs             # register/login handlers
│   │   ├── health.rs           # localized health endpoint
│   │   └── mod.rs
│   ├── auth/
│   │   ├── hash.rs             # Argon2 helpers
│   │   ├── jwt.rs              # JWT helpers
│   │   └── mod.rs
│   ├── middleware/
│   │   ├── auth.rs             # bearer token middleware
│   │   └── mod.rs
│   └── push/
│       ├── worker.rs           # notification queue processor
│       ├── ntfy.rs             # ntfy sender
│       ├── webpush.rs          # placeholder web push sender
│       └── mod.rs
├── locales/
│   ├── en.json
│   └── ko.json
└── peanut-console/
    ├── src/app/page.tsx        # minimalist dashboard page
    ├── src/app/layout.tsx      # app layout
    ├── src/app/globals.css     # base styling
    └── next.config.mjs         # static export config
```

## API summary

### `GET /api/health`
Returns a localized JSON health payload.

Example:
```json
{
  "status": "ok",
  "message": "Systems are operational."
}
```

Localization source:
- `Accept-Language: en-*` -> English
- `Accept-Language: ko-*` -> Korean

### `POST /api/register`
Registers a new user.

Request body:
```json
{
  "email": "user@example.com",
  "password": "secret"
}
```

Behavior:
- first user becomes admin and active
- later users are inactive by default

### `POST /api/login`
Logs in an active user and returns a JWT token as plain text.

Request body:
```json
{
  "email": "user@example.com",
  "password": "secret"
}
```

### `GET /api/me`
Protected endpoint that reads JWT claims and returns a plain string response.

## Build and run

## Prerequisites

- Rust toolchain
- Node.js / npm
- SQLite development libraries if building locally on Linux

### Intended build flow

```bash
./scripts/build.sh
```

This script is designed to:
1. install frontend dependencies
2. export the Next.js console
3. build the Rust release binary

### Docker flow

```bash
docker compose up --build
```

### Current reality

At the moment, the build does not complete successfully on the repository head because of the missing storage implementation and state/schema mismatches listed above.

## Strengths of the current design

Even in prototype form, the repository has a few strong ideas:

- good technology fit for a small self-hosted service
- Rust + SQLite keeps operations simple
- frontend embedding supports single-binary deployment
- auth and i18n are already separated into clean modules
- push delivery is designed as an async worker, which is the right shape for notifications
- Dockerfile already reflects the final intended distribution model

## Recommended next steps

If development continues, the most important fixes are:

1. implement `src/storage/` and `src/api/storage.rs`
2. unify Axum state handling around `AppState`
3. load config from environment variables (`DATABASE_URL`, JWT secret, storage path, bind address)
4. add migrations for `push_queue` and `push_subscriptions`
5. make frontend export part of a deterministic build pipeline
6. replace hardcoded `temp_secret` with a real secret source
7. return structured JSON for auth endpoints instead of plain strings
8. connect the console to real backend metrics and storage/push data

## Codebase notes

Approximate hand-written code footprint in the current repository (excluding dependency folders):
- Rust: ~517 lines across 16 files
- TS/TSX: ~73 lines across 3 files
- CSS: 14 lines
- SQL: 16 lines

The generated `package-lock.json` is much larger and not representative of application complexity.

## Development note for contributors

If you want this repository to become a working MVP quickly, the shortest path is:
- finish storage
- fix state typing
- add missing push migrations
- build/export console assets during release builds

Once those are complete, Peanut can become a coherent single-node backend starter instead of just an architectural prototype.

## License

No license file is currently present in this repository. Add one before public reuse or distribution.
