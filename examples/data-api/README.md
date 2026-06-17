# Peanut Data API example payloads

This folder contains copy-pasteable request payloads for the bounded Peanut Data API.

Scenarios included:
- `todos/`
  - per-user task lists with `owner_private`
- `contacts/`
  - shared authenticated CRM-lite data with `authenticated_shared_rw`
- `posts/`
  - simple content backend flows with presets and import payloads

Suggested usage:
1. create a table with `create-table.json`
2. insert rows with `create-row*.json`
3. update rows with `update-row*.json`
4. save repeated views with `preset-*.json`
5. seed or restore rows with `import-*.json`

Example:

```bash
export BASE_URL=http://127.0.0.1:3000
export APP_ID=default
export APP_KEY='<PASTE_APP_API_KEY>'
export TOKEN='<PASTE_ACCESS_TOKEN>'

curl -s -X POST "$BASE_URL/api/apps/$APP_ID/data/tables" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  --data @examples/data-api/todos/create-table.json

curl -s -X POST "$BASE_URL/api/apps/$APP_ID/data/tables/todos/rows" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  --data @examples/data-api/todos/create-row-buy-milk.json
```

Use `http://127.0.0.1:3492` for `BASE_URL` when using the default Docker
Compose package.
