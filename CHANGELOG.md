# Changelog

All notable changes to Peanut should be documented in this file.

This project does not have a formal release history yet, so the notes below act as a draft for the first public release cut.

## Unreleased

### Added
- SQLite-backed Data API for Peanut-managed logical tables
- table CRUD endpoints and row CRUD endpoints under `/api/data/...`
- fixed access policy modes including `owner_private`, `admin_only`, and `authenticated_shared_rw`
- generic row filtering with `filter_field`, `filter_op`, `filter_value`, plus ordering and limit support
- hybrid push delivery with both ntfy topic subscriptions and Web Push subscriptions
- `GET /api/push/vapid-public-key` for browser-side VAPID bootstrap
- browser and manual Web Push registration flows in the embedded console
- table schema editing and deletion from the console
- row update/delete actions in the console
- release-oriented documentation for Data API, Web Push smoke tests, curl quickstart, and Docker Compose operations

### Changed
- push queue worker now treats delivery as successful when at least one subscription succeeds, instead of failing the whole item on the first broken destination
- queue items with no subscriptions configured now fail terminally instead of burning retry cycles
- `GET /api/push/queue` now returns queue summary counts for operator visibility, including `partial_success`
- partial-delivery queue items now keep failure details in `last_error` even when at least one destination succeeded
- dead Web Push subscriptions are automatically pruned when providers return terminal 404/410-style errors
- partial-delivery queue state is now structured through explicit `partial_failure_count` and `failed_destinations_json` fields instead of inferring from `last_error` text alone
- push queue summary now also includes current `ntfy_subscriptions` and `web_push_subscriptions` counts for delivery-kind visibility
- `JWT_SECRET` is now treated as required runtime configuration
- storage is enforced as user-scoped isolation
- README and README.ko now document current product scope, operational flow, and API usage more explicitly

### Fixed
- console no longer tries to fetch a non-existent default data table on fresh login when the database has no tables
- empty-table state now shows a friendly message instead of surfacing a `data table not found` error in the session banner

## Release draft summary

Peanut is now a small single-binary self-host backend with:
- JWT auth and admin approval flow
- user-scoped file storage
- constrained SQLite-backed Data API
- ntfy + Web Push hybrid delivery
- embedded web console for core operations

## Suggested first release title

`v0.1.0 - single-binary self-host backend with Data API and hybrid push`
