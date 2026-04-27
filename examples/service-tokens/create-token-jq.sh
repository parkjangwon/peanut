#!/usr/bin/env bash
set -euo pipefail

command -v jq >/dev/null 2>&1 || {
  echo 'jq is required for create-token-jq.sh' >&2
  exit 1
}

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${ADMIN_JWT:?Set ADMIN_JWT to an admin bearer token}"

response=$(curl -s -X POST "$BASE_URL/api/admin/service-tokens" \
  -H "authorization: Bearer $ADMIN_JWT" \
  -H 'content-type: application/json' \
  --data @examples/service-tokens/create-token.json)

echo "$response" | jq .
echo
printf 'export TOKEN_ID=%q\n' "$(echo "$response" | jq -r '.service_token.id')"
printf 'export SERVICE_TOKEN=%q\n' "$(echo "$response" | jq -r '.token')"
