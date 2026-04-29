# Peanut 기술 심층 분석 보고서

작성일: 2026-04-29
분석가: pjw's agent

## 1. 프로젝트 개요 (Overview)

- **한 줄 요약:** Rust/Axum으로 작성된 단일 바이너리 self-host 백엔드 런타임.
- **주요 목적:** 솔로 개발자나 소규모 팀이 별도 BaaS, 오브젝트 스토리지, 푸시 서버, 간단한 데이터 백엔드, 함수 실행기를 따로 운영하지 않고 한 프로세스와 SQLite/로컬 파일시스템으로 최소 백엔드 코어를 운영하게 하는 것이 목적이다.
- **타겟 사용자:** 작은 웹/모바일 앱을 직접 배포하는 개발자, 내부 운영 도구를 빠르게 붙이고 싶은 소규모 팀, 외부 SaaS 의존도를 줄이고 싶은 self-host 지향 사용자.
- **프로젝트 성격:** PocketBase/Supabase/Firebase의 일부 문제 영역을 훨씬 좁은 범위에서 흉내 내는 API-first 백엔드 코어다. README의 "giant backend platform이 아니다"라는 선언은 실제 구현 범위와 대체로 일치한다.

## 2. 기술 스택 및 아키텍처 (Tech & Architecture)

- **언어/프레임워크:** Rust 2021, Axum 0.7, Tokio 1.x, SQLx 0.7 SQLite, tower-http CORS, jsonwebtoken, argon2, rust-embed, web-push, reqwest, DashMap.
- **저장소:** SQLite가 영속 데이터의 중심이고, 오브젝트 스토리지는 로컬 파일시스템을 사용한다.
- **배포:** `Dockerfile`, `docker-compose.yml`, `scripts/build.sh`가 있으며, 릴리스 빌드는 Rust 단일 바이너리를 만든다.
- **아키텍처 패턴:** 계층형 모놀리스에 가깝다. `main.rs`가 라우터와 공유 `AppState`를 조립하고, `src/api/*.rs`가 HTTP 핸들러와 상당한 비즈니스 로직을 직접 갖는다. 엄격한 Clean Architecture나 Hexagonal Architecture는 아니다.
- **폴더 구조 분석:**
  - `src/main.rs`: 환경 설정 로드, DB 초기화, 백그라운드 워커 시작, Axum 라우팅 조립.
  - `src/config.rs`: 환경 변수 검증 및 기본값 관리.
  - `src/db.rs`: SQLite 연결, 마이그레이션, 일일 백업 로직.
  - `src/api/auth.rs`, `admin.rs`: 회원가입, 로그인, refresh session, admin user 관리, service token 관리.
  - `src/api/storage.rs`: legacy storage와 S3-like object API. 7,500라인 이상으로 프로젝트에서 가장 큰 모듈이다.
  - `src/api/data.rs`: SQLite-backed logical table/row API, event log, export/import, query preset.
  - `src/api/functions.rs`, `src/functions/mod.rs`: 함수 CRUD, 버전, invocation log, Node 서브프로세스 실행기.
  - `src/push/*`: ntfy/Web Push 큐와 백그라운드 delivery worker.
  - `src/middleware/*`: bearer/service token 인증, S3 SigV4-like 인증, request id, rate limit, auth client policy.
  - `migrations/`: users, auth sessions/events, data API, functions, push queue, service tokens 스키마.
  - `docs/`, `examples/`: 실제 운영/통합 문서와 curl 예제가 비교적 풍부하다.
- **데이터 흐름:** 요청은 `main.rs`의 Axum 라우터로 들어와 request id, rate limit, CORS를 거친다. `/api` 하위 protected route는 bearer JWT 또는 service token 미들웨어를 거치며, S3-like route는 별도 S3 auth 미들웨어에서 bearer, header signature, presigned query를 처리한다. 핸들러는 SQLx로 SQLite를 조회/수정하거나 로컬 스토리지 계층을 호출한 뒤 JSON/XML/바이너리 응답을 반환한다. push와 backup은 `tokio::spawn` 백그라운드 루프로 동작한다.

## 3. 핵심 기능 및 도메인 로직 (Key Features)

- **도메인 모델:**
  - `users`: 이메일, Argon2 password hash, active/admin 상태.
  - `refresh_tokens`, `password_reset_tokens`, `auth_events`: 세션 추적, 회전, 감사 이벤트.
  - `service_tokens`: admin 자동화를 위한 opaque bearer token. 원문은 1회만 반환하고 DB에는 hash 저장.
  - `data_tables`, `data_rows`, `data_row_events`, `data_query_presets`: admin-defined logical table, JSON row, 이벤트 로그, 저장 쿼리.
  - `functions`, `function_versions`, `function_version_secrets`, `function_invocations`: API 함수 정의, 버전, secret, 실행 이력.
  - `push_subscriptions`, `push_queue`: ntfy/Web Push 구독과 delivery queue.
  - 로컬 스토리지 메타데이터: object metadata, multipart upload staging, S3-like ETag/headers.

