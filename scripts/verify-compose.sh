#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}"
BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
SERVICE="${SERVICE:-peanut}"
ADMIN_TOKEN="${PEANUT_ADMIN_TOKEN:-}"
APP_ID="${PEANUT_VERIFY_APP_ID:-default}"
BOOTSTRAP_EMAIL="${PEANUT_BOOTSTRAP_EMAIL:-}"
BOOTSTRAP_PASSWORD="${PEANUT_BOOTSTRAP_PASSWORD:-}"
SMOKE_TABLE="verify_$(date +%s)"
SMOKE_BUCKET="verify-$(date +%s)"
SMOKE_FUNCTION="verify_$(date +%s)"
SMOKE_FUNCTION_SLUG="verify-$(date +%s)"

json_value() {
  python3 -c 'import json,sys; data=json.load(sys.stdin); cur=data
for part in sys.argv[1].split("."):
    cur=cur[part]
print(cur)' "$1"
}

echo "==> Building and starting Peanut with ${COMPOSE_FILE}"
docker compose -f "${COMPOSE_FILE}" up -d --build "${SERVICE}"

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
  docker compose -f "${COMPOSE_FILE}" logs --tail=200 "${SERVICE}"
  exit 1
fi

echo "==> Verifying Docker image has Deno"
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" deno --version >/tmp/peanut-deno.txt
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
    >/tmp/peanut-restore-scheduled.json
  grep -q '"restart_required":true' /tmp/peanut-restore-scheduled.json
  curl -fsS "${BASE_URL}/api/admin/backups/restore-pending" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-restore-pending.json
  grep -q "${BACKUP_NAME}" /tmp/peanut-restore-pending.json
  curl -fsS -X DELETE "${BASE_URL}/api/admin/backups/restore-pending" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    >/tmp/peanut-restore-cleared.json

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
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/functions/endpoints/${SMOKE_FUNCTION_SLUG}" \
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

echo "==> Compose verification complete"
