#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${APP_ID:=default}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"
: "${STORAGE_BUCKET:=assets}"

status=$(curl -sS -o /tmp/peanut-ensure-storage-bucket.json -w "%{http_code}" \
  -X POST "$BASE_URL/api/apps/$APP_ID/storage/buckets" \
  -H "authorization: Bearer $SERVICE_TOKEN" \
  -H 'content-type: application/json' \
  --data "{\"name\":\"$STORAGE_BUCKET\",\"public_read\":false,\"allow_client_uploads\":true,\"allowed_mime_types\":[]}")

if [ "$status" = "201" ] || [ "$status" = "409" ]; then
  exit 0
fi

echo "[ensure storage bucket] failed with HTTP $status:" >&2
cat /tmp/peanut-ensure-storage-bucket.json >&2 || true
exit 1