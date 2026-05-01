# Backend Platform Core Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add the backend platform primitives needed for a Supabase-like Peanut console without building the console yet.

**Architecture:** Introduce an `apps` domain as the common product boundary, then thread `app_id` through Auth, Data, Storage, Push, Functions, credentials, diagnostics, and audit events. Preserve current API behavior by creating a default app and keeping legacy routes mapped to that default app while adding app-scoped routes for the future console and SDKs.

**Tech Stack:** Rust, Axum, SQLx SQLite migrations, local filesystem storage, Deno Functions runtime, existing JWT/service-token auth, existing integration/unit test harness.

---

## Guiding Rules

- Keep old routes working by mapping them to a `default` app until the console and SDK can move to app-scoped routes.
- Every new user-created resource gets an `app_id`, including tables, rows, buckets, push subscriptions, functions, invocations, provider settings, keys, and audit events.
- Do not make the console yet. Build JSON APIs that a console can consume directly.
- Prefer additive migrations. Backfill existing rows into the default app.
- Treat “project” as the user-facing concept and `apps` as the internal table/API noun unless we later rename globally.
- Add tests before implementation for every slice.

## Target Route Shape

Keep current routes:

```text
/api/register
/api/login
/api/storage/*
/api/s3/:bucket/*
/api/functions/*
/api/push/*
/api/admin/*
```

Add future-proof app-scoped routes:

```text
/api/apps
/api/apps/:app_id
/api/apps/:app_id/keys
/api/apps/:app_id/auth/providers
/api/apps/:app_id/storage/buckets
/api/apps/:app_id/storage/buckets/:bucket/policy
/api/apps/:app_id/functions/:name/lint
/api/apps/:app_id/functions/:name/test
/api/apps/:app_id/push/diagnostics
/api/apps/:app_id/activity
```

Legacy routes resolve to the default app. App-scoped routes require either an admin user/service token for management or an app key with the required scope for SDK-style access.

---

### Task 1: Add Apps Domain and Default App

**Files:**
- Create: `migrations/202605010002_add_apps.sql`
- Create: `src/api/apps.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/test_support.rs`
- Modify: `tests/common/mod.rs`
- Test: `src/api/apps.rs`

**Step 1: Write failing tests**

Add tests for:

```rust
#[tokio::test]
async fn test_admin_can_create_list_update_and_delete_apps() { /* create app, list, update display_name */ }

#[tokio::test]
async fn test_default_app_exists_after_db_init() { /* SELECT id FROM apps WHERE id = 'default' */ }

#[tokio::test]
async fn test_non_admin_cannot_manage_apps() { /* expect 403 */ }
```

**Step 2: Add migration**

Create `apps`:

```sql
CREATE TABLE IF NOT EXISTS apps (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    created_by TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at DATETIME NULL,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE SET NULL
);

INSERT OR IGNORE INTO apps (id, name, display_name)
VALUES ('default', 'default', 'Default App');
```

**Step 3: Add API handlers**

Implement admin-only:

- `GET /api/apps`
- `POST /api/apps`
- `GET /api/apps/:app_id`
- `PATCH /api/apps/:app_id`

Deletion should be soft-delete only and blocked for `default`.

**Step 4: Run tests**

Run:

```bash
cargo test api::apps -- --nocapture
cargo test db::tests::test_db_init
```

Expected: pass.

**Step 5: Commit**

```bash
git add migrations/202605010002_add_apps.sql src/api/apps.rs src/api/mod.rs src/app.rs src/test_support.rs tests/common/mod.rs
git commit -m "feat: add app project domain"
```

---

### Task 2: Thread App Context Through Existing Resources

**Files:**
- Create: `src/app_context.rs`
- Create: `migrations/202605010003_scope_resources_to_apps.sql`
- Modify: `src/lib.rs`
- Modify: `src/app.rs`
- Modify: `src/api/data/*`
- Modify: `src/api/functions/*`
- Modify: `src/api/push/*`
- Modify: `src/api/storage/*`
- Test: existing domain tests plus new app isolation tests

