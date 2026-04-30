# Peanut Refactor Priorities Plan

> For Hermes: use subagent-driven-development skill if executing this plan. Keep each slice shippable, do not expand Peanut into a general backend platform, and prefer structure-only refactors before new feature work.

**Goal:** reduce maintenance risk in Peanut by breaking oversized backend modules into explicit boundaries without changing the product scope.

**Architecture:** keep the existing API-first, single-binary, self-host posture. Refactor by extracting feature-local modules and service helpers behind the current routes first; do not redesign the entire app into a heavyweight layered architecture. Preserve current HTTP contracts and test coverage while moving code.

**Tech Stack:** Rust, Axum, SQLx SQLite, local filesystem storage, Node subprocess sandbox, ntfy/Web Push integrations.

---

## Why this plan exists

Current codebase facts verified in the repo before writing this plan:
- `src/api/storage.rs` is the largest backend module at ~8k LOC.
- `src/api/data.rs` is ~3.7k LOC.
- `src/api/functions.rs` is ~2.7k LOC.
- `cargo test` currently passes with 178 tests.
- `./scripts/build.sh` currently succeeds.
- `main.rs` already acts as a wide composition root for config, state, workers, route assembly, middleware, and fallback behavior.

Main risk:
- Peanut’s product scope is still coherent, but core modules are reaching a size where every new feature increases regression risk and local reasoning cost.

Non-goals:
- do not change public API contracts unless a task explicitly says so
- do not introduce raw SQL endpoints or a plugin/orchestration system
- do not rebuild a new console as part of this refactor plan
- do not force a broad domain-driven rewrite across the whole repo in one pass

---

## Phase order

1. Storage API split
2. Data API split
3. Functions API split
4. Router/composition extraction from `main.rs`
5. Shared service/helper extraction where duplication or rule-mixing remains
6. Cleanup, docs, verification

Each phase should land independently.

---

## Phase 1: Split `src/api/storage.rs`

**Objective:** break storage handling into smaller modules organized by protocol concern while preserving the current route surface and test behavior.

**Files:**
- Create: `src/api/storage/mod.rs`
- Create: `src/api/storage/basic.rs`
- Create: `src/api/storage/s3_object.rs`
- Create: `src/api/storage/s3_list.rs`
- Create: `src/api/storage/s3_multipart.rs`
- Create: `src/api/storage/s3_copy.rs`
- Create: `src/api/storage/s3_tagging.rs`
- Create: `src/api/storage/s3_presign.rs`
- Create: `src/api/storage/s3_xml.rs`
- Create: `src/api/storage/s3_error.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/api/storage.rs` (temporary source for extraction, then delete/replace)

### Task 1.1: Establish storage module shell
**Objective:** create a `src/api/storage/` module tree without changing behavior.

**Steps:**
1. Create `src/api/storage/mod.rs`.
2. Re-export the existing handler function names from `mod.rs` so `main.rs` route wiring can stay unchanged during the split.
3. Move only imports/constants/types that are clearly shared into `mod.rs`.
4. Keep all logic compiling before any deeper extraction.

**Run:**
- `cargo test api::storage::tests -- --nocapture`
- `cargo test`

### Task 1.2: Extract legacy storage CRUD handlers
**Objective:** isolate non-S3 object CRUD from the S3-compatible surface.

**Files:**
- Create: `src/api/storage/basic.rs`
- Modify: `src/api/storage/mod.rs`

**Steps:**
1. Move `list_objects`, `get_object`, `put_object`, `delete_object` into `basic.rs`.
2. Keep helper functions that are used only by legacy CRUD inside `basic.rs`.
3. Re-export those handlers from `mod.rs`.
4. Verify no route names or response contracts changed.

**Run:**
- `cargo test api::storage::tests::test_storage_is_scoped_per_user -- --nocapture`
- `cargo test api::storage::tests::test_presigned_get_round_trip_uses_sigv4_query_auth -- --nocapture`
- `cargo test`

### Task 1.3: Extract S3 listing and listing XML formatting
**Objective:** separate bucket listing logic from object read/write logic.

**Files:**
- Create: `src/api/storage/s3_list.rs`
- Create: `src/api/storage/s3_xml.rs`
- Modify: `src/api/storage/mod.rs`

**Steps:**
1. Move `list_bucket_objects` and list-related query structs/helpers into `s3_list.rs`.
2. Move XML serialization helpers used primarily by list responses into `s3_xml.rs`.
3. Keep shared encoding/token helpers close to listing.
4. Preserve current list-type=2 and multipart-upload listing behavior.

**Run:**
- `cargo test api::storage::tests::test_s3_like_list_objects_v2_supports_delimiter_common_prefixes -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_list_multipart_uploads_returns_active_uploads -- --nocapture`
- `cargo test`

