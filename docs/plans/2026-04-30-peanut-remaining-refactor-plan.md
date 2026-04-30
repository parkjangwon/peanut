# Peanut Remaining Refactor Implementation Plan

> **For Hermes:** Use subagent-driven-development if executing this plan. Keep each slice shippable, preserve current HTTP/runtime contracts, and avoid turning Peanut into a framework.

**Goal:** finish the second-stage refactor work that remains after the first storage/data/functions/app split, focusing on oversized modules that still mix multiple responsibilities.

**Architecture:** keep Peanut’s current API-first, single-binary structure. Prefer feature-local module extraction, re-exports, and narrow helper modules over new global abstractions. Do not redesign product boundaries; only make existing boundaries explicit.

**Tech Stack:** Rust, Axum, SQLx SQLite, local filesystem storage, Node subprocess sandbox, ntfy/Web Push.

---

## Current verified codebase facts

- `src/api/storage/mod.rs` is still the largest file at ~6899 lines.
- `src/api/auth.rs` is ~1592 lines (~969 lines before tests).
- `src/storage/local.rs` is ~1168 lines (~987 lines before tests).
- `src/functions/mod.rs` is ~807 lines (~724 lines before tests).
- `src/api/push.rs` is ~1383 lines (~728 lines before tests).
- `src/app.rs` is already extracted and readable; no further composition work is urgent.
- Current storage module tree exists, but `src/api/storage/mod.rs` still contains tagging/XML/helper-heavy logic and the main test block.

## Refactor order

1. Storage API second-pass split
2. Auth API split
3. Local storage implementation split
4. Functions runtime / host-call split
5. Push API split
6. Cleanup + verification

Each phase lands as its own commit.

---

## Phase 1: Storage API second-pass split

**Objective:** turn `src/api/storage/mod.rs` from a partial monolith into a true module index plus only genuinely shared helpers.

**Target files:**
- Modify: `src/api/storage/mod.rs`
- Create: `src/api/storage/s3_tagging.rs`
- Create: `src/api/storage/s3_xml.rs`
- Create: `src/api/storage/s3_error.rs`
- Create: `src/api/storage/s3_multipart.rs`
- Create: `src/api/storage/s3_copy.rs`
- Optional Create: `src/api/storage/tests.rs`

### Task 1.1: Map remaining concerns inside `storage/mod.rs`
**Objective:** identify which sections still belong to tagging, XML formatting, multipart, copy, errors, or tests.

**Files:**
- Read/Modify: `src/api/storage/mod.rs`

**Steps:**
1. Add temporary section comments around remaining concern clusters if the file lacks clear markers.
2. Group functions into six buckets:
   - shared constants + shared tiny helpers
   - XML encode/escape/response helpers
   - tagging parse/canonicalization/render helpers
   - multipart-specific helpers/types
   - copy-specific helpers/types
   - test-only helpers/tests
3. Confirm each bucket has at least one stable extraction destination.

**Run:**
- no tests yet; this is mapping only

### Task 1.2: Extract XML-only helpers
**Objective:** move response-serialization helpers that are storage-protocol-specific but not request-flow-specific.

**Files:**
- Create: `src/api/storage/s3_xml.rs`
- Modify: `src/api/storage/mod.rs`

**Steps:**
1. Move XML escape / XML envelope / list XML rendering helpers into `s3_xml.rs`.
2. Expose only the minimal `pub(super)` functions needed by sibling modules.
3. Keep `mod.rs` imports slim; prefer `use crate::api::storage::s3_xml::...` inside sibling modules.
4. Do not move JSON or generic response helpers that already live elsewhere.

**Run:**
- `cargo test api::storage::tests::test_s3_like_list_objects_v2_supports_delimiter_common_prefixes -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_head_object_honors_conditional_headers -- --nocapture`

### Task 1.3: Extract tagging parsing and normalization
**Objective:** isolate S3 tagging rules from generic storage routing.

