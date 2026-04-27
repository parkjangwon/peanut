#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN to a plaintext pst_... token}"

curl -s "$BASE_URL/api/admin/users"   -H "authorization: Bearer $SERVICE_TOKEN"
