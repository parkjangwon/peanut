#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILES="${COMPOSE_FILES:-${COMPOSE_FILE:-docker-compose.yml}}"
BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
SERVICE="${SERVICE:-peanut}"
ADMIN_TOKEN="${PEANUT_ADMIN_TOKEN:-}"
APP_ID="${PEANUT_VERIFY_APP_ID:-default}"
BOOTSTRAP_EMAIL="${PEANUT_BOOTSTRAP_EMAIL:-}"
BOOTSTRAP_PASSWORD="${PEANUT_BOOTSTRAP_PASSWORD:-}"
RUN_ID="$(python3 -c 'import time; print(time.time_ns())')"
SMOKE_TABLE="verify_${RUN_ID}"
SMOKE_BUCKET="verify-${RUN_ID}"
SMOKE_FUNCTION="verify_${RUN_ID}"
SMOKE_FUNCTION_SLUG="verify-${RUN_ID}"

json_value() {
  python3 -c 'import json,sys; data=json.load(sys.stdin); cur=data
for part in sys.argv[1].split("."):
    cur=cur[part]
print(cur)' "$1"
}

curl_expect_status() {
  local expected="$1"
  local output="$2"
  shift 2
  local status
  status="$(curl -sS -o "${output}" -w "%{http_code}" "$@")"
  if [[ "${status}" != "${expected}" ]]; then
    echo "expected HTTP ${expected}, got ${status}: $*" >&2
    cat "${output}" >&2 || true
    exit 1
  fi
}

COMPOSE_ARGS=()
for compose_file in ${COMPOSE_FILES}; do
  COMPOSE_ARGS+=("-f" "${compose_file}")
done

echo "==> Building and starting Peanut with ${COMPOSE_FILES}"
docker compose "${COMPOSE_ARGS[@]}" up -d --build "${SERVICE}"

echo "==> Waiting for readiness at ${BASE_URL}/api/ready"
for _ in $(seq 1 60); do
  if curl -fsS "${BASE_URL}/api/ready" >/tmp/peanut-ready.json; then
    if grep -q '"ready":true' /tmp/peanut-ready.json; then
      echo "ready: ok"
      break
    fi
  fi
  sleep 2
done

if ! grep -q '"ready":true' /tmp/peanut-ready.json 2>/dev/null; then
  echo "readiness failed"
  docker compose "${COMPOSE_ARGS[@]}" logs --tail=200 "${SERVICE}"
  exit 1
fi

echo "==> Verifying Docker image has Deno"
docker compose "${COMPOSE_ARGS[@]}" exec -T "${SERVICE}" deno --version >/tmp/peanut-deno.txt
cat /tmp/peanut-deno.txt

if [[ -z "${ADMIN_TOKEN}" && -n "${BOOTSTRAP_EMAIL}" && -n "${BOOTSTRAP_PASSWORD}" ]]; then
  echo "==> Bootstrapping first admin"
  curl -fsS -X POST "${BASE_URL}/api/bootstrap/admin" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${BOOTSTRAP_EMAIL}\",\"password\":\"${BOOTSTRAP_PASSWORD}\"}" \
    >/tmp/peanut-bootstrap-admin.json
  ADMIN_TOKEN="$(json_value access_token </tmp/peanut-bootstrap-admin.json)"
fi