- **Auth/Admin:** 첫 가입자를 active admin으로 만들고 이후 가입자는 admin approval을 요구한다. JWT access token과 서버 추적 refresh token을 분리했고, 사용자 비활성화 시 protected request마다 DB를 재확인한다. self-host 프로젝트 기준으로 인증 기본기는 단단한 편이다.

- **Storage/S3-like API:** legacy `/api/storage/*`와 `/api/s3/:bucket/*key`를 모두 제공한다. S3-like 계층은 multipart, CopyObject/CopyPart, conditional read, single range, metadata, tagging, checksum, ListObjectsV2 일부를 지원한다. AWS S3 완전 호환은 아니며 "실용적 부분 호환"에 가깝다.

- **Data API:** admin이 JSON schema와 access policy를 가진 logical table을 정의하고 row CRUD를 제공한다. raw SQL을 노출하지 않고 bounded filter/search/sort/pagination만 제공한다. owner-private, authenticated shared, admin-only 정책이 있다. 이벤트 로그, SSE stream, export/import, checksum 검증까지 포함해 작은 앱/운영 도구에는 충분한 표면적이다.

- **Push:** SQLite queue 기반으로 ntfy와 Web Push를 delivery backend로 사용한다. 부분 성공, terminal failure, retry backoff, dead subscription pruning, queue summary/stats가 구현돼 있어 MVP 치고 운영 관측성이 괜찮다.

- **Functions:** admin-managed JS/TS function을 SQLite에 저장하고 Node 서브프로세스로 실행한다. 버전/rollback, async invocation, retry, invocation logs, bounded host binding(storage/push/data)을 제공한다. 다만 런타임 격리는 Node 플래그와 임시 디렉터리 수준으로 보이며, 강한 보안 샌드박스로 보기에는 부족하다.

## 4. 시장 경쟁력 비교 (vs Alternatives)

| 구분 | 이 프로젝트 | PocketBase | Supabase/Firebase | 비교 우위/열위 |
| :--- | :--- | :--- | :--- | :--- |
| 배포 복잡도 | Rust 단일 바이너리 + SQLite + 로컬 파일 | Go 단일 바이너리 + SQLite | 다수 서비스 또는 외부 SaaS | Peanut은 운영 부담이 매우 낮다. Supabase/Firebase보다 훨씬 단순하다. |
| 관리자 UI | 현재 API-first landing page 중심, 새 console은 계획 단계 | 강력한 내장 Admin UI | 웹 콘솔 제공 | Peanut의 가장 큰 제품 약점. |
| 데이터 API | JSON schema 기반 logical table, bounded query | 컬렉션/레코드 모델과 UI | Postgres/Firestore 기반 풍부한 쿼리 | Peanut은 안전하고 단순하지만 기능/질의력은 제한적이다. |
| 파일 스토리지 | 로컬 FS + S3-like 일부 호환 | 파일 업로드 기능 | S3/GCS 수준 managed storage | self-host 단순성은 강점, 분산/내구성/호환성은 열위. |
| Auth | JWT, refresh token, admin approval, events | 내장 auth와 admin UI | 매우 성숙한 auth 생태계 | 기본기는 갖췄지만 OAuth/social login/메일 delivery는 부족하다. |
| Functions | JS/TS subprocess MVP + host binding | hooks/rules 중심 | Edge/Cloud Functions 생태계 | Peanut은 로컬 확장성은 좋지만 샌드박스와 패키지 생태계가 약하다. |
| 운영 관측성 | request id, health/ready, queue stats, auth events | 기본 admin UX | managed observability 연동 | 작은 배포에는 충분하나 metrics/tracing/exporter는 부족하다. |
| 확장성 | 단일 노드 SQLite 중심 | 단일 노드 SQLite 중심 | managed horizontal scale | 의도적으로 소규모 self-host에 맞춰져 있다. |

- **장점 (Pros):**
  - Rust 단일 바이너리와 SQLite 조합으로 배포/운영 난이도가 낮다.
  - README와 docs가 제품 경계를 솔직하게 설명하며, API 예제와 runbook이 실용적이다.
  - 테스트가 소스 내부에 상당히 많고, 특히 storage edge case coverage가 넓다.
  - 인증, service token, request id, structured error, readiness 같은 운영 기본기가 있다.
  - S3-like storage, Data API, Functions, Push를 하나의 작은 런타임에 묶은 점은 작은 프로젝트에 매력적이다.