**Files:**
- Create: `src/api/storage/s3_tagging.rs`
- Modify: `src/api/storage/mod.rs`
- Modify if needed: `src/api/storage/s3_object.rs`

**Steps:**
1. Move tagging constants, query detection, XML parse, canonicalization, and XML response helpers into `s3_tagging.rs`.
2. Keep the public entrypoint in whichever route handler already owns the request path; only move helpers first.
3. If tagging request branches inside `put_bucket_object` / `get_bucket_object` / `delete_bucket_object` are too noisy, extract small internal dispatch helpers instead of full handler moves.
4. Prefer `pub(super)` visibility, not `pub(crate)`, unless another storage module truly needs broader access.

**Run:**
- `cargo test api::storage::tests::test_s3_like_object_tagging_subresource_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_get_object_supports_single_byte_range -- --nocapture`

### Task 1.4: Extract multipart-specific helpers
**Objective:** isolate multipart upload bookkeeping and response formatting.

**Files:**
- Create: `src/api/storage/s3_multipart.rs`
- Modify: `src/api/storage/mod.rs`
- Modify if needed: `src/api/storage/s3_list.rs`
- Modify if needed: `src/api/storage/s3_object.rs`

**Steps:**
1. Move multipart upload id parsing, part validation, part listing helpers, complete/abort response builders, and related structs into `s3_multipart.rs`.
2. Leave the main request handlers where they currently sit if moving them would create cross-module churn; helper extraction is enough for this pass.
3. Re-check upload listing paths, because list bucket flows often share multipart XML formatting.

**Run:**
- `cargo test api::storage::tests::test_s3_like_multipart_upload_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_list_multipart_uploads_returns_active_uploads -- --nocapture`

### Task 1.5: Extract copy + error helpers
**Objective:** isolate specialized CopyObject/UploadPartCopy helpers and S3 XML error rendering.

**Files:**
- Create: `src/api/storage/s3_copy.rs`
- Create: `src/api/storage/s3_error.rs`
- Modify: `src/api/storage/mod.rs`
- Modify if needed: `src/api/storage/s3_object.rs`

**Steps:**
1. Move copy-source parsing, copy response payload generation, and copy metadata normalization into `s3_copy.rs`.
2. Move XML S3 error envelope builders/status mapping into `s3_error.rs`.
3. Ensure object handlers import from `s3_copy` / `s3_error` rather than keeping inline helper blocks.
4. Keep all human-visible error codes/messages unchanged.

**Run:**
- `cargo test api::storage::tests::test_s3_like_copy_part_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_presign_generates_sigv4_query_params -- --nocapture`

### Task 1.6: Move storage tests out of `mod.rs`
**Objective:** make `mod.rs` production-oriented again.

**Files:**
- Create optional: `src/api/storage/tests.rs`
- Modify: `src/api/storage/mod.rs`

**Steps:**
1. If test helpers are large, move the `#[cfg(test)] mod tests` block into `tests.rs` and wire it with `#[cfg(test)] mod tests;`.
2. Keep any tiny `#[cfg(test)]` type import in `mod.rs` only if unavoidable.
3. Confirm test helper visibility remains minimal.

**Run:**
- `cargo test api::storage::tests -- --nocapture`
- `cargo test`

**Commit suggestion:**
- `refactor: finish storage module split`

---

## Phase 2: Split `src/api/auth.rs`

**Objective:** separate public auth flows, password lifecycle, session management, and auth events.

**Target files:**
- Replace/Modify: `src/api/auth.rs` -> `src/api/auth/mod.rs`
- Create: `src/api/auth/public.rs`
- Create: `src/api/auth/password.rs`
- Create: `src/api/auth/sessions.rs`
- Create: `src/api/auth/events.rs`
- Create optional: `src/api/auth/types.rs`
- Modify if needed: `src/api/mod.rs`
- Modify if needed: `src/app.rs`

### Task 2.1: Create auth module shell
**Objective:** convert the single file into a module tree without changing route names.