**Step 1: Write failing isolation tests**

Add one test per domain:

```rust
#[tokio::test]
async fn test_data_tables_are_isolated_by_app_id() {}

#[tokio::test]
async fn test_functions_are_isolated_by_app_id_and_endpoint_slug() {}

#[tokio::test]
async fn test_push_subscriptions_are_isolated_by_app_id() {}

#[tokio::test]
async fn test_storage_bucket_listing_is_isolated_by_app_id() {}
```

**Step 2: Add migration**

Backfill all current resources to `default`:

```sql
ALTER TABLE data_tables ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
CREATE UNIQUE INDEX IF NOT EXISTS idx_data_tables_app_name ON data_tables(app_id, name);

ALTER TABLE functions ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE function_invocations ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
CREATE UNIQUE INDEX IF NOT EXISTS idx_functions_app_name ON functions(app_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_functions_app_endpoint ON functions(app_id, endpoint_slug);

ALTER TABLE push_subscriptions ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
ALTER TABLE push_queue ADD COLUMN app_id TEXT NOT NULL DEFAULT 'default';
```

If SQLite rejects duplicate old unique indexes, create a follow-up table-copy migration for `data_tables` and `functions` so uniqueness becomes `(app_id, name)` and `(app_id, endpoint_slug)`.

**Step 3: Add `AppContext`**

`src/app_context.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppContext {
    pub app_id: String,
}

impl AppContext {
    pub fn default_app() -> Self {
        Self { app_id: "default".to_string() }
    }
}
```

Legacy routes insert `AppContext::default_app()`. App-scoped routes insert the path app id.

**Step 4: Update SQL**

Every load/list/create/update/delete query for Data, Functions, Push, and app-scoped Storage must bind `app_id`. Existing tests should keep passing because the default app context is injected.

**Step 5: Run tests**

```bash
cargo test api::data::tests -- --nocapture
cargo test api::functions::tests -- --nocapture
cargo test api::push::tests -- --nocapture
cargo test api::storage::tests -- --nocapture
```

Expected: existing behavior passes; new isolation tests pass.

**Step 6: Commit**

```bash
git add migrations/202605010003_scope_resources_to_apps.sql src/app_context.rs src/lib.rs src/app.rs src/api tests
git commit -m "feat: scope backend resources by app"
```

---

### Task 3: Split Client, Server, and Admin Keys

**Files:**
- Create: `migrations/202605010004_add_app_keys.sql`
- Create: `src/api/keys.rs`
- Create: `src/auth/principal.rs`
- Modify: `src/middleware/auth.rs`
- Modify: `src/middleware/s3_auth.rs`
- Modify: `src/api/admin.rs`
- Modify: `src/app.rs`
- Test: `src/api/keys.rs`, `src/middleware/auth.rs`

**Step 1: Define key model**

Use prefixes:

```text
pk_  publishable client key, public SDK identity, no admin access
sk_  server key, app-scoped backend SDK access
adm_ admin key, app-scoped admin automation
pst_ legacy service token, maps to default app admin for compatibility
```

**Step 2: Add migration**

```sql
CREATE TABLE IF NOT EXISTS app_keys (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_type TEXT NOT NULL,
    scopes_json TEXT NOT NULL DEFAULT '[]',
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME NULL,
    expires_at DATETIME NULL,
    revoked_at DATETIME NULL,
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_app_keys_app_id ON app_keys(app_id);
```

**Step 3: Add principal abstraction**

`Principal` should represent:

- user JWT
- legacy service token
- app key

Fields:

```rust
pub struct Principal {
    pub actor_id: String,
    pub actor_kind: ActorKind,
    pub app_id: Option<String>,
    pub is_admin: bool,
    pub scopes: Vec<String>,
}
```

Keep `Claims` compatibility initially by inserting both `Claims` and `Principal` into request extensions.

**Step 4: Add key APIs**

Admin-only:

- `GET /api/apps/:app_id/keys`
- `POST /api/apps/:app_id/keys`
- `DELETE /api/apps/:app_id/keys/:key_id`

Create response returns the raw key once.

**Step 5: Enforce scopes**

Initial scopes:

```text
auth:public
auth:admin
data:read
data:write
storage:read
storage:write
functions:invoke
functions:admin
push:subscribe
push:send
admin:all
```

Do not remove JWT user auth. Add app key support where SDK usage needs it.

**Step 6: Tests**

```bash
cargo test api::keys -- --nocapture
cargo test middleware::auth::tests -- --nocapture
cargo test middleware::s3_auth::tests -- --nocapture
```

Expected:

- `pk_` cannot call admin APIs.
- `sk_` can use app-scoped SDK APIs with granted scopes.
- `adm_` can manage app resources.
- `pst_` still works for default app admin compatibility.

**Step 7: Commit**

```bash
git add migrations/202605010004_add_app_keys.sql src/auth/principal.rs src/api/keys.rs src/middleware src/api/admin.rs src/app.rs
git commit -m "feat: split app client server and admin keys"
```

---

### Task 4: Add Auth Provider Configuration APIs

**Files:**
- Create: `migrations/202605010005_add_auth_provider_configs.sql`
- Create: `src/api/auth/providers.rs`
- Modify: `src/api/auth/mod.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/secrets.rs`
- Test: `src/api/auth/providers.rs`

**Step 1: Add migration**

```sql
CREATE TABLE IF NOT EXISTS auth_provider_configs (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0,
    config_json TEXT NOT NULL DEFAULT '{}',
    secret_ciphertext TEXT,
    encryption_version INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, provider),
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE
);
```

**Step 2: Support provider shapes**

Start with config-only API, not OAuth login flow:

```json
{
  "provider": "password",
  "enabled": true,
  "config": {
    "allow_signup": true,
    "require_email_verification": false,
    "password_min_length": 8
  }
}
```

Future-ready OAuth:

```json
{
  "provider": "google",
  "enabled": false,
  "config": {
    "client_id": "...",
    "redirect_url": "https://..."
  },
  "client_secret": "stored encrypted"
}
```

**Step 3: Add APIs**

- `GET /api/apps/:app_id/auth/providers`
- `PUT /api/apps/:app_id/auth/providers/:provider`
- `GET /api/apps/:app_id/auth/public-config`

Public config redacts secrets and only shows enabled client-safe provider info.

**Step 4: Integrate password auth**

Current password auth remains default. If `password.enabled = false`, reject `/api/register` and `/api/login` for that app-scoped auth path. Legacy `/api/register` uses default app.

**Step 5: Tests**

```bash
cargo test api::auth::providers -- --nocapture
cargo test api::auth::tests -- --nocapture
```

Expected:

- secrets never appear in API responses.
- disabled password provider blocks app-scoped password login.
- legacy auth remains enabled for default app unless explicitly disabled.

**Step 6: Commit**

```bash
git add migrations/202605010005_add_auth_provider_configs.sql src/api/auth src/secrets.rs src/app.rs
git commit -m "feat: add auth provider configuration api"
```

---

### Task 5: Add Storage Buckets and Bucket Policies

**Files:**
- Create: `migrations/202605010006_add_storage_buckets.sql`
- Create: `src/api/storage/buckets.rs`
- Create: `src/storage/policy.rs`
- Modify: `src/api/storage/mod.rs`
- Modify: `src/api/storage/s3_*`
- Modify: `src/storage/local/*`
- Modify: `src/app.rs`
- Test: `src/api/storage/tests.rs`, `tests/storage_test.rs`

**Step 1: Add policy model**

Policies:

```text
private        only owner/admin/server key
public_read    anyone can GET/HEAD/list if allowed
authenticated  active user required
service_only   server/admin key only
```

Write policy options:

```json
{
  "read": "private",
  "write": "authenticated",
  "max_object_bytes": 5242880,
  "allowed_mime_types": ["image/png", "image/jpeg"]
}
```