- **단점 (Cons):**
  - 내장 운영 콘솔이 제거된 상태라 제품 사용성은 API/curl/문서 의존도가 높다.
  - `src/api/storage.rs`, `src/api/data.rs`, `src/api/functions.rs`가 매우 커서 유지보수 비용이 빠르게 증가할 수 있다.
  - Functions sandbox는 강한 격리 모델이 아니다. 악성 또는 실수성 코드 실행을 방어하는 제품으로 포지셔닝하면 위험하다.
  - SQLite/로컬 파일시스템 기반이라 멀티 노드, 고가용성, 대용량 파일 내구성 요구에는 맞지 않는다.
  - S3 호환성은 "partial+"이며, 특정 SDK/클라이언트와의 완전 호환을 기대하면 깨질 수 있다.

## 5. 코드 품질 및 리스크 (Quality & Risk)

- **코드 스타일:** Rust 코드 스타일은 대체로 직선적이고 읽을 수 있다. 복잡한 도메인도 과도한 추상화 없이 구현되어 onboarding은 빠르다. 반면 API 핸들러 파일들이 비대해져 함수 단위 추적은 가능하지만 모듈 경계가 약하다.
- **테스트 커버리지:** `#[test]`, `#[tokio::test]`가 auth, admin, data, storage, push, functions, middleware, db에 넓게 존재한다. 전용 `tests/` 디렉터리보다 모듈 내부 테스트 중심이다. storage는 특히 많은 edge case 테스트를 보유한다.
- **문서 품질:** README, 한국어 README, auth/data/service-token/storage 문서, examples가 잘 정리되어 있다. 다만 CHANGELOG에는 "embedded console" 관련 오래된 표현이 남아 있어 현재 API-first 상태와 일부 불일치한다.
- **보안 리스크:**
  - Functions는 Node subprocess를 실행하므로 엄격한 OS sandbox, 권한 격리, 네트워크 차단, 파일시스템 제한 없이는 신뢰된 admin 코드 전용으로 봐야 한다.
  - global rate limit은 IP 기준 in-memory 100/min 수준이라 reverse proxy 환경에서 `x-forwarded-for` 신뢰 정책이 필요하다.
  - password reset `inline` delivery는 self-host/dev에는 편하지만 운영 기본값으로는 노출 리스크가 있다.
  - service token은 admin-only 고권한 모델이라 fine-grained scope가 없다.
- **운영 리스크:**
  - 백업은 24시간 sleep 이후 처음 실행되므로 프로세스 시작 직후 자동 백업은 없다.
  - 백그라운드 push worker와 backup worker가 같은 프로세스에 묶여 있어 장애 분리성이 낮다.
  - SQLite와 로컬 FS는 단일 머신 운영에는 좋지만, 컨테이너/볼륨 백업 정책이 명확히 필요하다.
- **유지보수 난이도:** 중. 현재는 코드가 명시적이라 따라가기 쉽지만, storage/data/functions의 파일 크기와 정책 분기가 늘어나면 high로 올라갈 가능성이 크다.

## 6. 종합 의견 (Conclusion)

- **추천 여부:** 소규모 self-host 앱 백엔드와 내부 운영 자동화 용도로는 추천. 운영 콘솔, 강한 함수 샌드박스, 대규모 멀티테넌시, 완전한 S3 호환성을 기대하는 도입은 보류.
- **한 줄 평:** Peanut은 "작고 솔직한 self-host BaaS 코어"로는 방향이 좋지만, Functions 격리와 API 모듈 비대화를 관리하지 않으면 제품 신뢰성이 먼저 흔들릴 수 있다.
- **우선 개선 제안:**
  1. `storage`, `data`, `functions`를 protocol/parser/service/persistence 단위로 분리해 파일 크기와 변경 충돌을 줄인다.
  2. Functions를 "trusted admin extension"으로 명확히 문서화하거나, 별도 사용자/컨테이너/네트워크 정책을 둔 진짜 sandbox로 격상한다.
  3. API-first 상태라면 OpenAPI 스펙이나 generated client를 제공해 console 부재를 보완한다.
  4. CHANGELOG와 README의 console 관련 표현을 현재 상태에 맞게 정리한다.
  5. 운영용 기본값에서 password reset `inline` 사용 위험과 reverse proxy `x-forwarded-for` 신뢰 조건을 더 명시한다.
