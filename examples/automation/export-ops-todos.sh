#!/usr/bin/env bash
set -euo pipefail

: "${PEANUT_ENV_FILE:=examples/automation/peanut.env}"

if [[ ! -f "$PEANUT_ENV_FILE" ]]; then
  echo "env file not found: $PEANUT_ENV_FILE" >&2
  echo "copy examples/automation/peanut.env.sample to that path and fill in real values" >&2
  exit 1
fi

# shellcheck disable=SC1090
source "$PEANUT_ENV_FILE"

: "${BASE_URL:?Set BASE_URL in the env file}"
: "${SERVICE_TOKEN:?Set SERVICE_TOKEN in the env file}"
: "${TABLE_NAME:=ops_todos}"
: "${AUTOMATION_OUT_DIR:=/opt/peanut/backups}"

TIMESTAMP=$(date +%F-%H%M%S)
mkdir -p "$AUTOMATION_OUT_DIR"

curl -fsS "$BASE_URL/api/data/tables/$TABLE_NAME/export" \
  -H "authorization: Bearer $SERVICE_TOKEN" \
  > "$AUTOMATION_OUT_DIR/$TABLE_NAME-$TIMESTAMP.json"

echo "wrote $AUTOMATION_OUT_DIR/$TABLE_NAME-$TIMESTAMP.json"
