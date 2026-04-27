#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"

curl -s -X POST "$BASE_URL/api/data/tables"   -H "authorization: Bearer $SERVICE_TOKEN"   -H 'content-type: application/json'   --data @examples/operations-e2e/todos-table.json
