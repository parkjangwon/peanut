# Peanut

<img width="1376" height="768" alt="1777174518806" src="https://github.com/user-attachments/assets/1658b9d2-a1a1-4dcc-b2aa-8adeb8516d0c" />

Peanut은 Rust 단일 바이너리로 배포되는 작은 self-host 백엔드 런타임이다.

핵심 방향은 명확하다.
- SQLite 기반 영속성
- 로컬 파일시스템 스토리지
- JWT 인증 + admin 승인 흐름
- 외부 앱/운영 도구를 위한 API-first 백엔드 표면
- ntfy 기반 push queue MVP

Peanut은 거대한 범용 플랫폼이 아니라, 작고 이해 가능하며 운영 복잡도가 낮은 백엔드 코어를 목표로 한다.

## 제품 철학

Peanut은 아래 원칙을 지향한다.

1. 단일 바이너리 배포
   - Rust 서버 하나로 전체 백엔드를 제공하고 `/` 에 API-first 랜딩 페이지를 서빙한다
2. 낮은 운영 복잡도
   - SQLite + local storage로 시작한다
3. 정직한 기능 범위
   - 큰 미완성 기능보다 작은 완성 기능을 우선한다
4. self-host 우선
   - 한 대의 서버, 한 개의 데이터 디렉터리, 한 개의 서비스로도 충분히 운영 가능해야 한다

## 현재 제공 기능

### Auth / Admin
- `POST /api/register`
  - 첫 유저는 자동으로 active admin이 된다
  - 이후 유저는 inactive 상태로 생성되고 admin 승인 후 활성화된다
- `POST /api/login`
  - 짧은 수명의 bearer access token, refresh token, 만료 시각을 포함한 JSON 응답 반환
- `POST /api/auth/refresh`
  - 유효한 refresh token을 회전시키고 새로운 access token + refresh token 쌍을 반환한다
- `POST /api/auth/logout`
  - refresh token을 revoke해서 외부 앱이 명시적으로 세션을 종료할 수 있게 한다
- `POST /api/auth/change-password`
  - 인증된 사용자의 비밀번호 변경
  - 성공 시 해당 유저의 기존 refresh session을 모두 revoke한다
- `POST /api/auth/forgot-password`
  - 일치하는 유저에 대해 password reset token을 생성한다
  - 전달 방식은 `PASSWORD_RESET_DELIVERY` 로 제어된다
  - `inline` 은 로컬/dev/self-host 흐름을 위해 reset token을 JSON으로 반환한다
  - `log` 는 응답에서 token을 숨기고 서버 로그에 기록해 운영자 전달 흐름을 만들 수 있게 한다
- `POST /api/auth/reset-password`
  - 1회용 reset token으로 새 비밀번호를 설정한다
  - 성공 시 해당 유저의 기존 refresh session을 모두 revoke한다
- `GET /api/auth/sessions`
  - 현재 사용자의 tracked auth session 목록을 반환한다
- `GET /api/auth/events`
  - 현재 사용자의 최근 auth event를 반환해서 audit/debug에 활용할 수 있다
- `DELETE /api/auth/sessions/:session_id`
  - 특정 auth session 하나를 revoke한다
- `POST /api/auth/sessions/revoke-all`
  - 현재 사용자의 auth session 전체를 revoke한다
- `GET /api/me`
  - 현재 인증된 유저 정보 JSON 반환
  - 보호된 API는 요청마다 현재 유저 레코드를 다시 확인하므로, 비활성화된 유저는 만료되지 않은 토큰이 있어도 즉시 접근이 차단된다
- `GET /api/admin/users`
  - admin 전용 유저 목록
- `PUT /api/admin/users/:user_id/activate`
  - admin 전용 승인 처리
- `PUT /api/admin/users/:user_id/deactivate`
  - admin 전용 비활성화 처리이며 해당 유저의 보호 API 접근을 즉시 차단한다
- `GET /api/admin/service-tokens`
  - server-to-server 자동화를 위한 admin service token 목록 조회
- `POST /api/admin/service-tokens`
  - opaque service token을 생성하고 plaintext token을 1회 반환
- `DELETE /api/admin/service-tokens/:token_id`
  - service token을 즉시 revoke

외부 프론트 앱 관점에서 의미하는 것:
- Peanut이 이제 signup, login, session refresh, logout, password change, password reset을 제공하는 auth backend로 동작할 수 있다
- access token은 짧게 유지하고 refresh token으로 장기 세션을 이어갈 수 있다
- refresh session은 서버에서 추적되며 logout, password change, password reset, admin deactivate 시 revoke된다
- 앱은 `GET /api/auth/events` 로 최근 auth event를 조회해 login/session/reset 흐름을 디버깅할 수 있다
- 운영자는 app-facing auth route를 특정 browser origin/client id로 제한할 수 있다
- 앱은 auth session 목록 조회, 단일 세션 revoke, 전체 세션 revoke까지 Peanut API로 처리할 수 있다
- 운영자는 protected API용 admin service token도 발급할 수 있다
- 자세한 연동 가이드는 `docs/auth-client.ko.md`, 브라우저 예제는 `examples/auth-client-web/` 참고

