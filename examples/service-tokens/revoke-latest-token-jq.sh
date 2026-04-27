#!/usr/bin/env bash
set -euo pipefail

command -v jq >/dev/null 2>&1 || {
  echo 'jq is required for revoke-latest-token-jq.sh' >&2
  exit 1
}

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${ADMIN_JWT:?Set ADMIN_JWT to an admin bearer token}"

TOKEN_ID=$(curl -s "$BASE_URL/api/admin/service-tokens" \
  -H "authorization: Bearer $ADMIN_JWT" | jq -r '.service_tokens[0].id // empty')

if [ -z "$TOKEN_ID" ]; then
  echo 'No service token found to revoke.' >&2
  exit 1
fi

curl -s -X DELETE "$BASE_URL/api/admin/service-tokens/$TOKEN_ID" \
  -H "authorization: Bearer $ADMIN_JWT"
