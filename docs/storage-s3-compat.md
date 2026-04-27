# Peanut storage S3 compatibility matrix

Peanut storage is intentionally API-first and self-host-first.

Current backend shape:
- local filesystem storage backend
- legacy authenticated storage routes kept for simple app use
- S3-like path-style routes added for broader client/tool compatibility
- user isolation still applies under the current auth model

This page summarizes what Peanut supports today, what is partial, and what is explicitly out of scope for now.

## Route families

Legacy simple routes:
- `GET /api/storage`
- `GET /api/storage/*key`
- `PUT /api/storage/*key`
- `DELETE /api/storage/*key`

S3-like routes:
- bucket list: `GET /api/s3/:bucket`
- object read/write/delete/head: `GET|PUT|DELETE|HEAD /api/s3/:bucket/*key`
- multipart lifecycle: `POST|PUT|GET|DELETE /api/s3/:bucket/*key` with S3-style query params
- presign helper: `POST /api/s3/:bucket/presign/*key`

## Compatibility matrix

| Area | Status | Current support | Notes |
|---|---|---|---|
| Basic object CRUD | Supported | PUT / GET / HEAD / DELETE on `/api/s3/:bucket/*key` | Local filesystem backend under S3-like contract |
| Auth modes | Supported | Bearer auth, SigV4-style `Authorization` header auth, presigned query auth | Same protected backend, different auth entry points |
| Presigned URLs | Supported | Normal object URLs, `?tagging`, `?uploads`, multipart part/uploadId flows | Helper currently allows only `tagging` and `uploads` subresource hints |
| Object metadata headers | Supported | `Content-Type`, `Content-Length`, `ETag`, `Last-Modified` | `Last-Modified` returned as HTTP-date |
| Standard response headers | Supported | `Cache-Control`, `Content-Disposition`, `Content-Encoding`, `Content-Language`, `Expires` | Persisted on PUT and replayed on GET/HEAD |
| Custom metadata | Supported | `x-amz-meta-*` | Persisted and replayed on PUT/GET/HEAD |
| Conditional reads | Supported | `If-Match`, `If-None-Match`, `If-Modified-Since`, `If-Unmodified-Since` | GET and HEAD only |
| Byte ranges | Partial | Single-range GET, open-ended range, suffix range | Multi-range is not supported |
| ListObjectsV2 | Partial+ | `list-type=2`, `prefix`, `delimiter`, `max-keys`, `continuation-token`, `start-after`, `encoding-type=url`, `fetch-owner=true` | Practical client compatibility, not full AWS parity |
| Multipart upload | Supported | initiate, upload part, CopyPart, list uploads, list parts, complete, abort | Non-final parts must be at least 5 MiB |
| Multipart ETag | Supported | composite multipart ETag persisted after complete | Returned on complete and later GET/HEAD |
| CopyObject | Partial+ | path-style `x-amz-copy-source`, `COPY|REPLACE`, self-copy with `REPLACE` | Some advanced conditional copy headers are intentionally rejected |
| CopyPart | Supported | multipart part copy with optional `x-amz-copy-source-range` | Source must use `/bucket/key` path-style form |
| Checksums | Partial | `x-amz-checksum-sha256`, `x-amz-checksum-sha1` | Multiple checksum headers on one PUT are rejected |
| Object tagging | Partial+ | `x-amz-tagging` header + `GET/PUT/DELETE ?tagging` | Minimal tagging contract, not full AWS tagging surface |
| XML error envelopes | Supported | S3-like XML error responses on `/api/s3/...` | Legacy `/api/storage/*` stays JSON |

## Practical supported flows

### 1. Good current fits
- app-managed object upload/download
- presigned file exchange
- S3-like listing for folders/prefixes
- multipart upload for larger files
- object copy inside the same user scope
- operator automation using admin/service tokens or user auth

### 2. Explicitly supported details
- `HEAD` works for metadata reads
- presigned HEAD remains valid even though HEAD rejects behavioral subresources
- `max-keys=0` returns an empty page instead of failing
- `continuation-token` wins over `start-after` when both are present
- `fetch-owner=true` emits owner info in list responses

## Known limits and intentional exclusions

These are the main places where Peanut is still deliberately narrower than AWS S3:

| Topic | Current behavior |
|---|---|
| Raw AWS parity | Not the goal; Peanut follows a practical compatibility subset |
| Multi-range GET | Rejected with `416 InvalidRange` |
| Unsupported `encoding-type` values | Rejected; only `url` is accepted |
| HEAD with tagging/multipart behavioral subresources | Rejected explicitly |
| CopyObject conditional headers `x-amz-copy-source-if-*` | Rejected explicitly |
| `x-amz-copy-source-range` on CopyObject | Rejected; only CopyPart supports it |
| Arbitrary presign subresources | Rejected; helper only allows current bounded set |
| Full IAM/access-key management | Not implemented; current auth model is Peanut-scoped |
| Advanced tagging semantics | Minimal contract only |
| Full AWS checksum matrix | Only SHA-256 and SHA-1 currently supported |

## Recommendation for users

Choose the route family by use case:
- want the simplest app integration -> use legacy `/api/storage/*`
- want S3-like client/tool compatibility -> use `/api/s3/*`
- want temporary object access -> use presigned S3-like URLs
- want large-file upload -> use multipart S3-like flow

If you need strict AWS parity across every edge case, treat Peanut as partially compatible today rather than a drop-in replacement for every S3 client feature.