### 외부 auth client 가이드
- 한국어 가이드: `docs/auth-client.ko.md`
- English guide: `docs/auth-client.md`
- 브라우저 예제: `examples/auth-client-web/`

### Service token 가이드
- 한국어 가이드: `docs/service-tokens.ko.md`
- English guide: `docs/service-tokens.md`
- 운영 자동화 런북: `docs/automation-runbook.ko.md`
- curl 예제: `examples/service-tokens/`
- jq 보조 부트스트랩: `examples/service-tokens/create-token-jq.sh`
- end-to-end 운영 예제: `examples/operations-e2e/`
- 실행 가능한 automation 예제 + env 샘플: `examples/automation/`

### Storage
- 상세 호환 범위 표: `docs/storage-s3-compat.ko.md`
- English matrix: `docs/storage-s3-compat.md`
- 유저 단위로 격리된 object storage
- 기존 단순 endpoint도 계속 유지한다:
  - 조회
  - 업로드
  - 읽기
  - 삭제
- `/api/s3/:bucket/*key` 아래에 S3-like path-style endpoint를 추가했다
- 인증된 클라이언트는 `POST /api/s3/:bucket/presign/*key` 으로 presigned S3-like URL을 만들 수 있다
- presign helper는 이제 `PUT/GET/DELETE ...?tagging` 같은 object tagging subresource URL도 만들 수 있다
- presign helper는 이제 아래 multipart 호환 query 계약도 최소 범위로 지원한다:
  - `POST ...?uploads`
  - `PUT ...?partNumber=N&uploadId=...`
  - `GET ...?uploadId=...`
  - `POST ...?uploadId=...`
  - `DELETE ...?uploadId=...`
- presign helper에 `subresource`를 줄 때는 현재 `tagging`과 `uploads`만 허용한다
- S3-like object route는 bearer auth, SigV4-style `Authorization` header auth, 또는 presigned URL용 SigV4-style query auth를 받을 수 있다
- S3-like multipart upload는 이제 더 강한 S3 호환 계약을 지원한다:
  - `POST /api/s3/:bucket/*key?uploads` 로 `UploadId`를 발급받는다
  - `GET /api/s3/:bucket?uploads=1&prefix=...&max-uploads=...&key-marker=...&upload-id-marker=...` 로 active multipart upload를 marker pagination과 함께 조회한다
  - `PUT /api/s3/:bucket/*key` 에 `x-amz-copy-source: /src-bucket/src-key` 헤더를 주면 새 key로 CopyObject 한다
  - `x-amz-copy-source` 는 `/bucket/key` 형태의 path-style 값만 허용한다
  - `x-amz-metadata-directive: COPY|REPLACE` 로 CopyObject 메타데이터 동작을 제어할 수 있다
  - 같은 object를 자기 자신에게 CopyObject 하는 경우는 `x-amz-metadata-directive: REPLACE` 가 필요하다
  - `x-amz-copy-source-range` 는 CopyObject에서는 거부되며 CopyPart에서만 지원한다
  - `PUT /api/s3/:bucket/*key?partNumber=N&uploadId=...` 로 part를 업로드한다
  - 마지막 part를 제외한 multipart part는 S3 관례에 맞춰 최소 5 MiB 이상이어야 한다
  - `PUT /api/s3/:bucket/*key?partNumber=N&uploadId=...` 에 `x-amz-copy-source: /src-bucket/src-key` 헤더를 주면 기존 object에서 CopyPart 한다
  - `x-amz-copy-source-range: bytes=start-end` 로 ranged CopyPart도 지원한다
  - `GET /api/s3/:bucket/*key?uploadId=...&max-parts=...&part-number-marker=...` 로 staged part 목록을 marker pagination과 함께 조회한다
  - `POST /api/s3/:bucket/*key?uploadId=...` 에 `CompleteMultipartUpload` XML을 보내 최종 object를 조립한다
  - `DELETE /api/s3/:bucket/*key?uploadId=...` 로 staging upload를 중단한다
