# Peanut

Peanut은 Rust 단일 바이너리 안에 API 서버와 운영 콘솔을 함께 담아 배포하는 작은 self-host 백엔드 런타임이다.

핵심 방향은 명확하다.
- SQLite 기반 영속성
- 로컬 파일시스템 스토리지
- JWT 인증 + admin 승인 흐름
- 내장된 Next.js 콘솔
- ntfy 기반 push queue MVP

Peanut은 거대한 범용 플랫폼이 아니라, 작고 이해 가능하며 운영 복잡도가 낮은 백엔드 코어를 목표로 한다.

## 제품 철학

Peanut은 아래 원칙을 지향한다.

1. 단일 바이너리 배포
   - Rust 서버가 API와 콘솔을 함께 서빙한다
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
  - bearer token과 만료 시각을 포함한 JSON 응답 반환
- `GET /api/me`
  - 현재 인증된 유저 정보 JSON 반환
- `GET /api/admin/users`
  - admin 전용 유저 목록
- `PUT /api/admin/users/:user_id/activate`
  - admin 전용 승인 처리

### Storage
- 유저 단위로 격리된 object storage
- 인증된 유저는 자신의 키만:
  - 조회
  - 업로드
  - 읽기
  - 삭제
가능하다

### Push (현재 릴리스 MVP)
현재 Peanut은 ntfy 기반 push MVP를 제공한다.

- `GET /api/push/subscriptions`
- `POST /api/push/subscriptions`
- `DELETE /api/push/subscriptions/:subscription_id`
- `POST /api/push/messages`
- `GET /api/push/queue`

의미하는 것:
- 유저가 ntfy topic을 구독할 수 있다
- push 메시지가 SQLite queue에 쌓인다
- 백그라운드 워커가 ntfy topic으로 전달한다
- queue 상태, retry 횟수, 마지막 에러를 API/콘솔에서 볼 수 있다

아직 아닌 것:
- 완전한 Web Push / VAPID 지원은 현재 릴리스 범위에 포함되지 않는다
- `src/push/webpush.rs`는 향후 작업을 위한 placeholder다

### Console
내장 콘솔에서 가능한 것:
- health 확인
- register / login / session 확인
- admin 승인 처리
- user-scoped storage 관리
- ntfy topic 구독 관리
- push queue 확인

콘솔은 Next.js 정적 export 결과물을 Rust 바이너리에 임베드해 함께 배포한다.

## API 요약

### `GET /api/health`

```json
{
  "status": "ok",
  "message": "Systems are operational."
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
  "access_token": "jwt",
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

### `POST /api/push/subscriptions`

```json
{
  "topic": "alerts_main"
}
```

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
├── locales/
└── peanut-console/
```

## 환경변수

필수:
- `JWT_SECRET`

선택:
- `DATABASE_URL` (기본값: `sqlite://peanut.db`)
- `STORAGE_DIR` (기본값: `data/storage`)
- `BIND_ADDR` (기본값: `127.0.0.1:3000`)
- `MAX_UPLOAD_BYTES` (기본값: `5242880`)
- `RUST_LOG` (기본값: `info`)

시작점은 `.env.example` 참고.

## 로컬 개발

### 필요 조건
- Rust toolchain
- Node.js + npm

### 테스트

```bash
cargo test
```

### 콘솔만 빌드

```bash
cd peanut-console
npm install
npm run lint
npm run build
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

## 릴리스 전 체크리스트

```bash
cargo test
cd peanut-console && npm run lint && npm run build && cd ..
./scripts/build.sh
```

수동 확인:
1. 콘솔 열기
2. 첫 admin 유저 등록
3. 로그인
4. 두 번째 유저 승인
5. storage 업로드/읽기/삭제
6. ntfy topic 구독 후 push queue 메시지 전송

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
