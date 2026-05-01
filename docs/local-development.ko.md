# Peanut 로컬 개발 가이드

이 문서는 Peanut을 로컬 머신에서 직접 실행하는 방법을 설명합니다. 백엔드 API 개발, 콘솔 개발, Docker 이미지 빌드 전 로컬 검증에 사용합니다.

## 요구사항

- Rust stable toolchain
- 내장 콘솔 빌드를 위한 Node.js와 npm
- SQLx SQLite 지원
- `FUNCTIONS_ENABLED=true`로 실행할 경우 Deno

확인:

```bash
cargo --version
node --version
npm --version
deno --version
```

## 환경변수

Docker와 비슷한 조건으로 실행하기 위해 명시적으로 지정합니다.

```bash
export JWT_SECRET="$(openssl rand -hex 32)"
export FUNCTIONS_SECRETS_MASTER_KEY="$(openssl rand -hex 32)"
export DATABASE_URL="sqlite://peanut.dev.db"
export STORAGE_DIR="data/storage"
export BIND_ADDR="127.0.0.1:3000"
export MAX_UPLOAD_BYTES="5242880"
export FUNCTIONS_ENABLED="true"
export FUNCTIONS_ALLOW_NETWORK="false"
export FUNCTIONS_WORK_DIR="/tmp/peanut-functions"
export TRUST_PROXY_HEADERS="false"
export RUST_LOG="info"
```

`JWT_SECRET`을 바꾸면 기존 세션 토큰이 무효화됩니다. `FUNCTIONS_SECRETS_MASTER_KEY`는 Function secret 암호화에 사용되므로 저장된 secret을 계속 읽어야 한다면 안정적인 값을 사용하세요.

## 내장 콘솔 빌드

Peanut은 export된 Next.js 콘솔을 Rust 바이너리에서 함께 서빙합니다. 실제 단일 바이너리 경험을 확인하려면 콘솔을 먼저 빌드합니다.

```bash
cd console
npm install
npm run build
cd ..
```

UI만 빠르게 작업할 때는 별도 dev server를 사용할 수 있습니다.

```bash
cd console
npm run dev
```

다만 최종 확인은 `npm run build` 후 Rust 서버의 `/` 경로에서 해야 합니다.

## 서버 실행

```bash
cargo run
```

브라우저에서 엽니다.

```text
http://127.0.0.1:3000
```

첫 방문에서는 콘솔에서 platform admin을 만들 수 있습니다. API로도 가능합니다.

```bash
curl -s -X POST "http://127.0.0.1:3000/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

이미 관리자가 있으면 bootstrap은 `409`를 반환합니다. 이후에는 콘솔 또는 API로 로그인합니다.

```bash
curl -s -X POST "http://127.0.0.1:3000/api/admin/auth/login" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

## 공개 베타 흐름

platform admin으로 베타 초대를 만듭니다.

```bash
curl -s -X POST "$BASE_URL/api/admin/beta-invites" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"label":"local pilot","max_uses":1}'
```

응답의 `invite_code`로 organization owner를 만듭니다.

```bash
curl -s -X POST "$BASE_URL/api/beta/signup" \
  -H "content-type: application/json" \
  --data '{"invite_code":"pbi_...","organization_name":"Local Pilot","email":"founder@example.com","password":"password123"}'
```

모든 organization은 기본적으로 `beta_free` 플랜을 받습니다. 사용량과 쿼터는 다음처럼 확인합니다.

```bash
curl -s "$BASE_URL/api/orgs/$ORG_ID/usage" \
  -H "authorization: Bearer $ADMIN_TOKEN"
```

## App Key와 SDK 스모크

서버 키를 만듭니다.

```bash
curl -s -X POST "$BASE_URL/api/apps/default/keys" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"name":"local server","key_type":"server"}'
```

앱 사용자를 등록하고 로그인합니다.

```bash
curl -s -X POST "$BASE_URL/api/apps/default/auth/register" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "content-type: application/json" \
  --data '{"email":"user@example.com","password":"password123"}'

curl -s -X POST "$BASE_URL/api/apps/default/auth/login" \
  -H "x-peanut-api-key: $APP_KEY" \
  -H "content-type: application/json" \
  --data '{"email":"user@example.com","password":"password123"}'
```

## 검증

커밋 전 전체 검증:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings

cd console
npm run lint
npm run build
cd ..

bash -n scripts/verify-compose.sh
```

export된 콘솔을 브라우저로 확인하려면:

```bash
python3 -m http.server 4174 -d console/out
```

`http://localhost:4174/index.html`을 열고, 확인 후 `Ctrl-C`로 종료합니다.

## 로컬 상태 초기화

서버를 먼저 종료한 뒤 삭제합니다.

```bash
rm -f peanut.dev.db peanut.dev.db-*
rm -rf data/storage /tmp/peanut-functions
```

운영 `DATABASE_URL`이나 `STORAGE_DIR`에 대해 이 명령을 실행하지 마세요.