- multipart complete는 이제 최종 조립 object hash 대신 multipart composite ETag(`etag-partcount`)를 응답/저장한다
- ranged CopyPart 흐름에 대한 SDK-style smoke coverage도 추가했다
- S3-like object 응답은 content-type, content-length, ETag, last-modified 메타데이터를 포함한다
- S3-like GET은 이제 단일 `Range: bytes=...` 요청을 지원하고 `206 Partial Content` + `Content-Range`로 응답한다
- S3-like range 처리는 open-ended(`bytes=start-`)와 suffix(`bytes=-N`) 요청도 다루며, invalid/multi-range 요청은 `416 InvalidRange`로 거부한다
- S3-like GET과 HEAD는 이제 `If-Match`, `If-None-Match`, `If-Modified-Since`, `If-Unmodified-Since` 같은 기본 conditional request header를 처리한다
- `HEAD /api/s3/:bucket/*key` 는 object metadata 전용 경로로 유지되며, tagging/multipart subresource 스타일 query는 명시적으로 거부한다
- ETag validator가 있는 경우 date validator는 일반적인 HTTP precondition 우선순위에 따라 무시된다 (`If-Match` > `If-Unmodified-Since`, `If-None-Match` > `If-Modified-Since`)
- `Cache-Control`, `Content-Disposition`, `Content-Encoding`, `Content-Language`, `Expires` 같은 object response header는 PUT 시 저장되고 이후 PUT/GET/HEAD 응답에 다시 반영된다
- `x-amz-metadata-directive: REPLACE` 를 사용하는 CopyObject는 source object의 표준 response header를 기본 유지하면서, 명시적으로 전달한 값만 치환한다
- CopyObject는 이제 tagging/checksum 상호작용 계약을 문서화하고 현재 구현과 맞춘다:
  - 기본 `COPY` 는 저장된 tagging과 checksum header를 유지한다
  - `REPLACE` 는 tagging을 덮어쓸 수 있지만 source checksum 계약은 유지한다
- CopyObject에서는 `x-amz-copy-source-if-*` 조건부 헤더를 아직 지원하지 않으며, 전달 시 `InvalidRequest`로 거부한다
- PUT 시 `x-amz-checksum-sha256` 를 보내면 payload와 일치하는지 검증하고, 저장된 object 응답(이후 PUT/GET/HEAD)에도 다시 내려준다
- PUT 시 `x-amz-checksum-sha1` 도 같은 최소 계약으로 지원한다
- 같은 PUT에 checksum header를 여러 개 보내면 명시적으로 거부한다
- PUT 시 `x-amz-tagging` 을 보내면 최소 계약으로 tag count를 저장하고 이후 object 응답에서 `x-amz-tagging-count` 로 다시 내려준다
- `x-amz-tagging` 은 이제 URL-encoded query-string 형태로 정규화되어, 공백이나 `/` 같은 값도 `GET ?tagging` 왕복에서 보존된다
- `x-amz-tagging` 의 percent-encoding 이 잘못되면 그대로 저장하지 않고 명시적으로 거부한다
- object tagging subresource도 최소 계약으로 지원한다:
  - `GET /api/s3/:bucket/*key?tagging`
  - `PUT /api/s3/:bucket/*key?tagging` + `Tagging` XML
  - `DELETE /api/s3/:bucket/*key?tagging`
- tagging XML은 최소 계약으로 검증하며 duplicate key와 10개 초과 tag는 명시적으로 거부한다
- PUT 시 전달한 `x-amz-meta-*` custom object metadata는 이제 저장되며, 이후 PUT/GET/HEAD 응답에서도 다시 내려간다
- S3-like 성공/에러 응답은 `x-amz-request-id` 헤더를 포함하고, object `Last-Modified` 헤더는 HTTP-date 형식으로 내려간다
- S3-like bucket listing은 `list-type=2`, `prefix`, `delimiter`, `max-keys`, `continuation-token`, `start-after`, `encoding-type=url`, `fetch-owner=true`를 지원한다
- `continuation-token` 과 `start-after` 가 함께 오면 Peanut은 opaque continuation token을 우선하고 `start-after` 는 무시한다
- 잘못된 `encoding-type` 값은 `InvalidArgument` 로 거부한다
- `max-keys=0` 요청은 실패하지 않고 빈 page + truncation metadata를 반환한다
- continuation token은 raw key 대신 opaque base64url 스타일 토큰으로 내려간다
- `delimiter=/` 사용 시 `CommonPrefixes` XML 블록도 함께 내려준다
- S3-like storage 에러는 이제 `NoSuchKey`, `InvalidRequest` 같은 XML error envelope로 응답한다
- storage key는 계속 인증 유저별로 자동 격리된다

### Data API (SQLite 기반)
Peanut은 이제 Peanut이 관리하는 logical table용 제한된 SQLite 기반 data API를 제공한다.

상세 가이드:
- English guide: `docs/data-api.md`
- 한국어 가이드: `docs/data-api.ko.md`
- payload 예제: `examples/data-api/`

현재 가능한 것:
- `GET /api/data/tables`
- `POST /api/data/tables`
- `GET /api/data/tables/:table`
- `PATCH /api/data/tables/:table`
- `GET /api/data/tables/:table/presets`
- `POST /api/data/tables/:table/presets`
- `GET /api/data/tables/:table/presets/:preset_id/run`
- `PATCH /api/data/tables/:table/presets/:preset_id`
- `DELETE /api/data/tables/:table/presets/:preset_id`
- `GET /api/data/tables/:table/export`
- `POST /api/data/tables/:table/import`
- `GET /api/data/tables/:table/rows`
- `POST /api/data/tables/:table/rows`
- `GET /api/data/tables/:table/events`
- `GET /api/data/tables/:table/events/checkpoint`
- `GET /api/data/tables/:table/events/stream`
- `GET /api/data/tables/:table/rows/:row_id`
- `PATCH /api/data/tables/:table/rows/:row_id`
- `DELETE /api/data/tables/:table/rows/:row_id`

