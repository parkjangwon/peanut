#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}"
BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
SERVICE="${SERVICE:-peanut}"
ADMIN_TOKEN="${PEANUT_ADMIN_TOKEN:-}"
APP_ID="${PEANUT_VERIFY_APP_ID:-default}"

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

if [[ -n "${ADMIN_TOKEN}" ]]; then
  echo "==> Verifying admin backup API"
  curl -fsS -X POST "${BASE_URL}/api/admin/backups" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-backup.json
  grep -q '"backup"' /tmp/peanut-backup.json

  echo "==> Verifying Function editor lint API"
  curl -fsS -X POST "${BASE_URL}/api/apps/${APP_ID}/functions/editor/lint" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    --data '{"runtime":"typescript","source_code":"export default function handler(){ return { ok: true } }"}' \
    >/tmp/peanut-function-lint.json
  grep -q '"status":"passed"' /tmp/peanut-function-lint.json

  echo "==> Verifying app-scoped platform diagnostics"
  curl -fsS "${BASE_URL}/api/admin/ops/diagnostics" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-platform-diagnostics.json
  grep -q '"ok":true' /tmp/peanut-platform-diagnostics.json

  echo "==> Verifying app-scoped push diagnostics"
  curl -fsS "${BASE_URL}/api/apps/${APP_ID}/push/diagnostics" \
    -H "Authorization: Bearer ${ADMIN_TOKEN}" >/tmp/peanut-push-diagnostics.json
  grep -q '"checks"' /tmp/peanut-push-diagnostics.json
else
  echo "==> Skipping authenticated checks; set PEANUT_ADMIN_TOKEN to verify backup and app-scoped diagnostics"
fi

echo "==> Compose verification complete"
