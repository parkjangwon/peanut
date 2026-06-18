#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${APP_ID:=default}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"
: "${TABLE_NAME:=ops_todos}"

curl -s -X POST "$BASE_URL/api/apps/$APP_ID/data/tables/$TABLE_NAME/rows" \
  -H "authorization: Bearer $SERVICE_TOKEN" \
  -H 'content-type: application/json' \
  --data @examples/operations-e2e/todo-row.json