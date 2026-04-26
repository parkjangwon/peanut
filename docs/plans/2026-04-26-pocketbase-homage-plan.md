# Peanut PocketBase Homage Plan

> For Hermes: execute this plan in narrow, releasable slices. Copy PocketBase's strongest product ideas where they improve Peanut's operator and developer experience, but keep Peanut within its bounded self-host backend scope.

Goal: borrow the best parts of PocketBase without turning Peanut into a PocketBase clone, so Peanut becomes a stronger API-first self-host backend for external apps.

Architecture:
- Keep Peanut narrow: Rust single-binary server, SQLite, local storage, bounded APIs, bounded JS/TS functions.
- Homage UX and product surface where PocketBase is clearly stronger: realtime, data ergonomics, auth breadth, operator tooling, SDK/DX.
- Do not adopt PocketBase's fully generic collection platform posture if it weakens Peanut's bounded contract-layer identity.

Tech stack:
- Rust + Axum + SQLx + SQLite
- Existing Peanut auth/data/storage/push/functions modules
- Existing Node subprocess sandbox for functions
- Future SSE or WebSocket realtime transport

---

## Why this plan exists

PocketBase already proves there is demand for a small self-host backend in a single executable. That is not a reason to stop. It is a reason to sharpen Peanut's product line.

Peanut should treat PocketBase as:
- a validation signal for the category
- a benchmark for operator/developer experience
- a source of good product patterns to adapt

Peanut should not treat PocketBase as:
- a spec to duplicate feature-for-feature
- a reason to broaden into an unbounded BaaS
- a reason to de-prioritize push and bounded functions

## Keep / do not lose

These are Peanut-native strengths and should become stronger, not weaker:

1. Push as a first-class product surface
   - ntfy subscriptions
   - Web Push with VAPID
   - queue visibility and delivery state

2. Bounded Functions as a first-class product surface
   - API-managed functions
   - sandboxed JS/TS runtime
   - bounded Peanut host bindings
   - invocation lifecycle and operator visibility

3. External-app auth backend posture
   - auth origin/client policy
   - explicit session control
   - auth event visibility

4. Narrow contract-layer philosophy
   - no raw SQL endpoint
   - no arbitrary package installation
   - no generic unrestricted serverless runtime

## PocketBase capabilities Peanut should homage

### Track A — Realtime event surface
PocketBase advantage:
- realtime subscriptions are core product behavior, not an afterthought

Peanut homage target:
- add a clear realtime API for bounded event streams
- start with operator/app-relevant events only

First candidate event families:
- data row created/updated/deleted
- function invocation status changed
- push queue item status changed
- auth event created

Guardrails:
- no arbitrary table event firehose without policy checks
- no broad pub/sub platform positioning
- events must respect Peanut auth/policy boundaries

### Track B — Better bounded data ergonomics
PocketBase advantage:
- collections/records model is easier to understand and operate
- CRUD, filtering, and management feel cohesive

Peanut homage target:
- keep bounded Data API, but improve the product feel around it

First candidate upgrades:
- better schema/field metadata in table definitions
- bounded sort/filter/pagination improvements
- richer table inspection endpoints
- data import/export for bounded tables
- clearer row event/audit queries

Guardrails:
- no raw SQL
- no arbitrary joins/query language
- no drift into full DB-console-as-a-service

### Track C — Auth breadth and account lifecycle
PocketBase advantage:
- wider built-in auth lifecycle coverage

Peanut homage target:
- expand auth breadth where it directly helps external apps

First candidate upgrades:
- email verification request/confirm flow
- email change request/confirm flow
- OTP-based login or step-up verification
- optional OAuth provider login later

Guardrails:
- keep session tracking and operator policy posture strong
- prefer a few robust flows over many shallow auth modes

### Track D — Operator surface and runtime tooling
PocketBase advantage:
- settings, backups, logs, and admin operations feel productized

Peanut homage target:
- make Peanut easier to operate even before the new console lands

First candidate upgrades:
- backups API and restore flow
- logs/event visibility API
- runtime settings inspection surface
- stronger health/readiness/runtime diagnostics
- function/push/data operational summaries

Guardrails:
- do not block on a rich web console first
- API-first tooling comes before UI polish