### Task 1.4: Extract S3 object read/write/head/delete paths
**Objective:** isolate direct object request handling.

**Files:**
- Create: `src/api/storage/s3_object.rs`
- Modify: `src/api/storage/mod.rs`

**Steps:**
1. Move `head_bucket_object`, `get_bucket_object`, `put_bucket_object`, `delete_bucket_object` into `s3_object.rs`.
2. Keep request precondition/range/header logic there unless it is reused elsewhere.
3. Do not mix multipart or tagging-specific flows into this file unless unavoidable.
4. Preserve metadata and conditional-header behavior exactly.

**Run:**
- `cargo test api::storage::tests::test_s3_like_get_object_supports_single_byte_range -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_head_object_honors_conditional_headers -- --nocapture`
- `cargo test`

### Task 1.5: Extract multipart/copy/tagging/presign/error modules
**Objective:** isolate the four most specialized S3-compatible feature clusters.

**Files:**
- Create: `src/api/storage/s3_multipart.rs`
- Create: `src/api/storage/s3_copy.rs`
- Create: `src/api/storage/s3_tagging.rs`
- Create: `src/api/storage/s3_presign.rs`
- Create: `src/api/storage/s3_error.rs`
- Modify: `src/api/storage/mod.rs`

**Steps:**
1. Move multipart initiation/upload/list/complete/abort into `s3_multipart.rs`.
2. Move CopyObject/CopyPart logic into `s3_copy.rs`.
3. Move tagging header/subresource logic into `s3_tagging.rs`.
4. Move presign request parsing/signing helpers into `s3_presign.rs`.
5. Consolidate XML S3 error envelope helpers into `s3_error.rs`.
6. Remove dead code from the old monolith after every move.

**Run:**
- `cargo test api::storage::tests::test_s3_like_multipart_upload_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_copy_part_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_object_tagging_subresource_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_presign_generates_sigv4_query_params -- --nocapture`
- `cargo test`

### Task 1.6: Final storage cleanup
**Objective:** make the storage module tree readable and stable.

**Steps:**
1. Delete or shrink the old monolithic `src/api/storage.rs` file entirely.
2. Ensure module names reflect product concepts rather than accidental implementation details.
3. Run `cargo fmt` and `cargo test`.
4. Commit the storage split as its own change.

**Commit suggestion:**
- `refactor: split storage api into protocol-focused modules`

---

## Phase 2: Split `src/api/data.rs`

**Objective:** separate table admin, row CRUD, presets, import/export, and event-stream responsibilities.

**Files:**
- Create: `src/api/data/mod.rs`
- Create: `src/api/data/tables.rs`
- Create: `src/api/data/rows.rs`
- Create: `src/api/data/schema.rs`
- Create: `src/api/data/presets.rs`
- Create: `src/api/data/import_export.rs`
- Create: `src/api/data/events.rs`
- Create: `src/api/data/types.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`

### Task 2.1: Establish data module shell
**Steps:**
1. Create `src/api/data/mod.rs`.
2. Move shared request/response structs into `types.rs` when they are used by multiple handlers.
3. Re-export current handler names from `mod.rs`.
4. Keep behavior identical.

**Run:**
- `cargo test api::data::tests -- --nocapture`
- `cargo test`

### Task 2.2: Extract table lifecycle and schema rules
**Objective:** separate logical-table admin endpoints from row operations.

**Files:**
- Create: `src/api/data/tables.rs`
- Create: `src/api/data/schema.rs`

**Steps:**
1. Move create/get/update/delete table handlers into `tables.rs`.
2. Move schema validation and evolution rules into `schema.rs`.
3. Keep schema helpers callable from import paths too.
4. Avoid leaving schema rules buried inside HTTP handlers.

**Run:**
- `cargo test api::data::tests::test_admin_can_create_and_fetch_table -- --nocapture`
- `cargo test api::data::tests::test_schema_evolution_rejects_field_type_changes -- --nocapture`
- `cargo test`

### Task 2.3: Extract row CRUD and query logic
**Files:**
- Create: `src/api/data/rows.rs`

**Steps:**
1. Move list/get/create/update/delete row handlers into `rows.rs`.
2. Keep query parsing/filter application local unless reused by presets.
3. Make sure owner-private behavior remains unchanged.

**Run:**
- `cargo test api::data::tests::test_list_rows_supports_limit_order_and_filters -- --nocapture`
- `cargo test api::data::tests::test_owner_private_rows_are_isolated_per_user -- --nocapture`
- `cargo test`