**Step 2: Add migration**

```sql
CREATE TABLE IF NOT EXISTS storage_buckets (
    id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(app_id, name),
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE RESTRICT
);

INSERT OR IGNORE INTO storage_buckets (id, app_id, name, display_name, policy_json, created_by)
SELECT 'default-storage', 'default', 'default', 'Default Bucket',
       '{"read":"private","write":"authenticated","max_object_bytes":5242880,"allowed_mime_types":[]}',
       id
FROM users
WHERE is_admin = 1
LIMIT 1;
```

If no admin exists yet, create the default bucket lazily on first use.

**Step 3: Add bucket APIs**

- `GET /api/apps/:app_id/storage/buckets`
- `POST /api/apps/:app_id/storage/buckets`
- `GET /api/apps/:app_id/storage/buckets/:bucket`
- `PATCH /api/apps/:app_id/storage/buckets/:bucket/policy`

**Step 4: Enforce policy**

Apply policy to:

- legacy `/api/storage/*` through default bucket
- S3 `/api/s3/:bucket/*`
- presigned URLs
- Function host bindings

**Step 5: Tests**

```bash
cargo test api::storage::tests::test_storage_bucket_policy -- --nocapture
cargo test tests::storage_test -- --nocapture
```

Expected:

- public read bucket allows unauthenticated GET only.
- private bucket rejects unauthenticated access.
- server key can write when scope includes `storage:write`.
- function host bindings honor caller scope.

**Step 6: Commit**

```bash
git add migrations/202605010006_add_storage_buckets.sql src/api/storage src/storage src/app.rs tests/storage_test.rs
git commit -m "feat: add storage bucket policies"
```

---

### Task 6: Add Function Lint, Test, and Dry-Run APIs

**Files:**
- Create: `src/api/functions/editor.rs`
- Modify: `src/api/functions/mod.rs`
- Modify: `src/api/functions/types.rs`
- Modify: `src/functions/runtime.rs`
- Modify: `src/functions/host.rs`
- Modify: `src/app.rs`
- Test: `src/api/functions/mod.rs`

**Step 1: Add request/response types**

```rust
pub struct FunctionLintRequest {
    pub runtime: String,
    pub source_code: String,
}

pub struct FunctionTestRequest {
    pub runtime: Option<String>,
    pub source_code: Option<String>,
    pub input: serde_json::Value,
    pub env: Option<BTreeMap<String, String>>,
    pub allow_side_effects: Option<bool>,
}
```

**Step 2: Add lint API**

Routes:

- `POST /api/apps/:app_id/functions/lint`
- `POST /api/apps/:app_id/functions/:name/lint`

Implementation:

- run current source validation
- write source to temp dir
- run `deno check --quiet --no-prompt --allow-read=<temp>`
- return structured diagnostics: `ok`, `errors[]`, `warnings[]`, `duration_ms`

**Step 3: Add test/dry-run API**

Routes:

- `POST /api/apps/:app_id/functions/test`
- `POST /api/apps/:app_id/functions/:name/test`

Default `allow_side_effects=false`.

When side effects are false, host bindings allow:

- `storage.list`
- `storage.get`
- `data.listRows`
- `data.getRow`

and reject:

- `storage.put`
- `storage.delete`
- `push.enqueue`
- `data.createRow`
- `data.updateRow`
- `data.deleteRow`

**Step 4: Do not persist normal invocation**

Function tests should create a `function_editor_runs` row only if we want history. For first implementation, return result directly and emit audit event. Do not insert into `function_invocations` to avoid polluting production logs.

**Step 5: Tests**

```bash
cargo test api::functions::tests::test_function_lint_reports_deno_syntax_errors -- --nocapture
cargo test api::functions::tests::test_function_test_runs_without_persisting_invocation -- --nocapture
cargo test api::functions::tests::test_function_test_rejects_side_effect_host_calls_by_default -- --nocapture
```

Expected: pass with local Deno installed.

