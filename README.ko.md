# Peanut

Peanut은 Rust 백엔드와 임베디드 Next.js 콘솔을 결합한 초기 단계의 싱글 바이너리 백엔드 플랫폼 프로토타입이다.

이 저장소는 다음 구성을 목표로 하고 있다.

- Axum 기반 HTTP API
- SQLx + SQLite 영속성
- Argon2 비밀번호 해시 + JWT 인증
- 영어/한국어 i18n 헬스 체크 응답
- ntfy / Web Push 지향의 백그라운드 푸시 워커
- Next.js로 만든 콘솔 UI를 정적 export 후 Rust 바이너리에 내장

중요: 현재 `master` 브랜치는 아키텍처 방향은 꽤 명확하지만, 아직 빌드 가능한 완성 상태는 아니다.

## Peanut이 지향하는 방향

코드를 보면 Peanut은 “작고 단순하게 self-host 가능한 백엔드 플랫폼”을 지향한다.

핵심 의도는 대략 이렇다.

1. 하나의 서비스로 실행된다
2. SQLite에 사용자/토큰을 저장한다
3. 로컬 파일시스템에 오브젝트를 저장한다
4. 인증된 API를 통해 계정/스토리지 기능을 제공한다
5. 같은 바이너리 안에서 웹 콘솔도 같이 서빙한다
6. 푸시 알림은 백그라운드 워커가 비동기 처리한다

즉, “작은 운영 단위 + 적은 의존성 + 싱글 바이너리 배포”가 프로젝트의 핵심 컨셉으로 보인다.

## 현재 저장소 상태

백엔드/프론트엔드 코드 모두 어느 정도 작성되어 있지만, 아직 연결이 완전히 끝나지 않은 상태다.

### 이미 구현된 것

백엔드:
- i18n 기반 health endpoint
- 회원가입 / 로그인 흐름
- Argon2 비밀번호 해시
- JWT 생성 / 검증
- SQLite 초기화 + migration 실행
- 푸시 큐 폴링용 백그라운드 워커
- 콘솔 정적 파일 임베딩 서버

프론트엔드:
- Next.js App Router 기반 미니멀 다크 대시보드
- 정적 export 설정
- 시스템 / 스토리지 / 푸시 큐 카드형 UI

인프라:
- 멀티 스테이지 Dockerfile
- `docker-compose.yml`
- 프론트엔드 + 백엔드 통합 빌드 스크립트

### 아직 미완성인 부분

실제로 `cargo test`를 돌려 확인했을 때 현재 브랜치는 컴파일되지 않는다.

확인된 주요 문제는 다음과 같다.

1. `storage` 모듈이 없음
   - `src/main.rs`에서 `mod storage;`를 선언함
   - 하지만 `src/storage.rs` 또는 `src/storage/mod.rs`가 없음

2. `api::storage` 라우트 핸들러가 없음
   - `src/main.rs`에서는 `/storage/*key` 라우트를 연결함
   - 그런데 `src/api/mod.rs`에는 `health`, `auth`만 export 되어 있음

3. Rust 빌드 시 필요한 콘솔 export 결과물이 없음
   - `src/console.rs`는 `peanut-console/out/` 폴더를 임베드하도록 되어 있음
   - 현재 저장소에는 그 결과물이 없어서, 프론트 빌드 선행 없이 Rust 컴파일이 실패함

4. Axum state 타입이 맞지 않음
   - auth 핸들러는 `State<SqlitePool>`을 받음
   - 실제 앱은 `AppState`로 초기화됨
   - 그래서 라우터 state 타입 mismatch 컴파일 에러가 발생함

5. 푸시 관련 DB 스키마가 비어 있음
   - migration에는 `users`, `refresh_tokens`만 있음
   - 그런데 푸시 워커는 `push_queue`, `push_subscriptions` 테이블을 기대함

6. 환경변수 wiring이 덜 되어 있음
   - `docker-compose.yml`에는 `DATABASE_URL`이 있음
   - 실제 `src/main.rs`는 `sqlite://peanut.db`를 하드코딩함
   - `dotenvy` dependency는 있지만 startup 경로에서 실제 사용되지 않음

즉, 지금 상태의 Peanut은 “구현 중인 MVP 설계본”에 가깝고, 바로 배포 가능한 릴리스 상태는 아니다.

## 아키텍처 분석

