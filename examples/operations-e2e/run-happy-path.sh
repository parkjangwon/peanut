#!/usr/bin/env bash
set -euo pipefail

command -v jq >/dev/null 2>&1 || {
  echo 'jq is required for run-happy-path.sh' >&2
  exit 1
}

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${APP_ID:=default}"
: "${TABLE_NAME:=ops_todos}"
: "${STORAGE_BUCKET:=assets}"
: "${STORAGE_KEY:=ops/hello.txt}"

require_ok_json() {
  local label="$1"
  local response="$2"

  if echo "$response" | jq -e 'has("error")' >/dev/null; then
    echo "[$label] failed:" >&2
    echo "$response" | jq . >&2
    exit 1
  fi
}

echo "[1/9] readiness"
ready_json=$(curl -s "$BASE_URL/api/ready")
echo "$ready_json" | jq .
if [ "$(echo "$ready_json" | jq -r '.status')" != "ready" ]; then
  echo "Peanut is not ready" >&2
  exit 1
fi

if [ -z "${SERVICE_TOKEN:-}" ]; then
  : "${ADMIN_JWT:?Set ADMIN_JWT or SERVICE_TOKEN before running this script}"
  echo "[2/9] create service token"
  token_json=$(bash examples/operations-e2e/create-service-token.sh)
  require_ok_json "create service token" "$token_json"
  export SERVICE_TOKEN
  SERVICE_TOKEN=$(echo "$token_json" | jq -r '.token')
  echo "$token_json" | jq '{service_token}'
else
  echo "[2/9] reuse provided service token"
fi

echo "[3/9] create data table"
table_json=$(bash examples/operations-e2e/create-todos-table.sh)
if echo "$table_json" | jq -e '.code == "conflict"' >/dev/null; then
  echo "table already exists; continuing"
else
  require_ok_json "create data table" "$table_json"
  echo "$table_json" | jq .
fi

echo "[4/9] create row"
row_json=$(bash examples/operations-e2e/create-todo-row.sh)
require_ok_json "create row" "$row_json"
echo "$row_json" | jq .

echo "[5/9] list rows"
rows_json=$(bash examples/operations-e2e/list-todo-rows.sh)
require_ok_json "list rows" "$rows_json"
echo "$rows_json" | jq .

echo "[6/9] export table"
export_json=$(bash examples/operations-e2e/export-todos-table.sh)
require_ok_json "export table" "$export_json"
echo "$export_json" | jq '.metadata'

echo "[7/9] checkpoint and replay"
checkpoint_json=$(bash examples/operations-e2e/get-events-checkpoint.sh)
require_ok_json "checkpoint" "$checkpoint_json"
export LAST_EVENT_ID
LAST_EVENT_ID=$(echo "$checkpoint_json" | jq -r '.latest_event_id // 0')
replay_json=$(bash examples/operations-e2e/replay-todo-events.sh)
require_ok_json "replay events" "$replay_json"
echo "$replay_json" | jq .

echo "[8/9] import replace fixture"
import_json=$(bash examples/operations-e2e/import-todos-table.sh)
require_ok_json "import table" "$import_json"
echo "$import_json" | jq .

echo "[9/9] storage upload and head"
upload_json=$(bash examples/operations-e2e/upload-storage-object.sh)
if [ -n "$upload_json" ]; then
  require_ok_json "upload storage object" "$upload_json"
  echo "$upload_json" | jq .
fi
bash examples/operations-e2e/head-storage-object.sh

echo
echo "Peanut operations happy path completed."
