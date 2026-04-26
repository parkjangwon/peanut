# Peanut v2 Implementation Plan

> For Hermes: execute this plan in narrow, releasable slices. Use TDD for every behavior change, prefer API-first hardening before UI work, and keep Peanut within its project-local self-host backend scope.

Goal: evolve Peanut from a narrow but usable self-host backend core into a more operationally robust backend platform that external apps can rely on for auth, data, storage, push, and bounded backend functions.

Architecture:
- Keep Peanut self-host-first: single Rust service, SQLite, local storage, bounded APIs, bounded functions.
- Shift v2 toward API-first productization: harden backend contracts and runtime behavior before rebuilding a new operations console.
- Expand only where product value is clear: auth/session operations, data evolution, function safety, observability, and operator workflows.

Tech stack:
- Rust + Axum + SQLx + SQLite
- Local filesystem storage
- Node subprocess sandbox for functions
- Optional ntfy / Web Push integrations

---

## Product position for v2

Peanut v2 should become:
- a practical self-host backend core for solo developers and small teams
- an external-app-capable auth backend
- a bounded SQLite-backed data API with policy-aware CRUD
- a bounded function runtime for backend extensions
- an operationally understandable system with clear logs, health, readiness, and docs

Peanut v2 should not become:
- a raw SQL platform
- a full Supabase/Firebase clone
- an orchestration/plugin framework
- a multi-tenant cloud platform
- a general-purpose serverless runtime

## Current codebase facts

- Auth supports register/login/me plus refresh/logout/password lifecycle/session revoke flows.
- Data API supports admin-managed logical tables and bounded row CRUD.
- Storage supports user-scoped local object storage.
- Push supports ntfy + Web Push queueing and delivery.
- Functions support JS/TS execution with bounded Peanut host bindings.
- The old Next.js web console source has been removed upstream, so v2 should treat the backend as API-first and reintroduce a new console later.

## v2 principles

1. Keep the product boundary narrow and explicit.
2. Improve operational reliability before adding broad new surface area.
3. Prefer fixed policy modes over arbitrary user scripting.
4. Prefer external-app developer experience over cosmetic admin UX.
5. Keep every phase releasable on its own.

## Phase map

### Phase 1 — Foundation hardening
Target: make Peanut safe to boot, inspect, deploy, and troubleshoot.

Scope:
- startup/config validation improvements
- health vs readiness separation
- consistent JSON error envelope direction
- request correlation / structured logs foundation
- disk/storage/runtime checks
- API-first fallback behavior after web console removal
- build/release scripts updated for backend-first packaging

### Phase 2 — Auth v2
Target: make Peanut Auth a more complete external-app auth backend.

Scope:
- auth client/origin policy
- password reset delivery abstraction
- auth event log
- richer session metadata
- tighter app-facing docs/examples

### Phase 3 — Data API v2
Target: make Data API more realistic for app development without breaking the bounded model.

Scope:
- schema evolution rules
- richer policy visibility
- audit/event query endpoints
- import/export and operational tooling
- bounded query improvements

### Phase 4 — Functions v2
Target: make Functions production-usable as a bounded extension layer.

Scope:
- invocation lifecycle improvements
- retry/async model
- secrets handling hardening
- versioning/rollback
- sandbox limits and clearer operator visibility

### Phase 5 — Push/Storage v2
Target: improve operator trust in delivery and asset handling.

Scope:
- push queue state machine cleanup
- delivery retry/backoff policy
- subscription metadata and cleanup
- storage metadata/checksum/quota support
- safer download/access patterns

### Phase 6 — New operations console
Target: reintroduce a backend-driven operator console after API contracts are stable.

Scope:
- auth/users overview
- data/functions/push/storage operations
- runtime/config visibility
- audit/event views
- API-first architecture so the UI can evolve separately from the backend

## Phase 1 immediate execution order

### Slice 1: API-first fallback + build pipeline alignment
- remove hard dependency on deleted Next.js source during local/release build
- replace embedded-console assumption with a small backend landing/fallback page
- ensure fresh clone build works without `peanut-console/` source

### Slice 2: readiness endpoint
- add `GET /api/ready`
- check DB query success
- check storage directory exists and is writable
- return structured readiness reasons

### Slice 3: runtime/config validation
- centralize config loading
- validate DB URL / bind addr / upload limits / storage path upfront
- expose startup failure reasons cleanly

### Slice 4: logging and request tracing foundation
- request id propagation
- structured request logging
- tie API errors to request id where possible

## Acceptance criteria for v2 direction

- Fresh clone backend build does not require deleted web console source.
- Root/fallback behavior is explicit and API-first rather than depending on stale console artifacts.
- Operators can distinguish health from readiness.
- External app developers have stable, documented auth and core API contracts.
- Every subsystem has enough audit/logging to debug real failures.
- Peanut remains clearly bounded and does not drift into raw-SQL or arbitrary-runtime territory.

## Verification expectations for each slice

- `cargo test`
- `./scripts/build.sh`
- update `README.md` and `README.ko.md` whenever product/API/runtime behavior changes materially
- keep changes small enough to commit and ship independently

## First slice implementation note

Start with Phase 1 Slice 1 because the current upstream state removed the old `peanut-console` source but the repo still carries backend/runtime assumptions that it exists. Fixing that first restores clean builds and establishes the API-first posture needed for the rest of v2.
