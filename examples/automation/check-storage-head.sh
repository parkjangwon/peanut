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
: "${STORAGE_BUCKET:=assets}"
: "${STORAGE_KEY:=ops/hello.txt}"

curl -fsSI "$BASE_URL/api/s3/$STORAGE_BUCKET/$STORAGE_KEY" \
  -H "authorization: Bearer $SERVICE_TOKEN"

echo "storage HEAD check passed for s3://$STORAGE_BUCKET/$STORAGE_KEY"
