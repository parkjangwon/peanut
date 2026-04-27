#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${ADMIN_JWT:?Set ADMIN_JWT to an admin bearer token}"

curl -s -X POST "$BASE_URL/api/admin/service-tokens"   -H "authorization: Bearer $ADMIN_JWT"   -H 'content-type: application/json'   --data @examples/service-tokens/create-token.json