**Steps:**
1. Rename `src/api/auth.rs` to `src/api/auth/mod.rs`.
2. Add child module declarations.
3. Re-export existing handlers so `app.rs` stays unchanged.
4. Keep shared request/response types in `mod.rs` temporarily until natural grouping is obvious.

**Run:**
- `cargo test api::auth::tests -- --nocapture`
- `cargo test`

### Task 2.2: Extract public auth endpoints
**Objective:** isolate anonymous-client entrypoints.

**Files:**
- Create: `src/api/auth/public.rs`
- Modify: `src/api/auth/mod.rs`

**Scope:**
- `register`
- `login`
- `refresh_session`
- `logout` if it belongs to token/session edge handling without authenticated claims

**Steps:**
1. Move handlers and only their direct helpers.
2. Keep client-policy middleware expectations unchanged.
3. Ensure register/login payload and response structs remain exactly stable.

**Run:**
- `cargo test api::auth::tests::test_register_login_and_me_flow -- --nocapture`
- `cargo test api::auth::tests::test_refresh_rotates_session -- --nocapture`

### Task 2.3: Extract password lifecycle handlers
**Objective:** isolate forgot/reset/change-password flows and password validation helpers.

**Files:**
- Create: `src/api/auth/password.rs`
- Modify: `src/api/auth/mod.rs`

**Scope:**
- `forgot_password`
- `reset_password`
- `change_password`
- reset token generation/validation helpers
- password policy helpers if local to auth HTTP flows

**Run:**
- `cargo test api::auth::tests::test_forgot_password_flow_issues_reset -- --nocapture`
- `cargo test api::auth::tests::test_change_password_revokes_other_sessions_or_preserves_expected_contract -- --nocapture`

### Task 2.4: Extract session management
**Objective:** isolate authenticated session listing and revocation behavior.

**Files:**
- Create: `src/api/auth/sessions.rs`
- Modify: `src/api/auth/mod.rs`

**Scope:**
- `me`
- `list_sessions`
- `revoke_session`
- `revoke_all_sessions`

**Steps:**
1. Keep `me` here only if it shares session/authenticated-user query helpers; otherwise place it in a tiny `profile` section inside the same file.
2. Keep session DB row mapping helpers local.
3. Preserve revoke semantics exactly.

**Run:**
- `cargo test api::auth::tests::test_user_can_list_and_revoke_sessions -- --nocapture`
- `cargo test api::auth::tests::test_me_returns_current_user -- --nocapture`

### Task 2.5: Extract auth event queries
**Objective:** isolate audit/event querying from login/session mutation logic.

**Files:**
- Create: `src/api/auth/events.rs`
- Modify: `src/api/auth/mod.rs`

**Scope:**
- `list_auth_events`
- event response mapping helpers

**Run:**
- `cargo test api::auth::tests::test_auth_events_include_expected_entries -- --nocapture`
- `cargo test`

### Task 2.6: Optional type cleanup
**Objective:** create a readable auth index rather than another fat `mod.rs`.

**Files:**
- Create optional: `src/api/auth/types.rs`
- Modify: `src/api/auth/mod.rs`

**Steps:**
1. Move request/response structs used across 2+ auth child modules into `types.rs`.
2. Keep one-file-only structs close to the owning handler file.
3. Keep `mod.rs` mostly re-exports, shared constants, and test module wiring.

**Run:**
- `cargo test api::auth::tests -- --nocapture`
- `cargo test`

**Commit suggestion:**
- `refactor: split auth api into focused modules`

---

## Phase 3: Split `src/storage/local.rs`

**Objective:** separate the local storage backend implementation by storage concern rather than HTTP concern.

**Target files:**
- Replace/Modify: `src/storage/local.rs` -> `src/storage/local/mod.rs`
- Create: `src/storage/local/objects.rs`
- Create: `src/storage/local/listing.rs`
- Create: `src/storage/local/multipart.rs`
- Create: `src/storage/local/metadata.rs`
- Create optional: `src/storage/local/paths.rs`
- Modify if needed: `src/storage/mod.rs`