### Task 2.4: Extract presets and import/export flows
**Files:**
- Create: `src/api/data/presets.rs`
- Create: `src/api/data/import_export.rs`

**Steps:**
1. Move preset CRUD/run handlers into `presets.rs`.
2. Move table export/import handlers into `import_export.rs`.
3. Reuse schema helpers from `schema.rs` rather than duplicating checks.

**Run:**
- `cargo test api::data::tests::test_admin_can_manage_query_presets -- --nocapture`
- `cargo test api::data::tests::test_admin_can_export_table_snapshot -- --nocapture`
- `cargo test api::data::tests::test_admin_can_import_rows_into_table -- --nocapture`
- `cargo test`

### Task 2.5: Extract event replay/checkpoint/streaming
**Files:**
- Create: `src/api/data/events.rs`

**Steps:**
1. Move event listing, checkpoint, and SSE stream handlers into `events.rs`.
2. Keep event payload types in `types.rs` if reused elsewhere.
3. Preserve event id and resume semantics exactly.

**Run:**
- `cargo test api::data::tests::test_admin_can_query_row_events_for_table -- --nocapture`
- `cargo test api::data::tests::test_admin_can_replay_row_events_from_since_id_cursor -- --nocapture`
- `cargo test`

### Task 2.6: Final data cleanup
**Steps:**
1. Remove dead imports/helpers from the old monolith.
2. Make `mod.rs` a readable index of the Data API surface.
3. Run `cargo fmt && cargo test`.
4. Commit separately.

**Commit suggestion:**
- `refactor: split data api into focused modules`

---

## Phase 3: Split `src/api/functions.rs` and align `src/functions/mod.rs`

**Objective:** separate functions management, invocation, versioning, and event visibility so the bounded runtime can evolve without one giant handler file.

**Files:**
- Create: `src/api/functions/mod.rs`
- Create: `src/api/functions/admin.rs`
- Create: `src/api/functions/invoke.rs`
- Create: `src/api/functions/versions.rs`
- Create: `src/api/functions/invocations.rs`
- Create: `src/api/functions/events.rs`
- Create: `src/api/functions/types.rs`
- Modify: `src/functions/mod.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`

### Task 3.1: Establish functions module shell
**Steps:**
1. Create `src/api/functions/mod.rs` and re-export current public handlers.
2. Move shared request/response structs to `types.rs` when used by multiple handler groups.
3. Keep compilation green before deeper extraction.

**Run:**
- `cargo test api::functions::tests -- --nocapture`
- `cargo test`

### Task 3.2: Extract admin CRUD and policy validation
**Files:**
- Create: `src/api/functions/admin.rs`

**Steps:**
1. Move create/list/get/update/delete admin handlers into `admin.rs`.
2. Keep name/slug/runtime/policy validation near the admin write paths unless reused broadly.
3. Ensure secret redaction behavior stays intact.

**Run:**
- `cargo test api::functions::tests::test_non_admin_cannot_manage_functions -- --nocapture`
- `cargo test api::functions::tests::test_function_secrets_are_redacted_in_api_and_runtime_output -- --nocapture`
- `cargo test`

### Task 3.3: Extract invoke and invocation lifecycle handlers
**Files:**
- Create: `src/api/functions/invoke.rs`
- Create: `src/api/functions/invocations.rs`

**Steps:**
1. Move public invoke endpoint and request auth/policy checks into `invoke.rs`.
2. Move invocation list/detail/retry handlers into `invocations.rs`.
3. Keep async invocation lifecycle behavior unchanged.
4. Minimize direct sandbox details in HTTP handlers where practical.

**Run:**
- `cargo test api::functions::tests::test_admin_can_create_and_invoke_function -- --nocapture`
- `cargo test api::functions::tests::test_function_supports_async_invocation_lifecycle -- --nocapture`
- `cargo test api::functions::tests::test_admin_can_read_invocation_detail_retry_and_attempt_chain -- --nocapture`
- `cargo test`

### Task 3.4: Extract versions and realtime events
**Files:**
- Create: `src/api/functions/versions.rs`
- Create: `src/api/functions/events.rs`

**Steps:**
1. Move version history and rollback handlers into `versions.rs`.
2. Move SSE/realtime event streaming into `events.rs`.
3. Keep event payload types in `types.rs` if reused by invoke/invocation flows.

**Run:**
- `cargo test api::functions::tests::test_function_version_history_and_rollback -- --nocapture`
- `cargo test api::functions::tests::test_function_realtime_events_follow_async_invocation_lifecycle -- --nocapture`
- `cargo test`

### Task 3.5: Align runtime helper boundaries in `src/functions/mod.rs`
**Objective:** reduce HTTP/runtime coupling without a full runtime redesign.