if [[ -n "${ADMIN_TOKEN}" ]]; then
  echo "==> Verifying workspace invite setup"
  WORKSPACE_SUFFIX="${RUN_ID}"
  WORKSPACE_EMAIL="workspace-verify-${WORKSPACE_SUFFIX}@example.com"
  curl -fsS -X POST "${BASE_URL}/api/admin/workspace-invites" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"label":"compose verifier workspace invite","max_uses":1}' \
    >/tmp/peanut-workspace-invite.json
  WORKSPACE_INVITE_CODE="$(json_value invite_code </tmp/peanut-workspace-invite.json)"
  curl -fsS -X POST "${BASE_URL}/api/workspace-invites/accept" \
    -H "Content-Type: application/json" \
    --data "{\"invite_code\":\"${WORKSPACE_INVITE_CODE}\",\"workspace_name\":\"Compose Verify ${WORKSPACE_SUFFIX}\",\"email\":\"${WORKSPACE_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-workspace-accept.json
  WORKSPACE_ID="$(json_value workspace.id </tmp/peanut-workspace-accept.json)"
  curl -fsS "${BASE_URL}/api/workspaces" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-workspaces.json
  grep -q "${WORKSPACE_ID}" /tmp/peanut-workspaces.json
  curl -fsS "${BASE_URL}/api/workspaces/${WORKSPACE_ID}/resource-usage" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-workspace-usage.json
  grep -q '"limit_profile_id":"self_hosted_default"' /tmp/peanut-workspace-usage.json

  echo "==> Verifying app A/B isolation"
  APP_A_NAME="verify-a-${RUN_ID}"
  APP_B_NAME="verify-b-${RUN_ID}"
  curl -fsS -X POST "${BASE_URL}/api/apps" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"workspace_id\":\"default\",\"name\":\"${APP_A_NAME}\",\"display_name\":\"Verify A ${RUN_ID}\"}" \
    >/tmp/peanut-app-a.json
  curl -fsS -X POST "${BASE_URL}/api/apps" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"workspace_id\":\"default\",\"name\":\"${APP_B_NAME}\",\"display_name\":\"Verify B ${RUN_ID}\"}" \
    >/tmp/peanut-app-b.json
  APP_A_ID="$(json_value app.id </tmp/peanut-app-a.json)"
  APP_B_ID="$(json_value app.id </tmp/peanut-app-b.json)"

  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/keys" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"name":"compose verifier client key","key_type":"client"}' \
    >/tmp/peanut-app-a-client-key.json
  grep -q '"key_type":"client"' /tmp/peanut-app-a-client-key.json
  grep -q '"key":"pk_' /tmp/peanut-app-a-client-key.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/keys" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"name":"compose verifier server key","key_type":"server"}' \
    >/tmp/peanut-app-a-server-key.json
  APP_A_SERVER_KEY="$(json_value key </tmp/peanut-app-a-server-key.json)"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/keys" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"name":"compose verifier admin key","key_type":"admin"}' \
    >/tmp/peanut-app-a-admin-key.json
  grep -q '"key_type":"admin"' /tmp/peanut-app-a-admin-key.json
  grep -q '"key":"adm_' /tmp/peanut-app-a-admin-key.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_B_ID}/keys" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"name":"compose verifier server key","key_type":"server"}' \
    >/tmp/peanut-app-b-server-key.json
  APP_B_SERVER_KEY="$(json_value key </tmp/peanut-app-b-server-key.json)"

  SHARED_EMAIL="compose-shared-${RUN_ID}@example.com"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/auth/register" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${SHARED_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-app-a-register.json
  APP_A_USER_ID="$(json_value user.id </tmp/peanut-app-a-register.json)"
  curl -fsS -X PUT "${BASE_URL}/api/admin/users/${APP_A_USER_ID}/activate" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-app-a-activate.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/auth/login" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${SHARED_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-app-a-login.json
  APP_A_USER_TOKEN="$(json_value access_token </tmp/peanut-app-a-login.json)"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_B_ID}/auth/register" \
    -H "X-Peanut-Api-Key: ${APP_B_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${SHARED_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-app-b-register.json
  APP_B_USER_ID="$(json_value user.id </tmp/peanut-app-b-register.json)"
  curl -fsS -X PUT "${BASE_URL}/api/admin/users/${APP_B_USER_ID}/activate" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-app-b-activate.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_B_ID}/auth/login" \
    -H "X-Peanut-Api-Key: ${APP_B_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${SHARED_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-app-b-login.json
  curl_expect_status 409 /tmp/peanut-app-a-duplicate-register.json \
    -X POST "${BASE_URL}/api/apps/${APP_A_ID}/auth/register" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${SHARED_EMAIL}\",\"password\":\"password123\"}"

  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/data/tables" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"name\":\"${SMOKE_TABLE}_a\",\"display_name\":\"Compose Verify A\",\"schema\":{\"fields\":{\"title\":{\"type\":\"string\",\"required\":true}}},\"access_policy\":{\"mode\":\"authenticated_shared_rw\"}}" \
    >/tmp/peanut-app-a-data-table.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_A_ID}/data/tables/${SMOKE_TABLE}_a/rows" \
    -H "Authorization: Bearer ${APP_A_USER_TOKEN}" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data '{"data":{"title":"app a only"}}' \
    >/tmp/peanut-app-a-data-row.json

  echo "==> Verifying cross-app Data denial"
  curl_expect_status 403 /tmp/peanut-cross-app-data-denied.json \
    "${BASE_URL}/api/apps/${APP_B_ID}/data/tables" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}"

  echo "==> Verifying cross-app Storage denial"
  curl_expect_status 403 /tmp/peanut-cross-app-storage-denied.json \
    -X PUT "${BASE_URL}/api/apps/${APP_B_ID}/storage/buckets/missing/objects/blocked.txt" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    -H "Content-Type: text/plain" \
    --data 'blocked'

  echo "==> Verifying cross-app Function denial"
  curl_expect_status 403 /tmp/peanut-cross-app-function-denied.json \
    -X POST "${BASE_URL}/api/apps/${APP_B_ID}/function-endpoints/missing" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data '{"input":{"blocked":true}}'

  echo "==> Verifying disabled app blocks and re-enables SDK access"
  curl -fsS -X POST "${BASE_URL}/api/admin/apps/${APP_A_ID}/disable" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"reason":"compose production gate"}' \
    >/tmp/peanut-app-a-disable.json
  curl_expect_status 403 /tmp/peanut-disabled-app-denied.json \
    "${BASE_URL}/api/apps/${APP_A_ID}/data/tables" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}"
  curl -fsS -X POST "${BASE_URL}/api/admin/apps/${APP_A_ID}/enable" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-app-a-enable.json
  curl -fsS "${BASE_URL}/api/apps/${APP_A_ID}/data/tables" \
    -H "X-Peanut-Api-Key: ${APP_A_SERVER_KEY}" \
    >/tmp/peanut-reenabled-app-tables.json
  grep -q '"tables"' /tmp/peanut-reenabled-app-tables.json

  echo "==> Creating app-scoped server key for ${APP_ID}"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/keys" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"name":"compose verifier server key","key_type":"server"}' \
    >/tmp/peanut-server-key.json
  SERVER_KEY="$(json_value key </tmp/peanut-server-key.json)"

  echo "==> Verifying app-scoped auth lifecycle"
  VERIFY_EMAIL="compose-verify-$(date +%s)@example.com"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/auth/register" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${VERIFY_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-auth-register.json
  VERIFY_USER_ID="$(json_value user.id </tmp/peanut-auth-register.json)"
  curl -fsS -X PUT "${BASE_URL}/api/admin/users/${VERIFY_USER_ID}/activate" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-auth-activate.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/auth/login" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data "{\"email\":\"${VERIFY_EMAIL}\",\"password\":\"password123\"}" \
    >/tmp/peanut-auth-login.json
  USER_TOKEN="$(json_value access_token </tmp/peanut-auth-login.json)"
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/auth/sessions" \
    -H "Authorization: Bearer ${USER_TOKEN}" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    >/tmp/peanut-auth-sessions.json
  grep -q '"sessions"' /tmp/peanut-auth-sessions.json
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/auth/events" \
    -H "Authorization: Bearer ${USER_TOKEN}" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    >/tmp/peanut-auth-events.json
  grep -q '"events"' /tmp/peanut-auth-events.json
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/auth/users" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-auth-users.json
  grep -q "${VERIFY_EMAIL}" /tmp/peanut-auth-users.json

  echo "==> Verifying app-scoped Data CRUD"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/data/tables" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"name\":\"${SMOKE_TABLE}\",\"display_name\":\"Compose Verify\",\"schema\":{\"fields\":{\"title\":{\"type\":\"string\",\"required\":true}}},\"access_policy\":{\"mode\":\"authenticated_shared_rw\"}}" \
    >/tmp/peanut-data-table.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/data/tables/${SMOKE_TABLE}/rows" \
    -H "Authorization: Bearer ${USER_TOKEN}" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    -H "Content-Type: application/json" \
    --data '{"data":{"title":"ok"}}' \
    >/tmp/peanut-data-row.json
  grep -q '"id"' /tmp/peanut-data-row.json
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/data/tables/${SMOKE_TABLE}/rows" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-data-rows-admin.json
  grep -q '"rows"' /tmp/peanut-data-rows-admin.json

  echo "==> Verifying app-scoped Storage CRUD"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/storage/buckets" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"name\":\"${SMOKE_BUCKET}\",\"public_read\":false,\"allow_client_uploads\":true,\"allowed_mime_types\":[]}" \
    >/tmp/peanut-storage-bucket.json
  curl -fsS -X PUT "${BASE_URL}/api/apps/${APP_ID}/storage/buckets/${SMOKE_BUCKET}/objects/hello.txt" \
    -H "Authorization: Bearer ${USER_TOKEN}" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    -H "Content-Type: text/plain" \
    --data 'hello from compose' \
    >/tmp/peanut-storage-put.txt
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/storage/buckets/${SMOKE_BUCKET}/objects/hello.txt" \
    -H "Authorization: Bearer ${USER_TOKEN}" \
    -H "X-Peanut-Api-Key: ${SERVER_KEY}" \
    >/tmp/peanut-storage-get.txt
  grep -q 'hello from compose' /tmp/peanut-storage-get.txt
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/storage/buckets/${SMOKE_BUCKET}/objects" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-storage-objects-admin.json
  grep -q 'hello.txt' /tmp/peanut-storage-objects-admin.json

  echo "==> Verifying admin backup API"
  curl -fsS -X POST "${BASE_URL}/api/admin/backups" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-backup.json
  grep -q '"backup"' /tmp/peanut-backup.json
  BACKUP_NAME="$(json_value backup.name </tmp/peanut-backup.json)"
  curl -fsS "${BASE_URL}/api/admin/backups/${BACKUP_NAME}/download" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-backup-download.db
  test -s /tmp/peanut-backup-download.db
  curl -fsS -X POST "${BASE_URL}/api/admin/backups/${BACKUP_NAME}/restore" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"confirmation\":\"${BACKUP_NAME}\",\"reason\":\"compose restore-pending safety check\"}" \
    >/tmp/peanut-restore-scheduled.json
  grep -q '"restart_required":true' /tmp/peanut-restore-scheduled.json
  curl -fsS "${BASE_URL}/api/admin/backups/restore-pending" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-restore-pending.json
  grep -q "${BACKUP_NAME}" /tmp/peanut-restore-pending.json
  curl -fsS -X DELETE "${BASE_URL}/api/admin/backups/restore-pending" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-restore-cleared.json
  echo "==> Verifying restore-pending clear keeps readiness clean"
  curl -fsS "${BASE_URL}/api/ready" >/tmp/peanut-ready-after-restore-clear.json
  grep -q '"ready":true' /tmp/peanut-ready-after-restore-clear.json

  echo "==> Verifying Function editor lint API"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/functions/editor/lint" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"runtime":"typescript","source_code":"export default function handler(){ return { ok: true } }"}' \
    >/tmp/peanut-function-lint.json
  grep -q '"status":"passed"' /tmp/peanut-function-lint.json
  echo "==> Verifying app-scoped Function create and invoke"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/functions" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"name\":\"${SMOKE_FUNCTION}\",\"display_name\":\"Compose Verify\",\"endpoint_slug\":\"${SMOKE_FUNCTION_SLUG}\",\"runtime\":\"javascript\",\"source_code\":\"export default function handler(ctx) { return { ok: true, input: ctx.request.input } }\",\"invoke_policy\":\"authenticated\",\"timeout_ms\":3000,\"enabled\":true}" \
    >/tmp/peanut-function-create.json
  grep -q "\"app_id\":\"${APP_ID}\"" /tmp/peanut-function-create.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/function-endpoints/${SMOKE_FUNCTION_SLUG}" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"input":{"from":"compose"}}' \
    >/tmp/peanut-function-invoke.json
  grep -q '"status":"succeeded"' /tmp/peanut-function-invoke.json
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/functions/${SMOKE_FUNCTION}/invocations" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-function-invocations.json
  grep -q "\"app_id\":\"${APP_ID}\"" /tmp/peanut-function-invocations.json

  echo "==> Verifying app-scoped platform diagnostics"
  curl -fsS "${BASE_URL}/api/admin/ops/diagnostics" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-platform-diagnostics.json
  grep -q '"ok":true' /tmp/peanut-platform-diagnostics.json

  echo "==> Verifying app-scoped push diagnostics"
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/push/diagnostics" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-push-diagnostics.json
  grep -q '"checks"' /tmp/peanut-push-diagnostics.json
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/push/test-message" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data "{\"title\":\"compose test\",\"body\":\"hello from compose\",\"user_id\":\"${VERIFY_USER_ID}\"}" \
    >/tmp/peanut-push-test-message.json
  grep -q 'queued push message' /tmp/peanut-push-test-message.json
else
  echo "==> Skipping authenticated checks; set PEANUT_ADMIN_TOKEN or PEANUT_BOOTSTRAP_EMAIL/PEANUT_BOOTSTRAP_PASSWORD"
fi

echo "==> All compose production gate checks passed"