### Task 3.1: Create local storage module shell
**Objective:** split implementation without changing the public `LocalStorage` API.

**Steps:**
1. Rename `src/storage/local.rs` to `src/storage/local/mod.rs`.
2. Keep the public struct/type names stable.
3. Re-export or keep impl blocks in place so callers in API/runtime code do not change initially.

**Run:**
- `cargo test api::storage::tests -- --nocapture`
- `cargo test`

### Task 3.2: Extract path and key normalization helpers
**Objective:** isolate filesystem-safety and path derivation rules.

**Files:**
- Create optional: `src/storage/local/paths.rs`
- Modify: `src/storage/local/mod.rs`

**Steps:**
1. Move bucket/key normalization, path join, temp-file path, and upload-path derivation helpers.
2. Keep them private to `local` unless another storage backend would realistically reuse them.
3. Add focused unit tests if the path rules are currently only covered indirectly.

**Run:**
- `cargo test api::storage::tests::test_storage_is_scoped_per_user -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_copy_part_round_trip -- --nocapture`

### Task 3.3: Extract object CRUD implementation
**Objective:** isolate get/put/delete/head behavior from listing and multipart.

**Files:**
- Create: `src/storage/local/objects.rs`
- Modify: `src/storage/local/mod.rs`

**Steps:**
1. Move file read/write/delete/head helpers and metadata lookup used by direct object access.
2. Keep checksum/etag calculation close to object operations unless shared strongly elsewhere.
3. Preserve filesystem write semantics and atomicity rules.

**Run:**
- `cargo test api::storage::tests::test_s3_like_get_object_supports_single_byte_range -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_head_object_honors_conditional_headers -- --nocapture`

### Task 3.4: Extract listing and metadata/tagging support
**Objective:** isolate traversal/list queries from object mutation paths.

**Files:**
- Create: `src/storage/local/listing.rs`
- Create: `src/storage/local/metadata.rs`
- Modify: `src/storage/local/mod.rs`

**Steps:**
1. Move prefix traversal/listing logic into `listing.rs`.
2. Move metadata sidecar/tagging persistence and read/write helpers into `metadata.rs`.
3. Keep tagging storage format unchanged.

**Run:**
- `cargo test api::storage::tests::test_s3_like_list_objects_v2_supports_delimiter_common_prefixes -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_object_tagging_subresource_round_trip -- --nocapture`

### Task 3.5: Extract multipart upload persistence
**Objective:** isolate temporary upload/session state handling.

**Files:**
- Create: `src/storage/local/multipart.rs`
- Modify: `src/storage/local/mod.rs`

**Steps:**
1. Move multipart init/store/list/complete/abort persistence helpers into `multipart.rs`.
2. Keep object-finalization flow readable from the top-level impl.
3. Preserve cleanup semantics for aborted or completed uploads.

**Run:**
- `cargo test api::storage::tests::test_s3_like_multipart_upload_round_trip -- --nocapture`
- `cargo test api::storage::tests::test_s3_like_list_multipart_uploads_returns_active_uploads -- --nocapture`
- `cargo test`

**Commit suggestion:**
- `refactor: split local storage backend by concern`

---

## Phase 4: Split `src/functions/mod.rs`

**Objective:** separate sandbox execution orchestration from host-call implementations and response conversion helpers.

**Target files:**
- Replace/Modify: `src/functions/mod.rs` -> `src/functions/mod.rs` + children
- Create: `src/functions/runtime.rs`
- Create: `src/functions/host_calls.rs`
- Create optional: `src/functions/bindings_storage.rs`
- Create optional: `src/functions/bindings_data.rs`
- Create optional: `src/functions/bindings_push.rs`

### Task 4.1: Create functions runtime shell
**Objective:** keep the public runtime API stable while introducing internal modules.

**Steps:**
1. Add `mod runtime; mod host_calls;` declarations in `src/functions/mod.rs`.
2. Keep `execute_in_sandbox` and `SandboxExecutionRequest` publicly exported from the same path.
3. Move only imports and internal glue first; do not change behavior.

