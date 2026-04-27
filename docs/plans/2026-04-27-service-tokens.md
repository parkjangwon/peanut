# Peanut service tokens plan

## Goal
Add a narrow server-to-server service token feature for Peanut without turning it into a broad OAuth/client-credentials platform.

## Scope for this slice
- admin-managed opaque service tokens
- admin-only access mode for now
- one-time plaintext token reveal on create
- hashed token storage in SQLite
- bearer auth middleware accepts either JWT or service token
- list/create/revoke admin endpoints
- English/Korean docs

## Non-goals
- OAuth client credentials
- dynamic scopes/ACL matrices
- per-token row-level impersonation
- UI/console management

## API shape
- `GET /api/admin/service-tokens`
- `POST /api/admin/service-tokens`
- `DELETE /api/admin/service-tokens/:token_id`

## Verification
- targeted failing tests first
- `cargo test`
- `./scripts/build.sh`