**Step 6: Commit**

```bash
git add src/api/functions src/functions src/app.rs
git commit -m "feat: add function editor lint and test api"
```

---

### Task 7: Add Push Setup Diagnostics API

**Files:**
- Create: `src/api/push/diagnostics.rs`
- Modify: `src/api/push/mod.rs`
- Modify: `src/push/webpush.rs`
- Modify: `src/push/ntfy.rs`
- Modify: `src/app.rs`
- Test: `src/api/push/mod.rs`

**Step 1: Define diagnostics response**

```json
{
  "status": "ok|warning|error",
  "ntfy": {
    "configured": true,
    "base_url_ok": true,
    "auth_configured": false
  },
  "web_push": {
    "configured": true,
    "vapid_key_ok": true,
    "subject_ok": true,
    "public_key": "..."
  },
  "subscriptions": {
    "total": 2,
    "web_push": 1,
    "ntfy": 1
  },
  "queue": {
    "pending": 0,
    "retry_overdue": 0,
    "failed_recent": 0
  },
  "client_setup": {
    "vapid_public_key_url": "/api/apps/default/push/vapid-public-key",
    "subscription_url": "/api/apps/default/push/subscriptions"
  }
}
```

**Step 2: Add route**

- `GET /api/apps/:app_id/push/diagnostics`

Admin-only. Later the console can poll this during setup.

**Step 3: Add tests**

```bash
cargo test api::push::tests::test_push_diagnostics_reports_missing_vapid_config -- --nocapture
cargo test api::push::tests::test_push_diagnostics_counts_subscriptions_and_queue -- --nocapture
```

**Step 4: Commit**

```bash
git add src/api/push src/push src/app.rs
git commit -m "feat: add push setup diagnostics"
```

---

### Task 8: Add Audit Log and Activity Feed

**Files:**
- Create: `migrations/202605010007_add_audit_events.sql`
- Create: `src/api/activity.rs`
- Create: `src/audit.rs`
- Modify: `src/lib.rs`
- Modify: `src/app.rs`
- Modify: `src/api/auth/*`
- Modify: `src/api/admin.rs`
- Modify: `src/api/data/*`
- Modify: `src/api/storage/*`
- Modify: `src/api/functions/*`
- Modify: `src/api/push/*`
- Test: `src/api/activity.rs`

**Step 1: Add migration**

```sql
CREATE TABLE IF NOT EXISTS audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id TEXT NOT NULL DEFAULT 'default',
    actor_kind TEXT NOT NULL,
    actor_id TEXT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    request_id TEXT,
    ip_address TEXT,
    user_agent TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(app_id) REFERENCES apps(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_audit_events_app_created ON audit_events(app_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_resource ON audit_events(app_id, resource_type, resource_id);
```

**Step 2: Add helper**

`src/audit.rs`:

```rust
pub async fn record_audit_event(
    pool: &sqlx::SqlitePool,
    app_id: &str,
    principal: Option<&crate::auth::principal::Principal>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    metadata: serde_json::Value,
) -> Result<(), sqlx::Error> { /* insert row */ }
```

**Step 3: Record important actions**

Add audit events for:

- app created/updated
- key created/revoked
- auth provider updated
- user activated/deactivated
- data table created/updated/deleted/imported/exported
- storage bucket created/policy updated/object put/delete
- function created/updated/deleted/invoked/retried/rolled back/linted/tested
- push subscription created/deleted/message queued
- backup created/restore scheduled

Keep existing `auth_events` for auth-specific timeline, but mirror major auth actions to `audit_events`.

**Step 4: Add activity API**

- `GET /api/apps/:app_id/activity?resource_type=&resource_id=&limit=&cursor=`

Admin-only. Return newest first with cursor pagination.

**Step 5: Tests**

```bash
cargo test api::activity -- --nocapture
cargo test api::functions::tests::test_function_actions_emit_audit_events -- --nocapture
cargo test api::storage::tests::test_storage_policy_update_emits_audit_event -- --nocapture
```

