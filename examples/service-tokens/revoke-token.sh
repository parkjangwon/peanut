#!/usr/bin/env bash
set -euo pipefail

: "${BASE_URL:=http://127.0.0.1:3000}"
: "${ADMIN_JWT:?Set ADMIN_JWT to an admin bearer token}"
: "${TOKEN_ID:?Set TOKEN_ID to the service token id you want to revoke}"

curl -s -X DELETE "$BASE_URL/api/admin/service-tokens/$TOKEN_ID"   -H "authorization: Bearer $ADMIN_JWT"
