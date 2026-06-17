#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${APP_ID:=default}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"
: "${STORAGE_BUCKET:=assets}"
: "${STORAGE_KEY:=ops/hello.txt}"

curl -sI "$BASE_URL/api/apps/$APP_ID/storage/buckets/$STORAGE_BUCKET/objects/$STORAGE_KEY"   -H "authorization: Bearer $SERVICE_TOKEN"
