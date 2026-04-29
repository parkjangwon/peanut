# Peanut Module Boundaries

This note fixes the intended boundaries for the largest backend modules before
more MVP hardening work lands. It is not a rewrite plan; it is a guardrail for
small, safe changes.

## Current Rule

Do not add new unrelated responsibilities to these large API files:

- `src/api/storage.rs`
- `src/api/data.rs`
- `src/api/functions.rs`

When touching one of them, keep new helpers close to the behavior they support
and prefer extracting a focused module when the new code is reusable or protocol
specific.

## Storage

Current responsibilities:

- route handlers for legacy storage and S3-like storage
- S3 XML response and error envelopes
- range and conditional request handling
- object tagging and checksum parsing
- multipart upload request parsing and responses

Preferred extraction targets:

- `src/api/storage_xml.rs` for XML response builders and XML parsing helpers
- `src/api/storage_conditions.rs` for range and conditional read evaluation
- `src/api/storage_multipart.rs` for multipart query parsing and response builders
- `src/api/storage_tags.rs` for tagging header/XML normalization

## Data API

Current responsibilities:

- table and row handlers
- schema validation and schema evolution checks
- row filter/search/sort evaluation
- import/export checksum handling
- row event replay and SSE

Preferred extraction targets:

- `src/api/data_schema.rs` for schema validation and evolution rules
- `src/api/data_query.rs` for bounded filter/search/sort logic
- `src/api/data_import_export.rs` for snapshot checksum and import/export helpers
- `src/api/data_events.rs` for event payload and SSE helpers

## Functions

Current responsibilities:

- function CRUD and version lifecycle
- invocation lifecycle and retry
- policy checks, API key checks, and per-function rate limits
- event emission
- runtime invocation handoff to `src/functions/mod.rs`

Preferred extraction targets:

- `src/api/functions_policy.rs` for invoke policy, origin, API key, and rate-limit checks
- `src/api/functions_versions.rs` for version persistence and rollback helpers
- `src/api/functions_invocations.rs` for invocation creation, status updates, retry, and events

## Runtime Trust Boundary

Peanut Functions are trusted admin-managed extensions. The runtime uses a local
Node subprocess and bounded Peanut host bindings, but Peanut does not claim
OS-level sandboxing. Installations that do not need Functions should set:

```bash
FUNCTIONS_ENABLED=false
```

The readiness endpoint reports whether Functions are enabled and whether the
local Node runtime is available.
