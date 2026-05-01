#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${NPM_CONFIG_CACHE:=${TMPDIR:-/tmp}/peanut-npm-cache}"
export NPM_CONFIG_CACHE

command -v jq >/dev/null 2>&1 || {
  echo 'jq is required for scripts/check-openapi.sh' >&2
  exit 1
}

npx --yes @redocly/cli@latest lint docs/openapi.yaml
grep -q '/api/apps/{app_id}/function-endpoints/{endpoint_slug}:' docs/openapi.yaml
if grep -Eq '^  /api/(auth|data|storage|push|functions)(/|:)' docs/openapi.yaml; then
  echo "legacy global app API route found in OpenAPI contract" >&2
  exit 1
fi

health_json=$(curl -s "$BASE_URL/api/health")
echo "$health_json" | jq -e '.status == "ok" and (.message | type == "string")' >/dev/null

ready_json=$(curl -s "$BASE_URL/api/ready")
echo "$ready_json" | jq -e '.status == "ready" and (.checks | type == "array")' >/dev/null
echo "$ready_json" | jq -e '.checks[] | select(.name == "database" and (.ok | type == "boolean"))' >/dev/null
echo "$ready_json" | jq -e '.checks[] | select(.name == "storage" and (.ok | type == "boolean"))' >/dev/null
echo "$ready_json" | jq -e '.checks[] | select(.name == "functions" and (.enabled | type == "boolean"))' >/dev/null

if [ -n "${ADMIN_JWT:-}" ]; then
  backups_json=$(curl -fsS "$BASE_URL/api/admin/backups" \
    -H "authorization: Bearer $ADMIN_JWT")
  echo "$backups_json" | jq -e '
    (.backups | type == "array") and
    (has("restore_pending")) and
    ((.restore_pending == null) or (.restore_pending.exists | type == "boolean"))
  ' >/dev/null

  metrics_json=$(curl -fsS "$BASE_URL/api/admin/ops/metrics" \
    -H "authorization: Bearer $ADMIN_JWT")
  echo "$metrics_json" | jq -e '
    (.database.size_bytes | type == "number") and
    (.database.restore_pending | type == "boolean") and
    (.storage.ok | type == "boolean") and
    (.storage.multipart_stale_count | type == "number") and
    (.push.queued | type == "number") and
    (.functions.enabled | type == "boolean") and
    (.functions.running_limit | type == "number") and
    (.system.version | type == "string") and
    (.system.uptime_seconds | type == "number")
  ' >/dev/null
else
  echo "ADMIN_JWT not set; skipping authenticated live contract checks." >&2
fi

echo "OpenAPI lint and live contract checks passed."