**Run:**
- `cargo test api::functions::tests -- --nocapture`
- `cargo test`

### Task 4.2: Extract sandbox process orchestration
**Objective:** isolate child-process lifecycle, stdout parsing, and final result assembly.

**Files:**
- Create: `src/functions/runtime.rs`
- Modify: `src/functions/mod.rs`

**Scope:**
- `execute_in_sandbox`
- stdout message structs
- node process invocation
- top-level result assembly

**Run:**
- `cargo test api::functions::tests::test_admin_can_create_and_invoke_function -- --nocapture`
- `cargo test api::functions::tests::test_function_supports_async_invocation_lifecycle -- --nocapture`

### Task 4.3: Extract host-call dispatch
**Objective:** isolate the host-call command router from the sandbox runner.

**Files:**
- Create: `src/functions/host_calls.rs`
- Modify: `src/functions/runtime.rs`
- Modify: `src/functions/mod.rs`

**Scope:**
- `handle_host_call`
- common JSON arg extraction helpers
- response-to-JSON conversion helpers
- claims enforcement helpers

**Steps:**
1. Move dispatch and argument parsing helpers together.
2. Keep `required_string`, `optional_string`, `require_claims`, and response conversion close to dispatch unless obviously reusable by bindings modules.
3. Keep the command name contract identical.

**Run:**
- `cargo test api::functions::tests::test_authenticated_function_can_use_storage_and_push_bindings -- --nocapture`
- `cargo test api::functions::tests::test_authenticated_function_can_use_data_row_bindings -- --nocapture`

### Task 4.4: Optional binding modules by subsystem
**Objective:** reduce single-file dispatch noise if host call handlers remain large after Task 4.3.

**Files:**
- Create optional: `src/functions/bindings_storage.rs`
- Create optional: `src/functions/bindings_data.rs`
- Create optional: `src/functions/bindings_push.rs`
- Modify: `src/functions/host_calls.rs`

**Steps:**
1. Move storage host-call handlers into `bindings_storage.rs`.
2. Move data row host-call handlers into `bindings_data.rs`.
3. Move push enqueue handler into `bindings_push.rs` only if it materially improves readability.
4. Avoid over-fragmenting if each binding file would be tiny.

**Run:**
- `cargo test api::functions::tests::test_authenticated_function_can_use_storage_and_push_bindings -- --nocapture`
- `cargo test api::functions::tests::test_authenticated_function_can_use_data_row_bindings -- --nocapture`
- `cargo test`

**Commit suggestion:**
- `refactor: split functions runtime and host calls`

---

## Phase 5: Split `src/api/push.rs`

**Objective:** separate subscription management, delivery enqueue, queue inspection, and shared push response types.

**Target files:**
- Replace/Modify: `src/api/push.rs` -> `src/api/push/mod.rs`
- Create: `src/api/push/subscriptions.rs`
- Create: `src/api/push/messages.rs`
- Create: `src/api/push/queue.rs`
- Create optional: `src/api/push/types.rs`
- Modify if needed: `src/api/mod.rs`
- Modify if needed: `src/app.rs`

### Task 5.1: Create push module shell
**Objective:** split the file without changing route wiring.

**Steps:**
1. Rename `src/api/push.rs` to `src/api/push/mod.rs`.
2. Add child modules and re-export current handlers.
3. Keep shared response/request structs in `mod.rs` temporarily.

**Run:**
- `cargo test api::push::tests -- --nocapture`
- `cargo test`

### Task 5.2: Extract subscription lifecycle
**Objective:** isolate browser/webpush subscription CRUD.

**Files:**
- Create: `src/api/push/subscriptions.rs`
- Modify: `src/api/push/mod.rs`

**Scope:**
- `list_subscriptions`
- `create_subscription`
- `delete_subscription`
- VAPID public key endpoint if its logic is tightly coupled to subscription setup