## 1. 서버 부트스트랩과 라우팅

진입점은 `src/main.rs`다.

퍼블릭 라우트:
- `GET /api/health`
- `POST /api/register`
- `POST /api/login`

보호 라우트:
- `GET /api/me`
- 의도상 `/api/storage/*key`

fallback:
- 나머지 경로는 `src/console.rs`가 Next.js 정적 자산을 서빙함

즉 구조 자체는 “API + 콘솔을 한 프로세스에서 같이 제공”하는 형태다.

## 2. 인증 구조

인증 관련 코드는 세 층으로 나뉜다.

- `src/auth/hash.rs`
  - Argon2로 비밀번호 해시 생성
  - 비밀번호 검증

- `src/auth/jwt.rs`
  - 15분 만료 JWT 생성
  - `sub`, `exp`, `is_admin` 클레임 포함

- `src/middleware/auth.rs`
  - `Authorization: Bearer <token>` 파싱
  - JWT 검증
  - 검증된 claims를 request extension에 주입

현재 동작상 특징:
- 첫 가입자는 자동으로 admin + active 처리됨
- 이후 가입자는 inactive 상태로 생성됨
- JWT secret은 현재 `temp_secret` 하드코딩이라 운영 환경용으로는 부족함

즉 “간단한 bootstrap admin 생성”까지는 의도돼 있지만, 운영 보안 설계는 아직 진행 중이다.

## 3. DB 레이어

`src/db.rs`는 SQLite를 다음 설정으로 초기화한다.

- `journal_mode = WAL`
- `synchronous = NORMAL`
- foreign key 활성화
- `./migrations`에 있는 SQLx migration 실행

현재 migration에는 아래만 있다.
- `users`
- `refresh_tokens`

운영 복잡도를 낮추려는 의도는 명확하고, 작은 self-host 환경에는 잘 맞는 선택이다.

## 4. 푸시 알림 설계

푸시 관련 코드는 `src/push/` 아래에 있다.

- `worker.rs`
  - 5초마다 큐를 폴링
- `ntfy.rs`
  - `https://ntfy.sh/<topic>`으로 POST 전송
- `webpush.rs`
  - 실제 Web Push 구현 전 placeholder

코드상 의도는 다음과 같다.

- DB에 푸시 작업을 적재
- 유저별 subscription 조회
- 현재는 ntfy 기반 전달
- 이후 Web Push로 확장
- 성공/실패 상태를 DB에 반영

다만 현재 한계는 분명하다.
- 필요한 테이블 migration이 없음
- `endpoint`를 사실상 ntfy topic처럼 사용하고 있어, 아직은 정식 Web Push 모델이라기보다 프로토타입 설계에 가깝다

## 5. 콘솔 UI

프론트엔드는 `peanut-console/`에 있다.

현재 UI 특징:
- 다크 테마
- 미니멀 대시보드
- 정적인 카드 UI
- 아직 실시간 API 연동 없음
- 정적 export 후 Rust 바이너리 안에 포함하는 구조

이 부분은 Peanut의 “싱글 바이너리 simplicity” 컨셉과 잘 맞는다. 프론트를 별도 서비스로 띄우지 않고 함께 묶겠다는 방향이기 때문이다.

## 디렉터리 구조

```text
.
├── Cargo.toml                  # Rust 앱 의존성 정의
├── Dockerfile                  # 프론트/백엔드 멀티 스테이지 이미지 빌드
├── docker-compose.yml          # 로컬 컨테이너 실행 설정
├── migrations/                 # SQLite 스키마 migration
├── scripts/build.sh            # 콘솔 export + Rust release 빌드
├── src/
│   ├── main.rs                 # 앱 부트스트랩 / 라우팅
│   ├── db.rs                   # SQLite 초기화 / migration 실행
│   ├── console.rs              # 정적 자산 임베딩 서빙
│   ├── i18n.rs                 # 번역 헬퍼
│   ├── api/
│   │   ├── auth.rs             # register / login 핸들러
│   │   ├── health.rs           # 다국어 health endpoint
│   │   └── mod.rs
│   ├── auth/
│   │   ├── hash.rs             # Argon2 헬퍼
│   │   ├── jwt.rs              # JWT 헬퍼
│   │   └── mod.rs
│   ├── middleware/
│   │   ├── auth.rs             # bearer token 미들웨어
│   │   └── mod.rs
│   └── push/
│       ├── worker.rs           # 푸시 큐 처리기
│       ├── ntfy.rs             # ntfy 전송기
│       ├── webpush.rs          # Web Push placeholder
│       └── mod.rs
├── locales/
│   ├── en.json
│   └── ko.json
└── peanut-console/
    ├── src/app/page.tsx        # 미니멀 대시보드 페이지
    ├── src/app/layout.tsx      # 앱 레이아웃
    ├── src/app/globals.css     # 기본 스타일
    └── next.config.mjs         # static export 설정
```