**Steps:**
1. Identify helper types in `src/functions/mod.rs` that belong to sandbox execution only.
2. Extract internal helpers if needed into `src/functions/runtime.rs` and `src/functions/host_calls.rs`.
3. Keep public entrypoint names stable.
4. Do not change runtime contract unless tests require it.

**Run:**
- `cargo test api::functions::tests::test_authenticated_function_can_use_storage_and_push_bindings -- --nocapture`
- `cargo test api::functions::tests::test_authenticated_function_can_use_data_row_bindings -- --nocapture`
- `cargo test`

**Commit suggestion:**
- `refactor: split functions api and sandbox helpers`

---

## Phase 4: Extract app composition from `main.rs`

**Objective:** make boot/runtime setup readable by separating route assembly from process startup.

**Files:**
- Create: `src/app.rs`
- Create: `src/routes.rs`
- Modify: `src/main.rs`

### Task 4.1: Extract app builder
**Steps:**
1. Create `build_app(state, config) -> Router` in `src/app.rs` or `src/routes.rs`.
2. Move route assembly from `main.rs` into that builder.
3. Keep `main.rs` focused on config load, DB/storage init, state creation, worker spawn, and `axum::serve`.
4. Preserve current middleware order.

**Run:**
- `cargo test`
- `./scripts/build.sh`

### Task 4.2: Extract route-group helpers
**Steps:**
1. Create small helper builders for auth-public, auth-protected, admin/protected, storage, data, functions, and push routes.
2. Keep route declarations grouped by product surface.
3. Do not prematurely over-abstract simple routers.

**Run:**
- `cargo test`
- `./scripts/build.sh`

**Commit suggestion:**
- `refactor: extract app and route composition from main`

---

## Phase 5: Shared helper/service extraction only where it earns its keep

**Objective:** remove obvious rule duplication and handler/domain mixing without introducing a heavy framework.

**Files (examples, only if justified by repetition after Phases 1–4):**
- Create: `src/api/authz.rs`
- Create: `src/data/schema_rules.rs`
- Create: `src/storage/s3_headers.rs`
- Create: `src/functions/policy.rs`

### Task 5.1: Consolidate repeated authz and response helpers
**Steps:**
1. Search for repeated admin-check / user-check / status-mapping logic after module splits.
2. Extract only high-reuse helpers.
3. Prefer plain functions over trait-heavy abstractions.

### Task 5.2: Consolidate repeated protocol helpers
**Steps:**
1. Search for repeated S3 header parsing, checksum/tagging normalization, or common XML envelope code.
2. Extract helper modules only where at least two call sites clearly share the same rules.
3. Keep product-specific rules close to their subsystem if reuse is weak.

**Run:**
- `cargo test`
- `./scripts/build.sh`

**Commit suggestion:**
- `refactor: extract shared backend helpers after api split`

---

## Phase 6: Cleanup, documentation, and verification

**Objective:** leave the repo clearer than before and prove no behavior regressions.

**Files:**
- Modify: `README.md` if file/module layout references need updates
- Modify: `README.ko.md` if needed
- Modify: `docs/plans/2026-04-30-peanut-refactor-priorities.md` if execution findings require adjustments
- Remove: stray local artifacts such as `peanut-console/out/index.html` if no longer intentionally kept

### Task 6.1: Verification sweep
**Steps:**
1. Run `cargo test`.
2. Run `./scripts/build.sh`.
3. Run targeted route smoke checks if any route wiring changed unexpectedly.
4. Review `git diff --stat` to confirm changes are structural, not accidental product-scope drift.

### Task 6.2: Working tree cleanup
**Steps:**
1. Remove dead files left over from extraction.
2. Make module names and exports consistent.
3. Confirm `src/api/mod.rs` reads as a clean subsystem index.

### Task 6.3: Final commit strategy
**Suggested commits:**
1. `refactor: split storage api into protocol-focused modules`
2. `refactor: split data api into focused modules`
3. `refactor: split functions api and sandbox helpers`
4. `refactor: extract app and route composition from main`
5. `refactor: clean up shared backend helpers`

---

## Recommended execution order if time is limited

If only one refactor slice is feasible right now, do this order:
1. Storage split
2. Data split
3. Functions split
4. `main.rs` composition extraction

That order matches current maintenance risk and file size concentration.

## Success criteria

This plan succeeds when all of the following are true:
- no public route contracts changed unintentionally
- `cargo test` remains green throughout
- `./scripts/build.sh` remains green throughout
- storage/data/functions each have explicit module boundaries instead of giant monolith files
- `main.rs` is small enough to read as startup code, not subsystem code
- Peanut still feels like a narrow self-host backend core, not a generalized platform framework
