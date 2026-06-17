# Peanut Data API Guide

Peanut Data is app-scoped. Admins define tables through
`/api/apps/:app_id/data/...`, and application clients read/write rows through
the same app path with an `X-Peanut-Api-Key`.

## Routes

Admin routes:

- `POST /api/apps/:app_id/data/tables`
- `PATCH /api/apps/:app_id/data/tables/:table`
- `DELETE /api/apps/:app_id/data/tables/:table`
- `GET /api/apps/:app_id/data/tables/:table/events`
- `GET /api/apps/:app_id/data/tables/:table/events/checkpoint`
- `GET /api/apps/:app_id/data/tables/:table/events/stream`
- `GET /api/apps/:app_id/data/tables/:table/presets`
- `POST /api/apps/:app_id/data/tables/:table/presets`
- `GET /api/apps/:app_id/data/tables/:table/presets/:preset_id/run`
- `PATCH /api/apps/:app_id/data/tables/:table/presets/:preset_id`
- `DELETE /api/apps/:app_id/data/tables/:table/presets/:preset_id`
- `GET /api/apps/:app_id/data/tables/:table/export`
- `POST /api/apps/:app_id/data/tables/:table/import`

SDK routes:

- `GET /api/apps/:app_id/data/tables`
- `GET /api/apps/:app_id/data/tables/:table`
- `GET /api/apps/:app_id/data/tables/:table/rows`
- `POST /api/apps/:app_id/data/tables/:table/rows`
- `GET /api/apps/:app_id/data/tables/:table/rows/:row_id`
- `PATCH /api/apps/:app_id/data/tables/:table/rows/:row_id`
- `DELETE /api/apps/:app_id/data/tables/:table/rows/:row_id`
- `POST /api/apps/:app_id/data/query`

## Example Table

```json
{
  "name": "todos",
  "display_name": "Todos",
  "schema": {
    "fields": {
      "title": { "type": "string", "required": true, "max_length": 200 },
      "done": { "type": "boolean", "default": false }
    }
  },
  "access_policy": { "mode": "authenticated_shared_rw" }
}
```

Field specs may also declare database-style guardrails:

```json
{
  "name": "orders",
  "display_name": "Orders",
  "schema": {
    "fields": {
      "order_number": { "type": "string", "required": true, "unique": true },
      "account_id": {
        "type": "string",
        "required": true,
        "reference": { "table": "accounts" }
      }
    }
  },
  "access_policy": {
    "mode": "custom",
    "rules": {
      "read": { "allow": "owner" },
      "create": { "allow": "authenticated" },
      "update": { "allow": "owner" },
      "delete": { "allow": "admin" }
    }
  }
}
```

`unique` is enforced per Peanut table before rows are created or updated.
`reference.table` points at another table in the same app and expects the field
value to be that table's row id. Custom rules accept `admin`, `authenticated`,
or `owner` for each CRUD operation.

## Example Row

```json
{
  "data": {
    "title": "buy milk",
    "done": false
  }
}
```

## Query Surface

`GET /api/apps/:app_id/data/tables/:table/rows` supports bounded list queries:

- `limit`
- `offset`
- `order_by`
- `order=asc|desc`
- `search`
- `title_contains`
- `filter_field`
- `filter_op`
- `filter_value`

Supported filter operators are schema-aware. Strings support `eq`, `ne`,
`contains`, `starts_with`, and `ends_with`; numbers and datetimes support
comparison operators; booleans support equality checks.

Every table, row, preset, import/export, and event query is scoped by `app_id`.
Different apps can safely use the same table names.

The SQL endpoint accepts one ANSI-style `SELECT`, `INSERT`, `UPDATE`, or
`DELETE` statement. It runs through the same schema validation, access policy,
unique, reference, audit, and event paths as the row CRUD APIs. `UPDATE` and
`DELETE` require `WHERE id = ...`.