## API 요약

### `GET /api/health`
로케일에 따라 메시지가 달라지는 JSON health 응답을 반환한다.

예시:
```json
{
  "status": "ok",
  "message": "Systems are operational."
}
```

동작:
- `Accept-Language: en-*` -> 영어
- `Accept-Language: ko-*` -> 한국어

### `POST /api/register`
신규 유저를 생성한다.

요청 바디:
```json
{
  "email": "user@example.com",
  "password": "secret"
}
```

현재 로직:
- 첫 유저는 admin + active
- 이후 유저는 inactive

### `POST /api/login`
활성 유저 로그인 후 JWT 토큰을 plain text로 반환한다.

요청 바디:
```json
{
  "email": "user@example.com",
  "password": "secret"
}
```

### `GET /api/me`
보호 라우트이며 JWT claims를 읽어 문자열 응답을 반환한다.

## 빌드 / 실행 방식

## 필요 조건

- Rust toolchain
- Node.js / npm
- 로컬 Linux 빌드 시 SQLite/OpenSSL 개발 라이브러리

### 의도된 빌드 흐름

```bash
./scripts/build.sh
```

이 스크립트는 아래 순서로 동작하도록 작성되어 있다.

1. 프론트엔드 의존성 설치
2. Next.js console export 생성
3. Rust release 빌드

### Docker 실행 흐름

```bash
docker compose up --build
```

### 현재 실제 상태

하지만 저장소 최신 상태에서는 앞서 적은 누락 사항들 때문에 그대로는 빌드가 끝나지 않는다.

## 코드의 장점

지금 단계에서도 분명 장점이 있다.

- Rust + SQLite 조합이 작은 self-host 서비스에 잘 맞음
- 프론트 임베딩 구조가 싱글 바이너리 목표와 일치함
- auth / i18n / push 코드가 나름 역할별로 분리되어 있음
- Dockerfile이 최종 배포 모델을 어느 정도 반영하고 있음
- “작게 시작해서 점진적으로 확장”하기 좋은 구조임

## 우선 보완해야 할 것

이 저장소를 실제 동작하는 MVP로 빠르게 만들려면 우선순위는 아래 순서가 좋다.

1. `src/storage/`와 `src/api/storage.rs` 구현
2. Axum state를 `AppState` 기준으로 통일
3. `DATABASE_URL`, JWT secret, storage path, bind address를 환경변수로 읽도록 수정
4. `push_queue`, `push_subscriptions` migration 추가
5. 프론트 export를 release 빌드 파이프라인에 안정적으로 포함
6. 하드코딩된 `temp_secret` 제거
7. auth 응답을 plain text 대신 JSON 구조로 정리
8. 콘솔 UI를 실제 API 데이터와 연결

## 코드 규모 메모

의존성 폴더를 제외하고 대략적인 현재 애플리케이션 코드 규모는 다음 정도다.

- Rust: 약 517줄 / 16파일
- TS/TSX: 약 73줄 / 3파일
- CSS: 14줄
- SQL: 16줄

`package-lock.json`은 생성 파일이라 실제 앱 복잡도를 대표하지 않는다.

## 기여자 메모

이 저장소를 “설계가 보이는 프로토타입”에서 “실행 가능한 MVP”로 바꾸는 가장 짧은 경로는 아래다.

- storage 레이어 완성
- state 타입 정리
- push 관련 migration 추가
- console export를 빌드에 확실히 연결

이 네 가지가 끝나면 Peanut은 지금보다 훨씬 일관된 싱글 노드 백엔드 스타터가 될 수 있다.

## 라이선스

현재 저장소에는 라이선스 파일이 없다. 공개 배포/재사용을 고려한다면 라이선스 파일을 추가하는 것이 좋다.
