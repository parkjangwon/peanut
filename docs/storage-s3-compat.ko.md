# Peanut Storage S3 호환 범위 표

Peanut storage는 의도적으로 API-first, self-host-first 방향을 따른다.

현재 백엔드 형태:
- 로컬 파일시스템 storage backend
- 간단한 앱 사용을 위한 legacy authenticated storage route 유지
- 더 넓은 client/tool 호환을 위한 S3-like path-style route 추가
- 현재 auth 모델 기준으로 user isolation 유지

이 문서는 Peanut이 지금 무엇을 지원하는지, 어디가 partial인지, 무엇을 의도적으로 제외했는지 빠르게 정리한다.

## 라우트 계열

Legacy simple route:
- `GET /api/storage`
- `GET /api/storage/*key`
- `PUT /api/storage/*key`
- `DELETE /api/storage/*key`

S3-like route:
- bucket list: `GET /api/s3/:bucket`
- object read/write/delete/head: `GET|PUT|DELETE|HEAD /api/s3/:bucket/*key`
- multipart lifecycle: S3-style query param을 붙인 `POST|PUT|GET|DELETE /api/s3/:bucket/*key`
- presign helper: `POST /api/s3/:bucket/presign/*key`

## 호환 범위 표

| 영역 | 상태 | 현재 지원 | 메모 |
|---|---|---|---|
| 기본 object CRUD | 지원 | `/api/s3/:bucket/*key` 에서 PUT / GET / HEAD / DELETE | 로컬 파일시스템 backend를 S3-like 계약으로 노출 |
| 인증 방식 | 지원 | Bearer auth, SigV4-style `Authorization` header auth, presigned query auth | 같은 protected backend에 여러 auth 진입 방식 |
| Presigned URL | 지원 | 일반 object URL, `?tagging`, `?uploads`, multipart part/uploadId 흐름 | helper는 현재 `tagging`, `uploads` 힌트만 허용 |
| Object 메타데이터 헤더 | 지원 | `Content-Type`, `Content-Length`, `ETag`, `Last-Modified` | `Last-Modified` 는 HTTP-date 형식 |
| 표준 response 헤더 | 지원 | `Cache-Control`, `Content-Disposition`, `Content-Encoding`, `Content-Language`, `Expires` | PUT 시 저장되고 GET/HEAD에 재반영 |
| Custom metadata | 지원 | `x-amz-meta-*` | PUT/GET/HEAD에서 저장/재반영 |
| Conditional read | 지원 | `If-Match`, `If-None-Match`, `If-Modified-Since`, `If-Unmodified-Since` | GET / HEAD 기준 |
| Byte range | Partial | 단일 range GET, open-ended range, suffix range | multi-range는 미지원 |
| ListObjectsV2 | Partial+ | `list-type=2`, `prefix`, `delimiter`, `max-keys`, `continuation-token`, `start-after`, `encoding-type=url`, `fetch-owner=true` | 실용적 호환이지 완전 AWS parity는 아님 |
| Multipart upload | 지원 | initiate, upload part, CopyPart, list uploads, list parts, complete, abort | 마지막 part를 제외한 part는 최소 5 MiB |
| Multipart ETag | 지원 | complete 후 composite multipart ETag 저장 | complete 응답과 이후 GET/HEAD에 반영 |
| CopyObject | Partial+ | path-style `x-amz-copy-source`, `COPY|REPLACE`, `REPLACE` 기반 self-copy | 일부 고급 conditional copy header는 명시적으로 거부 |
| CopyPart | 지원 | optional `x-amz-copy-source-range` 포함 multipart part copy | source는 `/bucket/key` path-style 형식 필요 |
| Checksum | Partial | `x-amz-checksum-sha256`, `x-amz-checksum-sha1` | 한 PUT에 여러 checksum header는 거부 |
| Object tagging | Partial+ | `x-amz-tagging` header + `GET/PUT/DELETE ?tagging` | 최소 tagging 계약만 지원 |
| XML error envelope | 지원 | `/api/s3/...` 에서는 S3-like XML error | legacy `/api/storage/*` 는 JSON 유지 |

## 지금 잘 맞는 사용 흐름

### 1. 현재 특히 잘 맞는 용도
- 앱 업로드/다운로드
- presigned 파일 교환
- prefix/folder 기반 S3-like listing
- 큰 파일용 multipart upload
- 같은 user scope 안에서 object copy
- admin/service token 또는 user auth 기반 운영 자동화

### 2. 현재 분명히 지원하는 디테일
- `HEAD` 로 metadata 조회 가능
- HEAD는 behavioral subresource를 거부하지만 presigned HEAD 자체는 정상 동작
- `max-keys=0` 는 실패하지 않고 empty page 반환
- `continuation-token` 과 `start-after` 를 같이 보내면 continuation-token 우선
- `fetch-owner=true` 는 list response에 owner 정보 포함

## 알려진 제한과 의도적인 제외 범위

아래는 Peanut이 아직 AWS S3보다 의도적으로 좁은 지점들이다.

| 주제 | 현재 동작 |
|---|---|
| 완전한 AWS parity | 목표 아님. Peanut은 실용적인 호환 subset을 지향 |
| Multi-range GET | `416 InvalidRange` 로 거부 |
| 지원하지 않는 `encoding-type` 값 | 거부. 현재 `url`만 허용 |
| HEAD + tagging/multipart behavioral subresource | 명시적으로 거부 |
| CopyObject의 `x-amz-copy-source-if-*` | 명시적으로 거부 |
| CopyObject의 `x-amz-copy-source-range` | 거부. CopyPart만 지원 |
| 임의 presign subresource | 거부. 현재 bounded set만 허용 |
| 완전한 IAM/access-key 관리 | 미구현. 현재는 Peanut 범위 auth 모델 |
| 고급 tagging semantics | 최소 계약만 지원 |
| 전체 AWS checksum 매트릭스 | 현재 SHA-256, SHA-1만 지원 |

## 사용자에게 권장하는 선택

용도별로 route를 고르면 된다:
- 가장 단순한 앱 연동 -> legacy `/api/storage/*`
- S3-like client/tool 호환 -> `/api/s3/*`
- 임시 object 접근 -> presigned S3-like URL
- 큰 파일 업로드 -> multipart S3-like flow

모든 edge case까지 strict AWS parity가 필요하다면, 지금 Peanut은 완전 대체재라기보다 부분 호환 구현으로 보는 것이 맞다.