현재 모델:
- admin이 JSON schema + 고정 access policy로 logical table을 정의한다
- row는 Peanut이 관리하는 SQLite 테이블에 저장된다
- `owner_private` 정책은 인증 유저별 row 격리를 제공한다
- row 변경은 내부 이벤트 로그에 기록된다
- admin API는 `GET /api/data/tables/:table/events/checkpoint` 로 최신 durable row-event checkpoint를 먼저 확인한 뒤 realtime consumer를 붙일 수 있다
- admin API는 `GET /api/data/tables/:table/events?since_id=<event_id>`로 row mutation을 재생해서 resume/sync 흐름에 사용할 수 있다
- admin API로 `GET /api/data/tables/:table/events/stream`에서 row mutation 실시간 이벤트를 SSE로 구독할 수 있고, 각 payload에는 event id가 포함된다
- admin API로 table별 reusable query preset을 저장해 반복 조회에 재사용할 수 있다
- bounded admin API로 table snapshot export/import가 가능하다
- schema 업데이트는 이제 안전한 진화 규칙을 따른다:
  - 기존 field type은 in-place 변경할 수 없다
  - row가 이미 있는 테이블에서는 기존 field를 제거할 수 없다
  - row가 이미 있는 테이블에 새 required field를 추가하려면 default가 필요하다

여전히 아닌 것:
- `POST /api/sql` 같은 raw SQL은 열지 않는다
- DB 콘솔 서비스처럼 가려는 것은 아니다
- query/filter 기능은 이번 릴리스에서 의도적으로 좁게 유지한다

### Push (현재 릴리스 MVP)
Peanut은 현재 실용적인 hybrid push 레이어를 제공한다.
- 간단한 self-host 흐름용 ntfy topic 구독
- VAPID 환경변수가 설정된 경우 저장된 browser subscription 대상 Web Push 전송

엔드포인트:
- `GET /api/push/subscriptions`
- `POST /api/push/subscriptions`
- `DELETE /api/push/subscriptions/:subscription_id`
- `GET /api/push/vapid-public-key`
- `POST /api/push/messages`
- `GET /api/push/queue`
- `GET /api/push/queue/stats`

런타임 설정:
- `NTFY_BASE_URL`
  - 기본값은 `https://ntfy.sh`
  - `https://push.example.com` 같은 self-host ntfy 서버로 바꿔서 사용할 수 있다
- `NTFY_AUTH_TOKEN`
  - 인증이 필요한 ntfy 서버용 optional bearer token

의미하는 것:
- 유저는 `{ "topic": "alerts_main" }` 형태로 ntfy topic을 구독할 수 있다
- 브라우저는 `{ "endpoint": "...", "keys": { "p256dh": "...", "auth": "..." } }` 형태로 Web Push subscription을 등록할 수 있다
- 클라이언트는 `GET /api/push/vapid-public-key`로 browser `PushManager.subscribe(...)`에 필요한 public key를 가져올 수 있다
- push 메시지는 SQLite queue에 쌓인다
- 백그라운드 워커가 ntfy 또는 Web Push subscription으로 전달한다
- `GET /api/push/queue`는 이제 total/pending/processing/sent/failed/partial_success 집계를 담은 `summary` 블록도 함께 반환한다
- 여러 destination 중 하나만 성공해도 queue item 전체는 sent로 처리해서, 하나의 죽은 subscription 때문에 전체 전송이 막히지 않는다
- subscription이 하나도 없는 item은 의미 없는 재시도 대신 즉시 terminal failure로 정리한다
- 영구적으로 잘못된 delivery 경로도 즉시 terminal failure로 다룬다. 남은 Web Push subscription이 모두 dead endpoint라 prune된 경우 추가 retry를 태우지 않고, ntfy 4xx 응답도 운영자 수정이 필요한 non-retryable 오류로 본다
- 누락되거나 잘못된 Web Push VAPID 런타임 설정도 terminal operator error로 취급해서, env가 고쳐질 때까지 의미 없이 retry를 반복하지 않는다
- queue 상태, retry 횟수, 마지막 에러, `next_retry_at`를 API/콘솔에서 볼 수 있다
- 404/410 성격의 terminal Web Push 에러를 돌려주는 죽은 subscription은 subscription 테이블에서 자동 정리된다
- partial delivery 메타데이터도 구조화되어서, queue item은 이제 `last_error`와 함께 `partial_failure_count`, `failed_destinations[]`를 노출한다
- queue summary는 현재 `ntfy_subscriptions`, `web_push_subscriptions` 수도 함께 노출해서 delivery kind별 운영 가시성을 높인다
- `GET /api/push/queue/stats`는 최근 failure reason top-N을 terminal item failure와 destination-level delivery failure로 나눠서 보여준다
- queue item이 `sent`로 끝나더라도 partial delivery failure는 `last_error`에 남겨서, 성공 전달은 유지하면서도 죽은 destination을 운영에서 바로 찾을 수 있다

