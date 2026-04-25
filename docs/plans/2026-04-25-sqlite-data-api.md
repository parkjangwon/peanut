# Peanut SQLite Data API Plan

> For Hermes: use subagent-driven-development skill to implement this plan task-by-task when delegation is useful.

Goal: add a small, safe SQLite-backed data API to Peanut without turning it into a raw SQL console or a general-purpose backend cloud.

Architecture: keep Peanut narrow and self-host oriented. Expose a constrained CRUD/query layer over app-owned tables instead of arbitrary SQL execution. Store schema metadata and row-level ownership rules in Peanut-managed tables, then generate a small set of authenticated JSON endpoints on top of SQLite.

Tech Stack: Rust, Axum, SQLx SQLite, Serde JSON, existing JWT/admin middleware, embedded Next.js console.

---

## Product position

What Peanut should become here:
- a small SQLite-backed application data API
- a self-hosted row/document layer with explicit schema and ownership rules
- a practical “Supabase-lite for one box” feature, but only for core CRUD

What Peanut should not become here:
- a public raw SQL endpoint
- a hosted multi-tenant Postgres competitor
- a plugin/orchestration framework
- an unbounded query engine with joins/DDL exposed to clients

## Recommended API shape

Prefer resource-oriented endpoints over SQL text execution.

### Phase 1 endpoints
- `GET /api/data/tables`
  - list Peanut-managed logical tables visible to the caller
- `POST /api/data/tables`
  - admin-only; create a logical table definition
- `GET /api/data/tables/:table`
  - return table schema/metadata
- `POST /api/data/tables/:table/rows`
  - insert one row
- `GET /api/data/tables/:table/rows`
  - list rows with small filters, pagination, ordering
- `GET /api/data/tables/:table/rows/:row_id`
  - fetch one row
- `PATCH /api/data/tables/:table/rows/:row_id`
  - partial update
- `DELETE /api/data/tables/:table/rows/:row_id`
  - delete one row

### Optional Phase 1 admin/ops endpoints
- `POST /api/data/tables/:table/query`
  - constrained filter DSL only, not SQL text
- `GET /api/data/audit`
  - admin-only recent mutations/events

### Explicitly avoid in Phase 1
- `POST /api/sql`
- arbitrary joins from clients
- arbitrary DDL from non-admin users
- user-defined triggers/functions
- exposing SQLite file internals directly

## Data model recommendation

Use Peanut-owned metadata + a generic row store first. Do not generate arbitrary SQLite tables in v1 unless absolutely necessary.

### New Peanut-managed tables
1. `data_tables`
   - `id TEXT PRIMARY KEY`
   - `name TEXT UNIQUE NOT NULL`
   - `display_name TEXT NOT NULL`
   - `schema_json TEXT NOT NULL`
   - `access_policy_json TEXT NOT NULL`
   - `created_by TEXT NOT NULL`
   - `created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP`

2. `data_rows`
   - `id TEXT PRIMARY KEY`
   - `table_id TEXT NOT NULL`
   - `owner_user_id TEXT`
   - `data_json TEXT NOT NULL`
   - `created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP`
   - `updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP`
   - foreign key to `data_tables(id)`

3. `data_row_events`
   - `id INTEGER PRIMARY KEY AUTOINCREMENT`
   - `table_id TEXT NOT NULL`
   - `row_id TEXT NOT NULL`
   - `actor_user_id TEXT NOT NULL`
   - `action TEXT NOT NULL` (`insert`, `update`, `delete`)
   - `diff_json TEXT`
   - `created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP`

### Why generic row store first
- keeps migrations small
- avoids runtime table-generation complexity
- keeps the product honest and reversible
- works well with Peanut’s current small-scope philosophy
- lets us validate product demand before moving to typed physical tables

## Schema contract recommendation

Each logical table schema should be stored as JSON like:

```json
{
  "fields": {
    "title": { "type": "string", "required": true, "max_length": 200 },
    "done": { "type": "boolean", "required": true, "default": false },
    "priority": { "type": "integer", "required": false },
    "due_at": { "type": "datetime", "required": false }
  },
  "indexes": [
    ["done"],
    ["priority", "due_at"]
  ]
}
```

Supported field types for Phase 1:
- `string`
- `integer`
- `number`
- `boolean`
- `datetime`
- `json`

Do not add relations, joins, or nested policy expressions in v1.

## Access policy recommendation

Each logical table should declare one of a few fixed policy modes.

### Policy modes
1. `admin_only`
   - only admins can read/write
2. `owner_private`
   - rows have `owner_user_id`
   - users can only read/write their own rows
   - admins can read all rows
3. `authenticated_read_owner_write`
   - all authenticated users can read
   - only row owner/admin can modify
4. `authenticated_shared_rw`
   - any authenticated user can read/write
   - useful for tiny internal tools only

Represent this as fixed JSON config, for example:

```json
{
  "mode": "owner_private"
}
```

Avoid arbitrary policy scripting in v1.

## Query contract recommendation

For `GET /api/data/tables/:table/rows`, support only a small query surface:
- `limit` default 50, max 200
- `cursor` for pagination
- `order_by` limited to declared fields plus `created_at` and `updated_at`
- `order` = `asc` or `desc`
- `filter[field][op]=value`

Allowed ops in v1:
- `eq`
- `ne`
- `lt`
- `lte`
- `gt`
- `gte`
- `contains` for strings only
- `in` with small bounded lists

Reject:
- raw SQL fragments
- unbounded scans over huge limits
- filtering on undeclared fields

## Console UX recommendation

Add a new `Data` panel to the embedded console.

### Minimal console capabilities
- list logical tables
- inspect one schema
- create a row
- browse rows with limit/order/filter
- edit/delete a row
- show effective policy mode

### Admin-only console capabilities
- create table
- edit schema metadata
- inspect row mutation log

Do not build a spreadsheet-grade UI yet.

## Security rules

1. Never accept raw SQL from clients.
2. Always validate table names against Peanut-managed metadata.
3. Always validate incoming row JSON against stored schema.
4. Enforce policy mode on every read/write path.
5. Bound result sizes and filter complexity.
6. Record insert/update/delete events in `data_row_events`.
7. Return structured JSON errors only.
8. Keep all routes behind existing auth middleware.
9. Make table creation admin-only.
10. Reserve schema migration/DDL for server-side code only.

## Suggested file layout

- Create: `src/api/data.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`
- Create: `src/data/mod.rs`
- Create: `src/data/schema.rs`
- Create: `src/data/policy.rs`
- Create: `src/data/store.rs`
- Create: `migrations/202604250003_add_data_api_tables.sql`
- Modify: `peanut-console/src/app/console-client.tsx`
- Modify: `README.md`
- Modify: `README.ko.md`

## Implementation plan

### Task 1: Add failing tests for table metadata listing
Objective: define the first stable API contract for the new Data module.

Files:
- Create/Modify: `src/api/data.rs`
- Test: `src/api/data.rs` test module or `tests/data_api.rs`

Steps:
1. Write a failing test for `GET /api/data/tables` returning an empty list for a fresh DB.
2. Run the test to verify failure.
3. Add minimal response structs and handler.
4. Run the test again until it passes.
5. Commit.

### Task 2: Add metadata migrations and storage layer
Objective: introduce `data_tables`, `data_rows`, and `data_row_events`.

Files:
- Create: `migrations/202604250003_add_data_api_tables.sql`
- Create: `src/data/store.rs`
- Create: `src/data/mod.rs`

Steps:
1. Write a failing migration/init test asserting the new tables exist.
2. Add the migration.
3. Add typed SQLx row structs/helpers for the metadata tables.
4. Re-run the test until it passes.
5. Commit.

### Task 3: Add admin-only table creation
Objective: let admins define a logical table with schema + fixed access policy.

Files:
- Create/Modify: `src/api/data.rs`
- Create: `src/data/schema.rs`
- Create: `src/data/policy.rs`

