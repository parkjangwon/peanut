#!/usr/bin/env bash
set -euo pipefail

command -v jq >/dev/null 2>&1 || {
  echo 'jq is required for bootstrap-service-token-jq.sh' >&2
  exit 1
}

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${ADMIN_JWT:?Set ADMIN_JWT to an admin bearer token}"

response=$(./examples/service-tokens/create-token.sh)

echo "$response" | jq .
echo
printf 'export SERVICE_TOKEN=%q\n' "$(echo "$response" | jq -r '.token')"
printf 'export TOKEN_ID=%q\n' "$(echo "$response" | jq -r '.service_token.id')"
echo './examples/operations-e2e/create-todos-table.sh'
echo './examples/operations-e2e/create-todo-row.sh'
echo './examples/operations-e2e/upload-storage-object.sh'
echo './examples/operations-e2e/head-storage-object.sh'