아직 아닌 것:
- Peanut이 완전한 push 플랫폼을 지향하는 것은 아니다
- 내장 콘솔에 polished service-worker 등록 흐름까지 들어간 상태는 아니다
- Web Push 전송에는 VAPID 런타임 환경변수 설정이 필요하다

### Peanut Functions (JS/TS sandbox MVP)
Peanut은 이제 작은 백엔드 확장용 함수 런타임을 함께 제공한다.

현재 가능한 것:
- admin이 SQLite에 function 메타데이터와 소스 저장
- JavaScript / TypeScript 함수 코드를 콘솔/API에서 관리
- function별 endpoint slug, invoke policy, env/secrets JSON, allowed origins, rate limit, timeout 설정
- secret은 함수 버전별로 별도 저장되며 API 응답에는 값이 노출되지 않고 `secret_key_count`만 제공된다
- `POST /api/functions/endpoints/:endpoint_slug`로 authenticated / public / admin_only / api_key 정책 호출
- 같은 endpoint에서 `async_invoke: true`로 inline sync 실행 또는 queued async 실행 선택 가능
- admin API로 `GET /api/functions/:name/versions`에서 함수 버전 이력을 조회 가능
- admin API로 `POST /api/functions/:name/versions/:version_number/rollback`에서 active 버전을 롤백 가능
- admin API로 `GET /api/functions/:name/events`에서 invocation lifecycle 실시간 이벤트를 SSE로 구독 가능
- temp working directory + timeout 제한을 둔 별도 Node subprocess 실행
- invocation 로그를 SQLite에 저장하고, queued/running/succeeded/failed lifecycle, `invoke_mode`, `function_version_id`, `retry_count`, `parent_invocation_id`, 상세 조회, attempt chain 조회, 재실행까지 콘솔/API에서 가능
- authenticated function 안에서 사용할 수 있는 bounded Peanut host binding 제공:
  - `ctx.peanut.storage.list/get/put/delete`
  - `ctx.peanut.push.enqueue`
  - `ctx.peanut.data.listRows/getRow/createRow/updateRow/deleteRow`
- host binding은 Peanut의 기존 auth/policy 검사를 그대로 재사용하므로, 함수 안에서도 owner scope 데이터/스토리지 격리가 유지된다

현재 제약:
- 함수는 `default` 또는 named `handler`를 export해야 한다
- 입력/출력은 JSON만 지원한다
- arbitrary package 설치는 지원하지 않는다
- 런타임 탈출 위험이 있는 일부 패턴은 소스 단계에서 차단한다
- 외부 네트워크를 직접 여는 대신, Peanut 내부 primitive를 bounded host binding으로만 확장한다
- 완전한 Lambda clone이 아니라, 좁게 제한된 확장 레이어다

### 콘솔 / 운영 표면
Peanut은 현재 API-first 모드로 동작한다.

- 기존 내장 Next.js 콘솔 소스는 제거되었다
- 백엔드는 `/` 에서 간단한 landing page를 제공하고, 실제 기능은 `/api/...` 로 계속 사용할 수 있다
- 새 운영 콘솔은 v2의 일부로 backend core와 분리해서 다시 설계할 예정이다

## API 요약

요청/응답 메모:
- 모든 응답에는 correlation용 `x-request-id` 헤더가 포함된다
- JSON 에러 응답은 `error`, `code`, `request_id` 구조를 사용한다

에러 응답 예시:

```json
{
  "error": "missing bearer token",
  "code": "unauthorized",
  "request_id": "req_123"
}
```

### `GET /api/health`

```json
{
  "status": "ok",
  "message": "Systems are operational."
}
```

### `GET /api/ready`
운영자가 보는 backend readiness 상태를 반환한다.

```json
{
  "status": "ready",
  "checks": [
    { "name": "database", "ok": true, "message": "database query succeeded" },
    { "name": "storage", "ok": true, "message": "storage directory is writable", "path": "data/storage" }
  ]
}
```

### `POST /api/register`
요청:

```json
{
  "email": "admin@example.com",
  "password": "secret123"
}
```

응답:

```json
{
  "message": "First user registered as active admin.",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

검증 규칙:
- email 필수 + 기본적인 이메일 형식 검사
- password 최소 8자

### `POST /api/login`

```json
{
  "access_token": "***",
  "refresh_token": "***",
  "token_type": "Bearer",
  "expires_at": "2026-04-25T00:00:00Z",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### `GET /api/me`

```json
{
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### `GET /api/storage`

```json
{
  "keys": ["notes/welcome.txt"]
}
```

### S3-like storage endpoints
- `GET /api/s3/:bucket?list-type=2&prefix=notes/&max-keys=100&continuation-token=...`
- `HEAD /api/s3/:bucket/*key`
- `GET /api/s3/:bucket/*key`
- `PUT /api/s3/:bucket/*key`
- `DELETE /api/s3/:bucket/*key`

### `POST /api/push/subscriptions`

```json
{
  "topic": "alerts_main"
}
```

### `POST /api/data/tables`
대표적인 admin 요청:

```json
{
  "name": "todos",
  "display_name": "Todos",
  "schema": {
    "fields": {
      "title": { "type": "string", "required": true, "max_length": 200 },
      "done": { "type": "boolean", "required": false, "default": false }
    }
  },
  "access_policy": {
    "mode": "owner_private"
  }
}
```

대표 응답:

```json
{
  "table": {
    "name": "todos",
    "display_name": "Todos",
    "schema": {
      "fields": {
        "title": { "type": "string", "required": true, "max_length": 200, "default": null },
        "done": { "type": "boolean", "required": false, "max_length": null, "default": false }
      }
    },
    "access_policy": {
      "mode": "owner_private"
    }
  }
}
```

### `POST /api/data/tables/:table/rows`
대표적인 인증 요청:

```json
{
  "data": {
    "title": "buy milk"
  }
}
```

대표 응답:

```json
{
  "row": {
    "id": "uuid",
    "owner_user_id": "uuid",
    "data": {
      "title": "buy milk",
      "done": false
    },
    "created_at": "2026-04-25 01:05:58",
    "updated_at": "2026-04-25 01:05:58"
  }
}
```

### `GET /api/data/tables/:table/rows`
예시 query:

```text
/api/data/tables/todos/rows?filter_field=title&filter_op=contains&filter_value=milk&order_by=created_at&order=desc&limit=10
```

기대 동작:
- `search`는 선언된 string field를 bounded하게 훑는다
- `title_contains`와 범용 `filter_field/filter_op/filter_value`를 `order_by`, `order`, `limit`, `offset`과 함께 조합할 수 있다
- 콘솔 기본 흐름에서는 `contains`, `starts_with`, `ends_with`, `eq`, `ne`, `gt`, `gte`, `lt`, `lte`를 사용한다

### `GET /api/data/tables/:table/export`
admin snapshot export:
- table 메타데이터와 정규화된 row를 함께 반환한다
- `metadata.export_version`, `metadata.row_count`, `metadata.checksum_sha256`를 포함한다
- checksum은 export된 table+rows artifact 기준으로 계산되어 백업 검증에 쓸 수 있다
- 백업, 환경 간 마이그레이션, fixture 생성에 쓸 수 있다

### `GET /api/data/tables/:table/events`
### `GET /api/data/tables/:table/events/checkpoint`
admin row event log:
- `limit`, `row_id`, `action`, `since_id`를 지원한다
- `GET /api/data/tables/:table/events/checkpoint`는 resume checkpoint용 최신 durable row-event id를 반환한다
- 기본 모드는 최신 이벤트부터 반환해서 audit/debugging에 맞춘다
- `since_id`를 주면 오름차순 replay로 바뀌어 resume/sync worker에 적합하다

### `GET /api/data/tables/:table/events/stream`
admin row realtime stream:
- row mutation 이벤트용 SSE endpoint
- insert, update, delete 이벤트를 실시간으로 흘려준다
- 각 payload에 durable event `id`가 포함되어 `since_id` 기반 resume가 가능하다
- 운영 대시보드나 live sync worker에 유용하다

### `GET /api/data/tables/:table/presets`
### `POST /api/data/tables/:table/presets`
### `GET /api/data/tables/:table/presets/:preset_id/run`
### `PATCH /api/data/tables/:table/presets/:preset_id`
### `DELETE /api/data/tables/:table/presets/:preset_id`
admin saved query presets:
- table별 bounded row-query params를 재사용 가능한 preset으로 저장한다
- "open items", "recent failures", "buy-* tasks" 같은 반복 조회에 유용하다
- preset에는 `search`, filters, ordering, limit, offset이 저장된다
- `GET /api/data/tables/:table/presets/:preset_id/run`으로 저장된 preset을 바로 실행해 bounded row 결과를 받을 수 있다

### `POST /api/data/tables/:table/import`
admin snapshot import:
- `{ "mode": "append" | "replace", "rows": [...] }` 형태를 받는다
- `restore_table: true`를 주면 row insert 전에 `display_name`, `schema`, `access_policy`도 함께 복원할 수 있다
- `verify_checksum: true`와 `metadata`를 함께 보내면 import 전에 artifact 무결성을 먼저 검증한다
- checksum 검증은 export artifact 기준 필드(`table.created_by`, `table.created_at`, row id, `created_at`, `updated_at`)가 필요하다
- import row는 insert 전에 현재 schema 기준으로 정규화된다
- owner_private 테이블은 각 row마다 `owner_user_id`가 필요하다

## 디렉터리 구조

```text
.
├── Cargo.toml
├── Cargo.lock
├── Dockerfile
├── docker-compose.yml
├── build.rs
├── migrations/
├── src/
│   ├── main.rs
│   ├── db.rs
│   ├── console.rs
│   ├── i18n.rs
│   ├── api/
│   │   ├── admin.rs
│   │   ├── auth.rs
│   │   ├── common.rs
│   │   ├── health.rs
│   │   ├── push.rs
│   │   ├── storage.rs
│   │   └── mod.rs
│   ├── auth/
│   ├── middleware/
│   ├── push/
│   └── storage/
└── locales/
```

## 환경변수

필수:
- `JWT_SECRET`

선택:
- `DATABASE_URL` (기본값: `sqlite://peanut.db`; `sqlite:` URL 이어야 함)
- `STORAGE_DIR` (기본값: `data/storage`; 비어 있으면 안 됨)
- `BIND_ADDR` (기본값: `127.0.0.1:3000`; 유효한 socket address 여야 함)
- `MAX_UPLOAD_BYTES` (기본값: `5242880`; 0보다 큰 정수여야 함)
- `PASSWORD_RESET_DELIVERY` (기본값: `inline`; `inline` 또는 `log`)
- `AUTH_ALLOWED_ORIGINS` (쉼표 구분 origin 목록; 설정하면 auth route에서 일치하는 `Origin` 헤더가 필요함)
- `AUTH_ALLOWED_CLIENT_IDS` (쉼표 구분 client id 목록; 설정하면 auth route에서 일치하는 `x-peanut-client-id` 헤더가 필요함)
- `RUST_LOG` (기본값: `info`)
- `WEB_PUSH_VAPID_PRIVATE_KEY` (Web Push 전송 시에만 필요)
- `WEB_PUSH_VAPID_SUBJECT` (Web Push 전송 시에만 필요; `mailto:` 또는 `https://`)

시작점은 `.env.example` 참고.

## curl 기반 API quickstart

콘솔을 열지 않고 Peanut을 가장 빨리 만져보는 흐름이다.

```bash
export BASE_URL=http://127.0.0.1:3000
export CLIENT_ID=peanut-web-dev

# 1) 첫 admin 등록
curl -s -X POST "$BASE_URL/api/register" \
  -H 'content-type: application/json' \
  -H "x-peanut-client-id: $CLIENT_ID" \
  -d '{"email":"admin@example.com","password": "***"}'

# 2) 로그인
LOGIN_JSON=$(curl -s -X POST "$BASE_URL/api/login" \
  -H 'content-type: application/json' \
  -H "x-peanut-client-id: $CLIENT_ID" \
  -d '{"email":"admin@example.com","password": "***"}')

# 3) login 응답의 access_token 값을 복사해서 아래에 넣기

# 4) access token으로 table 생성
curl -s -X POST "$BASE_URL/api/data/tables" \
  -H 'authorization: Bearer <PASTE_ACCESS_TOKEN_HERE>' \
  -H 'content-type: application/json' \
  -d '{
    "name": "todos",
    "display_name": "Todos",
    "schema": {
      "fields": {
        "title": { "type": "string", "required": true, "max_length": 200 },
        "done": { "type": "boolean", "required": false, "default": false }
      }
    },
    "access_policy": { "mode": "owner_private" }
  }'

# 5) row 추가
curl -s -X POST "$BASE_URL/api/data/tables/todos/rows" \
  -H "content-type: application/json" \
  -H 'authorization: Bearer <PASTE_ACCESS_TOKEN_HERE>' \
  -d '{"data":{"title":"buy milk"}}'

# 6) filter/search/offset으로 row 조회
curl -s "$BASE_URL/api/data/tables/todos/rows?search=buy&filter_field=title&filter_op=starts_with&filter_value=buy&order_by=title&order=asc&limit=10&offset=0" \
  -H 'authorization: Bearer <PASTE_ACCESS_TOKEN_HERE>'
```

짧은 메모:
- 첫 등록 유저는 자동으로 active admin이 된다
- `AUTH_ALLOWED_CLIENT_IDS` 를 켠 경우 위 예시처럼 auth route에 `x-peanut-client-id` 를 계속 보내야 한다
- `owner_private` row는 인증 유저 기준으로 격리된다
- 같은 bearer token으로 storage, data, push, session 엔드포인트를 함께 호출할 수 있다
- server-to-server admin 자동화는 `docs/service-tokens.ko.md` 참고
- cron/운영 자동화 패턴은 `docs/automation-runbook.ko.md` 참고
- Data API를 조금 더 실전적으로 보려면 `docs/data-api.ko.md` 참고
- 바로 보낼 수 있는 payload 예제는 `examples/data-api/` 참고
- service token + data + storage를 한 번에 보는 운영 예제는 `examples/operations-e2e/` 참고
- 해당 예제에는 Data API list/export/import와 checkpoint/replay 단계도 포함되어 있다
- env 파일 기반 cron 스크립트 예제는 `examples/automation/` 참고
- 외부 프론트 auth 전체 흐름은 `docs/auth-client.ko.md`와 `examples/auth-client-web/` 참고

## 로컬 개발

### 필요 조건
- Rust toolchain

### 테스트

```bash
cargo test
```

### 전체 빌드

```bash
./scripts/build.sh
```

### 바이너리 실행

```bash
export JWT_SECRET='replace-this'
./target/release/peanut
```

브라우저 접속:
- `http://127.0.0.1:3000`

## Docker 실행

```bash
cp .env.example .env
# .env에서 JWT_SECRET 수정

docker compose up --build
```

### Docker Compose 운영 가이드

기본 `docker-compose.yml` 운영 포인트:
- 컨테이너 `3000` 포트를 호스트 `3000`에 노출한다
- `./data`를 컨테이너 `/app/data`에 마운트한다
- 기본 SQLite 경로는 `sqlite://data/peanut.db`
- 기본 storage 경로는 `data/storage`
- restart 정책은 `always`

권장 day-1 흐름:

```bash
cp .env.example .env
# JWT_SECRET 설정
# 필요하면 WEB_PUSH_VAPID_PRIVATE_KEY / WEB_PUSH_VAPID_SUBJECT도 설정

docker compose up --build -d
docker compose logs -f peanut
```

권장 day-2 운영 명령:

```bash
# 설정 변경 후 재배포
docker compose up -d --build

# 최근 로그 확인
docker compose logs --tail=200 peanut

# 데이터 유지한 채 정지
docker compose stop

# 다시 시작
docker compose start
```

백업/복구 메모:
- `./data/peanut.db`와 `./data/storage/`를 백업하면 된다
- 기본 compose 레이아웃을 유지한다면 `./data/` 전체 백업이면 충분하다
- 복구 시에는 컨테이너를 멈추고 `./data/`를 교체한 뒤 다시 시작하면 된다

## 로컬 브라우저 Web Push 실험 가이드

로컬 브라우저에서 Web Push 경로를 끝까지 확인하고 싶을 때 아래 순서로 보면 된다.

1. 런타임 환경변수 설정
   - `JWT_SECRET`
   - `WEB_PUSH_VAPID_PRIVATE_KEY`
   - `WEB_PUSH_VAPID_SUBJECT`
2. Peanut 실행 후 `http://127.0.0.1:3000` 접속
3. 첫 admin 유저 등록 후 로그인
4. Push 섹션에서 VAPID public key가 자동으로 채워지는지 확인
5. `Register browser Web Push` 클릭
6. 브라우저 알림 권한 요청이 뜨면 허용
7. 콘솔에 `web_push` subscription이 생기는지 확인
8. push 메시지 enqueue
9. queue 항목이 `sent`로 바뀌는지 확인하고, 실패하면 `last_error` 확인

메모:
- 자동 브라우저 등록은 실제 브라우저 알림 권한이 필요하다
- 브라우저 권한 팝업이 막히는 환경에서는 manual Web Push subscription 폼으로 백엔드 API 경로를 검증할 수 있다
- `GET /api/push/vapid-public-key`가 404를 반환하면 먼저 VAPID 환경변수를 확인하면 된다

## 릴리스 전 체크리스트

```bash
cargo test
./scripts/build.sh
node --check examples/auth-client-web/app.js
```

수동 확인:
1. `/` 를 열어 API-first 랜딩 페이지가 뜨는지 확인
2. 첫 admin 유저 등록
3. 로그인
4. data table 생성
5. row 생성/수정
6. title filter 또는 generic field filter 동작 확인
7. ntfy topic 구독 후 push queue 메시지 전송
8. VAPID가 설정돼 있으면 public key 자동 로드와 browser/manual Web Push subscription 확인
9. auth client policy를 켰다면 `examples/auth-client-web/` 에서 `x-peanut-client-id` 연동까지 확인

## 백업과 운영

단일 노드 배포 기준으로는 아래를 함께 백업하면 된다.
- SQLite DB 파일
- storage 디렉터리

기본 docker-compose 기준으로는 `./data/` 전체를 백업하면 된다.

## 현재 비목표

Peanut은 의도적으로 다음을 목표로 하지 않는다.
- 거대한 멀티테넌트 백엔드 클라우드
- 플러그인/오케스트레이션 프레임워크
- Supabase/Firebase 대체재 전체 구현
- 완전한 Web Push 플랫폼

## 라이선스

아직 라이선스 파일은 정해지지 않았다.
공개 배포를 하려면 릴리스 전에 명시적인 라이선스를 추가하는 것이 좋다.
