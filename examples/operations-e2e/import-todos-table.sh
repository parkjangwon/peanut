#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"
: "${TABLE_NAME:=ops_todos}"
: "${IMPORT_FILE:=examples/operations-e2e/todos-import-replace.json}"

curl -s -X POST "$BASE_URL/api/data/tables/$TABLE_NAME/import" \
  -H "authorization: Bearer $SERVICE_TOKEN" \
  -H 'content-type: application/json' \
  --data @"$IMPORT_FILE"