Steps:
1. Write a failing test for admin table creation.
2. Write a failing test that non-admin users receive `403`.
3. Implement request validation for table name/schema/policy.
4. Persist metadata into `data_tables`.
5. Re-run tests until both pass.
6. Commit.

### Task 4: Add row insert with schema validation
Objective: allow inserts only when payload matches the declared schema.

Files:
- Modify: `src/api/data.rs`
- Modify: `src/data/schema.rs`
- Modify: `src/data/store.rs`

Steps:
1. Write a failing test for a valid row insert.
2. Write a failing test for invalid field type / missing required field.
3. Implement schema validation.
4. Store row JSON in `data_rows`.
5. Re-run tests until they pass.
6. Commit.

### Task 5: Add owner-aware policy enforcement
Objective: make `owner_private` rows visible only to owners/admins.

Files:
- Modify: `src/api/data.rs`
- Modify: `src/data/policy.rs`
- Test: policy-focused route tests

Steps:
1. Write failing tests for owner access and cross-user denial.
2. Implement read/write policy checks.
3. Ensure admins can override as intended.
4. Re-run tests until they pass.
5. Commit.

### Task 6: Add row list/get/update/delete routes
Objective: complete the CRUD surface for one logical table.

Files:
- Modify: `src/api/data.rs`
- Modify: `src/main.rs`
- Modify: `src/api/mod.rs`

Steps:
1. Add failing tests for list/get/update/delete.
2. Implement minimal handlers.
3. Add structured JSON responses and errors.
4. Re-run tests until they pass.
5. Commit.

### Task 7: Add bounded filter/order/pagination support
Objective: provide practical browsing without exposing SQL.

Files:
- Modify: `src/api/data.rs`
- Modify: `src/data/store.rs`

Steps:
1. Add failing tests for `limit`, `order_by`, and one simple equality filter.
2. Implement a small server-side filter compiler from query params to fixed SQL fragments.
3. Reject undeclared fields and unsupported operators.
4. Re-run tests until they pass.
5. Commit.

### Task 8: Add mutation audit log
Objective: keep inserts/updates/deletes inspectable for operations and debugging.

Files:
- Modify: `src/data/store.rs`
- Modify: `src/api/data.rs`

Steps:
1. Add failing tests asserting events are written on insert/update/delete.
2. Implement event recording.
3. Add an admin-only event list endpoint if still justified.
4. Re-run tests.
5. Commit.

### Task 9: Add embedded console Data panel
Objective: make the feature operable without external tooling.

Files:
- Modify: `peanut-console/src/app/console-client.tsx`

Steps:
1. Add a failing UI-level manual checklist for listing tables, creating a row, and browsing rows.
2. Implement a narrow Data panel.
3. Keep the UX simple and admin-first.
4. Run `npm run lint` and `npm run build`.
5. Commit.

### Task 10: Update docs and operator guidance
Objective: describe the feature honestly and keep non-goals clear.

Files:
- Modify: `README.md`
- Modify: `README.ko.md`

Steps:
1. Document that Peanut offers a constrained SQLite data API, not raw SQL.
2. Document policy modes, limits, and non-goals.
3. Add backup guidance noting the feature lives inside the same SQLite DB file.
4. Commit.

## Verification checklist

Before calling the feature done:
- backend tests pass
- console lint/build pass
- `./scripts/build.sh` passes
- non-admin table creation is forbidden
- owner-private rows are isolated across two users
- invalid schema/row payloads return structured 400 errors
- large/unbounded queries are rejected
- mutation events are recorded
- README stays explicit that Peanut is not exposing arbitrary SQL

## Recommendation

The right first move is not raw SQL. The right first move is:
1. table metadata
2. fixed policy modes
3. generic JSON row store
4. bounded CRUD/query API

If this gets real usage and obvious pressure for richer relational features later, then Peanut can evaluate generated physical tables or a stronger query layer. Until then, keep it small.
