# Peanut Auth Platform Implementation Plan

> For Hermes: use subagent-driven-development style execution, keep scope narrow per phase, and follow TDD for every behavior change.

Goal: turn Peanut auth from a console/login core into an external-app-capable auth product that frontend apps can use directly for signup, login, session refresh, logout/revoke, and password lifecycle flows.

Architecture:
- Keep Peanut self-host-first and bounded: SQLite-backed auth state, JWT access tokens for API access, opaque refresh/reset tokens stored server-side, and simple email/password identity first.
- Reuse the existing user/admin model, but add explicit auth session tables and password-reset primitives so external apps can safely manage long-lived sessions.
- Deliver in phases so each slice is releasable and testable: Phase 1 session lifecycle, Phase 2 password lifecycle, Phase 3 external-app polish and auth product surface.

Tech stack:
- Rust + Axum + SQLx + SQLite
- Existing JWT auth middleware
- Existing embedded Next.js console for operational visibility

---

## Product target

When this plan is complete, Peanut Auth should support:
- external frontend apps using Peanut as their auth backend
- short-lived access token + refresh token session model
- logout/revoke support
- password change for authenticated users
- password reset request/confirm flow
- admin visibility into users and auth state
- docs/examples clear enough that a frontend developer can wire login without reading server code

## Non-goals for this milestone
- OAuth / social login
- magic links
- MFA / TOTP
- multi-tenant organizations / projects
- per-app auth client records
- email sending infrastructure beyond reset-token contract (initially expose reset token in bounded self-host-friendly form or log/dev-mode response depending implementation choice)

## Current codebase facts
- Users table already exists in `migrations/202604240001_create_users.sql`
- A `refresh_tokens` table already exists in the same migration but is currently unused
- Current auth API only supports register/login/me
- Access token is JWT-only, 15 minute TTL, no refresh path
- Middleware re-checks user active/admin state on every request
- Admin activation/deactivation already exists

## Phase 1 scope (implement now)
- `POST /api/register`
- `POST /api/login`
  - now returns both access token and refresh token
- `POST /api/auth/refresh`
- `POST /api/auth/logout`
- `POST /api/auth/change-password`
- `POST /api/auth/forgot-password`
- `POST /api/auth/reset-password`
- refresh token rotation + revoke on logout/password change/reset
- password reset token storage in SQLite
- README / README.ko / `.env.example` updates

## Phase 2 scope (next)
- auth session list/revoke endpoints for admin/user visibility
- console auth management UI for refresh/logout/password reset helpers
- frontend integration reference snippets
- optional CORS/auth-origin tightening for browser apps

## File plan for Phase 1
- Create: `migrations/202604260001_add_auth_session_tables.sql`
- Modify: `src/api/auth.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/db.rs` tests if table assertions expand
- Modify: `src/test_support.rs` if helpers are needed
- Modify: `README.md`
- Modify: `README.ko.md`
- Modify: `.env.example`

## Data model changes

### New / changed tables
1. `refresh_tokens`
- keep table but migrate to support:
  - `token_hash TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL`
  - `session_id TEXT NOT NULL`
  - `expires_at DATETIME NOT NULL`
  - `revoked_at DATETIME NULL`
  - `replaced_by_token_hash TEXT NULL`
  - `created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP`

2. `password_reset_tokens`
- `token_hash TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL`
- `expires_at DATETIME NOT NULL`
- `consumed_at DATETIME NULL`
- `created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP`
- FK to users cascade delete

3. optional helper index(es)
- index on `refresh_tokens.user_id`
- index on `password_reset_tokens.user_id`

## API contract target for Phase 1

