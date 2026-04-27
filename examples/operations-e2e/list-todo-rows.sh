#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"
: "${TABLE_NAME:=ops_todos}"

curl -s "$BASE_URL/api/data/tables/$TABLE_NAME/rows?order_by=created_at&order=desc&limit=20&offset=0"   -H "authorization: Bearer $SERVICE_TOKEN"