**Run:**
- `cargo test api::push::tests::test_subscription_crud_flow -- --nocapture`
- `cargo test api::push::tests::test_vapid_public_key_endpoint_or_equivalent -- --nocapture`

### Task 5.3: Extract message enqueue behavior
**Objective:** isolate request validation and enqueue logic from queue-inspection endpoints.

**Files:**
- Create: `src/api/push/messages.rs`
- Modify: `src/api/push/mod.rs`

**Scope:**
- `enqueue_message`
- payload validation helpers
- auth/admin checks local to enqueue path

**Run:**
- `cargo test api::push::tests::test_enqueue_message_requires_expected_permissions -- --nocapture`
- `cargo test api::push::tests::test_enqueue_message_persists_job -- --nocapture`

### Task 5.4: Extract queue inspection/status endpoints
**Objective:** isolate admin/ops read paths from write paths.

**Files:**
- Create: `src/api/push/queue.rs`
- Modify: `src/api/push/mod.rs`

**Scope:**
- `list_queue`
- `list_queue_stats`
- any queue row mapping helpers

**Run:**
- `cargo test api::push::tests::test_list_queue_and_stats -- --nocapture`
- `cargo test`

### Task 5.5: Optional shared types cleanup
**Objective:** keep `push/mod.rs` as an index, not a second monolith.

**Files:**
- Create optional: `src/api/push/types.rs`
- Modify: `src/api/push/mod.rs`

**Steps:**
1. Move request/response types used across more than one push child module into `types.rs`.
2. Keep one-owner structs local.
3. Move the `#[cfg(test)]` block out if it is large enough to matter.

**Run:**
- `cargo test api::push::tests -- --nocapture`
- `cargo test`

**Commit suggestion:**
- `refactor: split push api into focused modules`

---

## Phase 6: Cleanup and verification

**Objective:** ensure the second-pass refactor is structural only and leaves the repo easier to navigate.

**Files:**
- Modify if needed: `docs/plans/2026-04-30-peanut-refactor-priorities.md`
- Modify if needed: `docs/plans/2026-04-30-peanut-remaining-refactor-plan.md`
- Review: `src/api/mod.rs`
- Review: `src/storage/mod.rs`
- Review: `src/functions/mod.rs`

### Task 6.1: Export surface cleanup
**Steps:**
1. Remove unused `pub(crate)` exports that can be `pub(super)` or private.
2. Ensure each `mod.rs` reads like a clean subsystem index.
3. Remove dead imports and duplicate helper wrappers.

### Task 6.2: Test block cleanup
**Steps:**
1. Move large `#[cfg(test)]` blocks out of production-heavy files when it clearly improves readability.
2. Keep test helpers close to the subsystem; do not create global test utility sprawl.

### Task 6.3: Verification sweep
**Run:**
- `cargo fmt`
- `cargo test`
- `./scripts/build.sh`
- `git diff --stat HEAD~5..HEAD` (or equivalent range covering the active phase series)

### Task 6.4: Final review checklist
**Verify:**
- no public route contracts changed
- no runtime host-call names changed
- storage/auth/push/functions/local-storage each have clearer internal boundaries
- `mod.rs` files are indexes, not second monoliths
- no new abstraction layers were added without clear duplication pressure

**Commit suggestion:**
- `refactor: clean up backend module boundaries`

---

## Execution notes

- Prefer one commit per phase, not one giant batch.
- After each phase:
  1. run targeted tests first
  2. run full `cargo test`
  3. if routing or binary assembly changed, run `./scripts/build.sh`
- If a phase starts ballooning, stop and cut scope down to helper extraction only.
- When in doubt, keep logic near the owning subsystem instead of inventing a shared layer.

## Recommended commit sequence

1. `refactor: finish storage module split`
2. `refactor: split auth api into focused modules`
3. `refactor: split local storage backend by concern`
4. `refactor: split functions runtime and host calls`
5. `refactor: split push api into focused modules`
6. `refactor: clean up backend module boundaries`
