#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"

command -v jq >/dev/null 2>&1 || {
  echo 'jq is required for scripts/check-openapi.sh' >&2
  exit 1
}

npx --yes @redocly/cli@latest lint docs/openapi.yaml

health_json=$(curl -s "$BASE_URL/api/health")
echo "$health_json" | jq -e '.status == "ok" and (.message | type == "string")' >/dev/null

ready_json=$(curl -s "$BASE_URL/api/ready")
echo "$ready_json" | jq -e '.status == "ready" and (.checks | type == "array")' >/dev/null
echo "$ready_json" | jq -e '.checks[] | select(.name == "database" and (.ok | type == "boolean"))' >/dev/null
echo "$ready_json" | jq -e '.checks[] | select(.name == "storage" and (.ok | type == "boolean"))' >/dev/null
echo "$ready_json" | jq -e '.checks[] | select(.name == "functions" and (.enabled | type == "boolean"))' >/dev/null

echo "OpenAPI lint and live contract checks passed."
