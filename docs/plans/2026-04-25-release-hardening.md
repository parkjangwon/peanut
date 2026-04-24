# Peanut Release Hardening Plan

> For Hermes: use subagent-driven-development skill to implement this plan task-by-task when delegation is useful.

Goal: move Peanut from prototype/MVP-in-progress to a releasable self-host single-binary backend with a real admin console, safer auth, isolated storage, and operable push workflows.

Architecture: keep the project small and self-host oriented. Do not turn Peanut into a general-purpose platform. Harden the existing Rust + SQLite + embedded static console architecture so it is consistent, documented, and safe enough to ship. Prefer honest ntfy-based push MVP over pretending Web Push is finished.

Tech Stack: Rust, Axum, SQLx SQLite, Argon2, JWT, Next.js static export, Tailwind, Docker.

---

## Task 1: Replace prototype auth responses with stable JSON contracts
Objective: make auth/session/admin/storage clients depend on typed JSON rather than raw strings and regex parsing.

Files:
- Modify: `src/api/auth.rs`
- Modify: `src/api/admin.rs`
- Modify: `src/api/storage.rs`
- Modify: `src/main.rs`
- Modify: `peanut-console/src/app/console-client.tsx`

Steps:
1. Add typed response structs for register/login/me.
2. Return JSON bodies for success and error responses where appropriate.
3. Remove `/api/me` string formatting and return a typed session payload.
4. Update console client to consume JSON only.
5. Verify build and route behavior.

## Task 2: Add auth input validation and safer startup config
Objective: reject invalid credentials early and remove insecure production defaults.

Files:
- Modify: `src/api/auth.rs`
- Modify: `src/main.rs`
- Test: backend route/unit tests as added in Task 6
- Docs: `.env.example`, `README.md`, `README.ko.md`

Steps:
1. Validate email presence/shape and password minimum length.
2. Return structured 400 errors for invalid payloads.
3. Require `JWT_SECRET` at startup instead of silently using `temp_secret`.
4. Document required environment variables.

## Task 3: Isolate storage by authenticated user
Objective: prevent cross-user object listing/read/write/delete.

Files:
- Modify: `src/api/storage.rs`
- Modify: `src/storage/local.rs`
- Test: backend route/unit tests as added in Task 6
- Modify: `peanut-console/src/app/console-client.tsx`

Steps:
1. Scope object paths under authenticated user ID.
2. Strip user prefix from API-visible object keys.
3. Keep admins subject to explicit APIs rather than accidental global access.
4. Add regression tests proving one user cannot see another user’s objects.

## Task 4: Finish an honest push MVP
Objective: ship ntfy-based push end-to-end with subscriptions, queueing, visibility, and safer worker behavior.

Files:
- Modify: `src/api/mod.rs`
- Create/Modify: `src/api/push.rs`
- Modify: `src/main.rs`
- Modify: `src/push/worker.rs`
- Modify: `src/push/ntfy.rs` if needed
- Modify: `src/push/webpush.rs` if needed for explicit non-MVP stance
- Modify: migrations under `migrations/`
- Modify: `peanut-console/src/app/console-client.tsx`

Steps:
1. Add subscription CRUD API for ntfy topics.
2. Add push queue creation and queue inspection API.
3. Improve worker states: claim processing, failed/no-subscription handling, retries, error capture.
4. Add console UI for subscription management and sending a test notification.
5. Update docs to describe push honestly as ntfy MVP.

## Task 5: Harden console session UX and fix lint issues
Objective: remove localStorage token persistence and make the console build/lint cleanly.

Files:
- Modify: `peanut-console/src/app/console-client.tsx`
- Modify: `peanut-console/src/app/page.tsx` if needed
- Modify: `peanut-console/src/app/layout.tsx` if needed

Steps:
1. Keep access token in memory/session-only rather than localStorage.
2. Preserve only non-sensitive convenience state locally.
3. Refactor effects so `npm run lint` passes cleanly.
4. Improve user-facing status/error states.

## Task 6: Add integration-grade backend tests for release blockers
Objective: prove the most important contracts with automated tests.

Files:
- Modify: `src/main.rs` or supporting test modules to expose a testable app builder.
- Add/Modify: test modules under `src/` or `tests/`

Steps:
1. Add route tests for register/login/session JSON flow.
2. Add validation tests for invalid email/password.
3. Add authorization tests for admin-only endpoints.
4. Add storage isolation tests across two users.
5. Add push subscription/enqueue/queue state tests for the new MVP.

## Task 7: Refresh release docs and operator setup
Objective: make the repo truthful and runnable by someone new.

Files:
- Modify: `README.md`
- Modify: `README.ko.md`
- Create: `.env.example`
- Modify: `docker-compose.yml` if needed

Steps:
1. Replace outdated “repo does not build” statements.
2. Document current feature set and explicit non-goals.
3. Document required env vars, startup flow, admin bootstrap, storage location, and push setup.
4. Document release/run commands and verification steps.

## Task 8: Final verification and ship to master
Objective: verify the release candidate and land it on master directly.

Files:
- Working tree only

Steps:
1. Run backend tests.
2. Run console lint/build.
3. Run full project build script.
4. Manually verify health/auth/admin/storage/push flows against a running binary.
5. Fast-forward or merge work onto `master`, then commit and push directly to `origin/master`.