### Track E — SDK / developer experience
PocketBase advantage:
- easier client adoption with official SDKs and clean docs

Peanut homage target:
- create a minimal but strong external app developer path

First candidate upgrades:
- lightweight JS SDK or typed fetch client
- auth client examples beyond current minimal sample
- realtime client example once event API exists
- function invocation examples including async polling

Guardrails:
- keep SDK thin and honest
- avoid maintaining broad multi-language SDK surface too early

## PocketBase capabilities Peanut should NOT copy directly

1. Full generic collection platform ambition
   - Peanut should remain more bounded and operator-opinionated.

2. Unlimited extensibility posture
   - Peanut Functions should stay sandboxed and product-scoped.

3. "Backend for everything" messaging
   - Peanut should message itself as a bounded backend core for external apps.

4. Admin UI-first product emphasis
   - Peanut should remain API-first until backend contracts are clearly stable.

## Recommended execution order

### Phase PB-1 — Realtime foundation
Target: give Peanut a live event surface that makes the backend feel operational and app-ready.

Slice PB-1.1
- design event model and channel naming
- document policy rules for subscriptions
- choose SSE first unless WebSocket is clearly necessary

Slice PB-1.2
- add realtime endpoint for authenticated subscriptions
- support function invocation lifecycle events first

Slice PB-1.3
- add push queue lifecycle events
- add auth event stream

Slice PB-1.4
- add bounded data table row events with policy-aware filtering

### Phase PB-2 — Functions hardening first
Target: make bounded Functions so strong that Peanut is clearly not just a smaller PocketBase.

Slice PB-2.1
- function versioning and active-version switching
- rollback endpoint and audit trail

Slice PB-2.2
- retry metadata and attempt tracking
- parent/child invocation linkage

Slice PB-2.3
- secrets hardening
- secret redaction in API responses/logs
- secret reference model for runtime injection

Slice PB-2.4
- emit realtime invocation lifecycle events
- improve function operator summaries

### Phase PB-3 — Data API productization
Target: make Peanut's bounded data layer feel deliberate rather than minimal.

Slice PB-3.1
- table metadata inspection improvements
- better schema docs in API responses

Slice PB-3.2
- bounded query improvements: sort, page, safer filter expansion

Slice PB-3.3
- import/export for bounded tables
- row audit/event query endpoints

### Phase PB-4 — Auth breadth
Target: close the most visible auth gaps versus PocketBase while keeping Peanut's operator-focused posture.

Slice PB-4.1
- email verification flow

Slice PB-4.2
- email change flow

Slice PB-4.3
- OTP-based auth or step-up verification

Slice PB-4.4
- evaluate OAuth provider support only after the above flows are stable

### Phase PB-5 — Operator APIs
Target: make Peanut feel safer to run in the real world.

Slice PB-5.1
- backups create/list/download/restore

Slice PB-5.2
- logs/events API for runtime visibility

Slice PB-5.3
- runtime settings inspection and diagnostics summary

### Phase PB-6 — SDK / examples / docs
Target: improve first-run developer adoption.

Slice PB-6.1
- minimal JS client for auth + data + functions

Slice PB-6.2
- realtime example app

Slice PB-6.3
- docs refresh in README.md and README.ko.md after each stable surface lands

## Immediate implementation priority

Start here:

1. Functions versioning + rollback
2. Functions retry metadata + attempt tracking
3. Functions secrets hardening
4. Realtime foundation for function invocation events
5. Realtime expansion to push/data/auth events

Why this order:
- it doubles down on Peanut's unique value instead of chasing PocketBase breadth first
- it turns the existing Functions investment into a stronger product moat
- it sets up a realtime event layer with a concrete first use case

## Acceptance criteria

This homage strategy is working only if the result is:
- clearly inspired by PocketBase where it helps UX
- still recognizably Peanut in product boundary and philosophy
- stronger in push + bounded functions than before
- more operationally legible for external-app developers and self-host operators

This strategy is failing if Peanut starts to look like:
- a raw SQL service
- an unrestricted plugin host
- a generic clone with weaker product focus

## Verification expectations for each slice

- add or update focused tests first
- run `cargo test`
- run `./scripts/build.sh`
- update `README.md` and `README.ko.md` for every material API/runtime change
- keep slices small enough to commit and ship independently
