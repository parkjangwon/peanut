# Peanut Module Boundaries

This note captures the current backend module ownership. Keep changes app-scoped
by default; new public behavior should normally live under `/api/apps/:app_id`.

## API Modules

- `src/api/auth/`: app-scoped password auth, refresh sessions, auth events, OIDC,
  and first-admin bootstrap.
- `src/api/data/`: app-scoped table definitions, row CRUD, query presets,
  import/export, and row events.
- `src/api/storage/`: app-scoped bucket and object APIs.
- `src/api/functions/`: app-scoped function CRUD, versions, invocations, editor
  helpers, and endpoint invocation.
- `src/api/push/`: app-scoped subscriptions, queue state, diagnostics, and send
  flows.
- `src/api/app_scope.rs`: admin wrappers that bind existing domain handlers to a
  path `app_id`.
- `src/middleware/sdk_auth.rs`: app-key auth, scope checks, and bearer/app
  mismatch checks for SDK routes.

## Route Policy

Legacy global runtime paths are intentionally not mounted. If a new endpoint is
for application developers, add it under `/api/apps/:app_id/...`. If it is for
platform operation, keep it behind admin bearer auth.

## Runtime Trust Boundary

Peanut Functions are trusted admin-managed extensions. The runtime uses a local
Deno subprocess and bounded Peanut host bindings. This is process-level
hardening, not a hostile-tenant sandbox. Installations that do not need
Functions should set:

```bash
FUNCTIONS_ENABLED=false
```

The readiness endpoint reports whether Functions are enabled and whether the
local Deno runtime and work directory are available. `FUNCTIONS_ALLOW_NETWORK=false`
keeps common network APIs unavailable, and `FUNCTIONS_MAX_CONCURRENT` caps
simultaneous Function invocations in the Peanut process.