### `POST /api/login`
Response:
```json
{
  "access_token": "...",
  "refresh_token": "opaque-token",
  "token_type": "Bearer",
  "expires_at": "2026-04-26T00:00:00Z",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### `POST /api/auth/refresh`
Request:
```json
{ "refresh_token": "opaque-token" }
```
Response:
```json
{
  "access_token": "...",
  "refresh_token": "rotated-opaque-token",
  "token_type": "Bearer",
  "expires_at": "2026-04-26T00:10:00Z",
  "user": { ... }
}
```

### `POST /api/auth/logout`
Request:
```json
{ "refresh_token": "opaque-token" }
```
Response:
```json
{ "message": "logged out" }
```

### `POST /api/auth/change-password`
Protected endpoint.
Request:
```json
{
  "current_password": "old-secret",
  "new_password": "new-secret-123"
}
```
Response:
```json
{ "message": "password updated" }
```
Behavior:
- verify current password
- update hash
- revoke all existing refresh tokens for the user

### `POST /api/auth/forgot-password`
Request:
```json
{ "email": "user@example.com" }
```
Response:
```json
{
  "message": "if the user exists, a reset token was created",
  "reset_token": "dev-or-self-host-token"
}
```
Notes:
- for self-host practicality, return token in response for now rather than pretending email is wired
- later can gate response exposure behind config / dev mode

### `POST /api/auth/reset-password`
Request:
```json
{
  "reset_token": "opaque-reset-token",
  "new_password": "new-secret-123"
}
```
Response:
```json
{ "message": "password reset complete" }
```
Behavior:
- consume reset token once
- update password hash
- revoke all refresh tokens for the user

## Detailed implementation tasks

### Task 1: Add failing DB migration expectation test
Objective: prove the database must contain password reset token storage.

Files:
- Modify: `src/db.rs`

Steps:
1. Add a failing test asserting `password_reset_tokens` exists after `init_db`.
2. Run:
   `cargo test test_db_init_creates_push_and_data_tables -- --nocapture`
   Expected: FAIL because table missing.
3. Later update this test to pass after migration is added.

### Task 2: Add failing auth API tests for refresh/logout/password flows
Objective: pin expected platform-auth behavior before implementation.

Files:
- Modify: `src/api/auth.rs`

Tests to add:
1. login returns refresh token
2. refresh rotates refresh token and returns new access token
3. logout revokes refresh token
4. change-password requires current password and revokes old sessions
5. forgot-password creates reset token
6. reset-password consumes token and allows new login

Run:
`cargo test auth:: -- --nocapture`
Expected: FAIL with missing structs/routes/helpers.

### Task 3: Add auth migration
Objective: persist auth session and reset state.

Files:
- Create: `migrations/202604260001_add_auth_session_tables.sql`

Implementation notes:
- migrate legacy `refresh_tokens.token` to `token_hash` shape safely for fresh DBs
- prefer additive migration compatible with current repo state
- create `password_reset_tokens`
- add indexes

Verification:
`cargo test db:: -- --nocapture`

### Task 4: Add auth token helpers
Objective: centralize opaque token generation + hashing + expiry helpers.

Files:
- Modify: `src/api/auth.rs`
  or extract internal helper section there

Implementation:
- `generate_opaque_token() -> String`
- `hash_opaque_token(token: &str) -> String`
- `issue_refresh_token(...)`
- `revoke_refresh_token(...)`
- `revoke_all_refresh_tokens_for_user(...)`
- `issue_password_reset_token(...)`
- `consume_password_reset_token(...)`

Constraints:
- never store raw opaque tokens in DB
- store hash only
- compare by hash

### Task 5: Extend login response to include refresh token
Objective: make external-app login usable without custom session layer.

Files:
- Modify: `src/api/auth.rs`

Implementation:
- extend `LoginResponse`
- on successful login create refresh session row
- return refresh token to caller

Verification:
`cargo test test_register_login_and_me_return_structured_json -- --nocapture`
plus new login test.

### Task 6: Implement refresh endpoint with rotation
Objective: enable long-lived browser/mobile sessions.

Files:
- Modify: `src/api/auth.rs`
- Modify: `src/main.rs`

Implementation:
- add `RefreshTokenRequest`
- validate token hash against active row
- reject expired/revoked token
- rotate token on use
- mint new JWT + new refresh token
- optionally mark replaced token row

Verification:
`cargo test test_refresh_rotates_token -- --nocapture`

### Task 7: Implement logout endpoint
Objective: allow clients to explicitly end session.

Files:
- Modify: `src/api/auth.rs`
- Modify: `src/main.rs`

Implementation:
- add `LogoutRequest`
- revoke matching refresh token by hash
- idempotent success response is acceptable

Verification:
`cargo test test_logout_revokes_refresh_token -- --nocapture`

### Task 8: Implement change-password endpoint
Objective: support authenticated account password changes.

Files:
- Modify: `src/api/auth.rs`
- Modify: `src/main.rs`

Implementation:
- protected route
- verify current password
- validate new password
- update `users.password_hash`
- revoke all refresh tokens for user

Verification:
`cargo test test_change_password_revokes_existing_sessions -- --nocapture`

### Task 9: Implement forgot/reset password endpoints
Objective: allow external apps to drive password recovery.

Files:
- Modify: `src/api/auth.rs`
- Modify: `src/main.rs`

Implementation:
- `forgot-password` should not leak user existence in message
- create short-lived reset token if user exists
- return token in response for self-host practicality in this phase
- `reset-password` consumes token once, updates password, revokes refresh sessions

Verification:
`cargo test test_forgot_and_reset_password_flow -- --nocapture`

### Task 10: Update docs and environment examples
Objective: explain Peanut Auth as external-app auth surface.

Files:
- Modify: `README.md`
- Modify: `README.ko.md`
- Modify: `.env.example`

Doc additions:
- login/refresh/logout/change/reset endpoints
- external frontend app flow summary
- note that current reset-token delivery is self-host/dev-first and can later be swapped with mail delivery

### Task 11: Final verification
Objective: prove the platform is still healthy.

Run:
- `cargo fmt`
- `cargo test`
- `cd peanut-console && npm run lint`
- `cd peanut-console && npm run build`

### Task 12: Commit
```bash
git add migrations/202604260001_add_auth_session_tables.sql src/api/auth.rs src/api/mod.rs src/main.rs src/db.rs README.md README.ko.md .env.example
git commit -m "feat: add external app auth session flows"
```

## Design decisions to preserve
- Keep auth self-host practical, not SaaS-clone magic.
- Prefer simple opaque refresh/reset tokens over more JWT complexity.
- Return reset token in this milestone rather than faking email delivery.
- Revoke refresh sessions on deactivate, password change, and password reset.
- Do not add OAuth/MFA/etc. until the email/password product surface is genuinely solid.

## Acceptance criteria for Phase 1
- external frontend can sign up, log in, refresh session, log out, and reset password using Peanut only
- access tokens stay short-lived
- refresh tokens are server-tracked and revocable
- password changes/resets invalidate old sessions
- docs clearly describe Peanut Auth as usable for external frontend apps
