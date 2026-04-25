# Peanut Functions MVP Implementation Plan

> For Hermes: follow TDD where behavior changes, keep the scope narrow, and implement the smallest end-to-end slice that proves the product.

Goal: add Peanut Functions as a minimal JS/TS-only sandboxed function runtime managed from the embedded web console and invokable through external API calls.

Architecture:
- Store function metadata + source in SQLite.
- Execute functions in a separate Node subprocess with timeout, isolated environment, temp working directory, and JSON-only input/output contract.
- Keep v0 scope narrow: admin-managed functions, authenticated invoke endpoint, sync execution, invocation logs, no background queue, no external package install.

Tech stack:
- Rust + Axum + SQLx + Tokio process management
- Node.js runtime already present on host
- Next.js console client

## MVP scope
- Admin can create/update/delete/list functions from console/API.
- Each function has:
  - name
  - display_name
  - endpoint_slug
  - runtime (`javascript` or `typescript`)
  - source_code
  - timeout_ms
  - enabled
- External clients invoke with `POST /api/functions/:name/invoke`.
- Runtime contract:
  - default export or named `handler`
  - receives `{ request, auth, function }`
  - must return JSON-serializable value
- Sandbox constraints:
  - separate Node process per invocation
  - `env_clear()` then explicit allowlist only
  - temporary working directory
  - timeout kill
  - bounded stdout/stderr capture
- Invocation log stored in SQLite with status, duration, error, request/response JSON.

## Non-goals for v0
- arbitrary npm package install
- cron/data triggers
- public unauthenticated invoke
- streaming responses
- durable background execution
- full VM/container isolation

## File plan
- Create: `migrations/202604250004_add_functions_tables.sql`
- Create: `src/api/functions.rs`
- Create: `src/functions/mod.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/db.rs` tests if needed
- Modify: `src/test_support.rs` if helper expansion is needed
- Modify: `peanut-console/src/app/console-client.tsx`
- Modify: `README.md`
- Modify: `README.ko.md`

## Backend tasks
1. Add DB tables for functions + invocations.
2. Add failing API tests for admin CRUD and invoke behavior.
3. Implement function metadata model + validation.
4. Implement Node subprocess sandbox runner.
5. Implement invoke endpoint and invocation persistence.
6. Add list/detail/invocation log endpoints.

## Console tasks
1. Add client-side types for functions and invocations.
2. Add refresh/load helpers.
3. Add Functions panel for create/update/delete.
4. Add inline invoke form and recent invocation list.
5. Show derived invoke endpoint in console.

## Verification
- `cargo test`
- `cd peanut-console && npm run lint`
- `cd peanut-console && npm run build`
- `./scripts/build.sh`
