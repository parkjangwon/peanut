# Peanut Data API Guide

Peanut ships a bounded SQLite-backed Data API for app-facing CRUD and operator workflows.

This guide focuses on:
- the table/row model
- the supported query surface
- practical example schemas
- replay/checkpoint/SSE flows

See also:
- `README.md`
- `docs/auth-client.md`
- `docs/data-api.ko.md`
- `examples/data-api/`

## 1. Product boundary

Peanut Data API is intentionally narrow.

It is designed for:
- self-hosted app backends
- admin-defined logical tables
- schema-aware row CRUD
- bounded filtering, sorting, and pagination
- operator-friendly replay/export/import flows

It is not designed for:
- raw SQL over HTTP
- arbitrary joins
- generic database-console behavior
- free-form query execution

## 2. Core model

Current shape:
- admins create logical tables
- each table has:
  - `name`
  - `display_name`
  - `schema`
  - `access_policy`
- rows are stored in Peanut-managed SQLite tables
- row mutations are written to an internal event log

Useful access policies today:
- `owner_private`
  - each row belongs to an authenticated user
  - normal row access is isolated per owner
- `authenticated_shared_rw`
  - authenticated users can work against a shared table
- `admin_only`
  - operator-only table surface

## 3. Endpoints

Table endpoints:
- `GET /api/data/tables`
- `POST /api/data/tables`
- `GET /api/data/tables/:table`
- `PATCH /api/data/tables/:table`
- `DELETE /api/data/tables/:table`

Row endpoints:
- `GET /api/data/tables/:table/rows`
- `POST /api/data/tables/:table/rows`
- `GET /api/data/tables/:table/rows/:row_id`
- `PATCH /api/data/tables/:table/rows/:row_id`
- `DELETE /api/data/tables/:table/rows/:row_id`

Event/replay endpoints:
- `GET /api/data/tables/:table/events`
- `GET /api/data/tables/:table/events/checkpoint`
- `GET /api/data/tables/:table/events/stream`

Preset/export/import endpoints:
- `GET /api/data/tables/:table/presets`
- `POST /api/data/tables/:table/presets`
- `GET /api/data/tables/:table/presets/:preset_id/run`
- `PATCH /api/data/tables/:table/presets/:preset_id`
- `DELETE /api/data/tables/:table/presets/:preset_id`
- `GET /api/data/tables/:table/export`
- `POST /api/data/tables/:table/import`

## 4. Quick example: todos table

Create a table:

```json
{
  "name": "todos",
  "display_name": "Todos",
  "schema": {
    "fields": {
      "title": { "type": "string", "required": true, "max_length": 200 },
      "done": { "type": "boolean", "required": false, "default": false },
      "priority": { "type": "integer", "required": false, "default": 0 }
    }
  },
  "access_policy": { "mode": "owner_private" }
}
```

Insert a row:

```json
{
  "data": {
    "title": "buy milk",
    "priority": 2
  }
}
```

Typical row response shape:

```json
{
  "row": {
    "id": "uuid",
    "owner_user_id": "uuid",
    "data": {
      "title": "buy milk",
      "done": false,
      "priority": 2
    },
    "created_at": "2026-04-27 12:00:00",
    "updated_at": "2026-04-27 12:00:00"
  }
}
```

## 5. Supported query surface

`GET /api/data/tables/:table/rows` supports a bounded query contract.

Useful params:
- `limit`
- `offset`
- `order_by`
- `order=asc|desc`
- `search`
- `filter_field`
- `filter_op`
- `filter_value`

Current string filter ops:
- `eq`
- `ne`
- `contains`
- `starts_with`
- `ends_with`

Other currently supported filter shapes:
- `integer`, `number`, `datetime`: `eq`, `ne`, `gt`, `gte`, `lt`, `lte`
- `boolean`: `eq`, `ne`
- `json`: `eq`, `ne`

Important behavior:
- `search` scans only declared string fields in the table schema
- `offset` is applied after filtering and sorting
- unknown `order_by` values are rejected
- `filter_field`, `filter_op`, and `filter_value` must be provided together

Example:

```bash
curl -s "$BASE_URL/api/data/tables/todos/rows?search=buy&filter_field=title&filter_op=starts_with&filter_value=buy&order_by=title&order=asc&limit=10&offset=0" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

## 6. Example schemas worth copying

Ready-to-send payload files for these examples live in `examples/data-api/`.


### A. Todos

Good for:
- per-user personal task lists
- validating `owner_private`
- testing string filters and defaults

Suggested fields:
- `title: string`
- `done: boolean`
- `priority: integer`
- `due_at: datetime`

### B. Contacts

Good for:
- shared internal CRM-lite workflows
- validating sort and search behavior

Suggested access policy:
- `authenticated_shared_rw`

Suggested fields:
- `name: string`
- `email: string`
- `company: string`
- `notes: string`

### C. Posts

Good for:
- simple content backends
- draft/publish state
- export/import flows

Suggested fields:
- `title: string`
- `slug: string`
- `body: string`
- `status: string`

## 7. Safe schema evolution rules

`PATCH /api/data/tables/:table` is intentionally conservative.

Current rules:
- field types cannot change in place
- non-empty tables cannot drop existing fields
- new required fields on non-empty tables must define defaults

That means this is okay:
- add an optional field
- add a required field with a default
- change `display_name`

And this is rejected:
- `title: string -> integer`
- removing `done` after rows already exist
- adding `priority` as required without a default when rows already exist

## 8. Replay and realtime

Peanut gives two complementary row-event flows.

### A. Checkpoint + replay

Use this when a sync worker or operator process needs a durable resume point.

1. Read the current checkpoint:

```bash
curl -s "$BASE_URL/api/data/tables/todos/events/checkpoint" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

Typical response:

```json
{
  "table_name": "todos",
  "latest_event_id": 42
}
```

2. Later, replay only newer events:

```bash
curl -s "$BASE_URL/api/data/tables/todos/events?since_id=42&limit=50" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

Use this for:
- resume-after-restart workers
- admin sync tools
- recovery after SSE disconnects

### B. SSE stream

Use this when you want live row-mutation events.

```bash
curl -N "$BASE_URL/api/data/tables/todos/events/stream" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

Practical pattern:
1. load checkpoint
2. attach SSE stream
3. persist the latest seen event id
4. if the stream drops, replay with `since_id=<last_seen_id>`

## 9. Query presets

Presets are useful when operators or tools repeat the same filtered view.

Good uses:
- open todos
- high-priority contacts
- draft posts

Treat presets as saved bounded queries, not a custom report engine.

## 10. Export and import

Export:
- `GET /api/data/tables/:table/export`

Import:
- `POST /api/data/tables/:table/import`

Useful modes:
- `append`
- `replace`

Safety preview:
- add `dry_run: true` to validate checksum/schema/rows without mutating the database
- dry-run responses include `would_insert`, `would_replace`, `schema_changes`, and `validation_errors`

Practical payload files:
- `examples/data-api/todos/`
- `examples/data-api/contacts/`
- `examples/data-api/posts/`

Practical uses:
- backups
- environment seeding
- fixture replay
- restoring a known-good table snapshot

## 11. Practical recommendations

If you are starting a new app on Peanut Data API:
1. start with `owner_private` or `authenticated_shared_rw`
2. keep schemas small and explicit
3. rely on defaults for additive schema changes
4. use replay/checkpoint for any sync worker
5. use query presets for repeated operator views

## 12. Current non-goals

Still out of scope:
- `/api/sql`
- arbitrary joins
- unbounded full-text search
- custom database extensions exposed directly to clients