**Step 6: Commit**

```bash
git add migrations/202605010007_add_audit_events.sql src/audit.rs src/api/activity.rs src/api src/app.rs src/lib.rs
git commit -m "feat: add audit activity feed"
```

---

### Task 9: Add OpenAPI and Documentation Contract

**Files:**
- Modify: `docs/openapi.yaml`
- Modify: `README.md`
- Modify: `README.ko.md`
- Modify: `docs/deployment.md`
- Modify: `docs/service-tokens.md`
- Create: `docs/app-platform.md`

**Step 1: Document app model**

`docs/app-platform.md` should explain:

- default app compatibility
- app-scoped routes
- key types and scopes
- auth provider config
- storage bucket policy
- function editor APIs
- push diagnostics
- audit activity

**Step 2: Update OpenAPI**

Add schemas and endpoints for all new route surfaces. Keep old endpoints documented as default-app compatibility routes.

**Step 3: Run contract checks**

```bash
scripts/check-openapi.sh
```

Expected: pass.

**Step 4: Commit**

```bash
git add docs README.md README.ko.md
git commit -m "docs: document app platform api"
```

---

### Task 10: End-to-End Platform Smoke

**Files:**
- Create: `examples/platform-e2e/run-happy-path.sh`
- Modify: `.github/workflows/smoke.yml`

**Step 1: Create E2E flow**

Script should:

1. register admin
2. create app
3. create publishable/server/admin keys
4. configure password provider
5. create data table
6. create storage bucket and policy
7. create function
8. lint function
9. test function without side effects
10. create push diagnostic request
11. fetch activity feed and assert events exist

**Step 2: Add CI step**

In `smoke` job after existing operations happy path:

```yaml
- name: Platform happy path
  env:
    ADMIN_JWT: ${{ env.ADMIN_JWT }}
  run: examples/platform-e2e/run-happy-path.sh
```

**Step 3: Run locally**

```bash
cargo run
examples/platform-e2e/run-happy-path.sh
```

Expected: every API call returns 2xx and final activity list contains app/key/function/storage/push events.

**Step 4: Commit**

```bash
git add examples/platform-e2e .github/workflows/smoke.yml
git commit -m "test: add platform happy path smoke"
```

---

## Suggested Implementation Order

1. Apps domain and default app.
2. App context/resource scoping.
3. Principal and key split.
4. Audit log helper early, then emit events incrementally.
5. Auth provider config API.
6. Storage bucket policies.
7. Function lint/test/dry-run.
8. Push diagnostics.
9. OpenAPI/docs.
10. E2E platform smoke.

This order avoids reworking every feature twice. App context and principal are the foundations; the other features should sit on top of them.

## Acceptance Criteria

- All existing tests still pass.
- Existing legacy API routes continue to behave as default-app routes.
- New app-scoped routes can isolate two apps in the same Peanut instance.
- Client/server/admin keys have distinct scopes and failure modes.
- Auth provider config API stores secrets encrypted and redacts them in responses.
- Storage bucket policies are enforced by REST, S3-compatible routes, presigned URLs, and Function host bindings.
- Function lint/test APIs use Deno and do not pollute production invocation history.
- Push diagnostics explain setup health without requiring a console.
- Activity feed shows cross-domain app events in chronological order.
- Docker smoke still passes with Functions enabled.

## Final Verification Commands

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
scripts/check-openapi.sh
docker build -t peanut:local-smoke .
docker run --rm -d --name peanut-local-smoke -p 127.0.0.1:3010:3000 \
  -e JWT_SECRET=local-docker-secret-that-is-long-enough \
  -e DATABASE_URL=sqlite://data/peanut.db \
  -e STORAGE_DIR=data/storage \
  -e BIND_ADDR=0.0.0.0:3000 \
  -e FUNCTIONS_ENABLED=true \
  peanut:local-smoke
curl -fsS http://127.0.0.1:3010/api/ready | jq .
docker stop peanut-local-smoke
```

