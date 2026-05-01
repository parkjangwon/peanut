# Peanut

<img width="1376" height="768" alt="image" src="https://github.com/user-attachments/assets/3e934040-791d-4552-8385-66e7e4ffaf30" />

Peanut은 Rust 단일 바이너리로 배포되는 self-hosted BaaS입니다. 기본값으로
SQLite와 로컬 파일시스템 스토리지를 사용하며, 백엔드 API와 내장 admin console을
같은 프로세스에서 서빙합니다.

Peanut은 SaaS가 아닙니다. 직접 운영할 수 있는 작고 살펴보기 쉬운 백엔드 플랫폼을
원하는 팀을 위한 제품이며, 사내 여러 팀이나 여러 프로젝트가 함께 쓰는 경우를 위해
workspace와 app 격리는 유지합니다.

## 제공 기능

- app-scoped Auth와 app별로 격리된 user namespace
- app-scoped Data table과 row
- app-scoped Storage bucket과 object
- Deno 기반 app-scoped Functions
- app-scoped Push subscription, queue, diagnostics
- client/server/admin scope를 가진 API key
- workspace setup invite, membership, resource limit, usage counter
- owner/developer/operator/viewer platform admin role
- backup, restore-pending, readiness, diagnostics, ops metrics
- 영어/한국어 locale을 지원하는 내장 Next.js admin console
- self-hosted release 검증용 Docker Compose production gate

## API 형태

외부 애플리케이션 API는 app-scoped 경로만 사용합니다.

- Auth: `/api/apps/:app_id/auth/...`
- Data: `/api/apps/:app_id/data/...`
- Storage: `/api/apps/:app_id/storage/...`
- Push: `/api/apps/:app_id/push/...`
- Functions 관리: `/api/apps/:app_id/functions/...`
- Function invoke: `/api/apps/:app_id/function-endpoints/:endpoint_slug`

애플리케이션 호출에는 `X-Peanut-Api-Key`가 필요합니다. 사용자 보호 호출에는
`Authorization: Bearer <access_token>`도 필요합니다. JWT에는 `app_id`가 포함되며,
Peanut은 다른 app 경로에 사용된 bearer token을 거절합니다.

legacy global application route는 runtime API surface에 포함하지 않습니다.

## 로컬 개발

내장 콘솔을 빌드한 뒤 Rust 서비스를 실행합니다.

```bash
cd console
npm install
npm run build
cd ..

export JWT_SECRET="$(openssl rand -hex 32)"
cargo run
```

`http://127.0.0.1:3000`을 열고 첫 platform admin을 만듭니다.

## Docker Compose

`docker-compose.yml` 옆에 `.env`를 만듭니다.

```env
JWT_SECRET=replace-with-a-long-random-secret
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret
```

Peanut을 시작합니다.

```bash
docker compose up -d
```

`http://127.0.0.1:3000`을 열고 platform admin을 만들거나 로그인한 뒤, 배포를 신뢰하기 전에 production gate를 실행합니다.

```bash
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

로컬에서 빌드한 이미지를 같은 gate로 검증하려면 다음처럼 실행합니다.

```bash
COMPOSE_FILES="docker-compose.yml docker-compose.build.yml" \
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
JWT_SECRET="$(openssl rand -hex 32)" \
FUNCTIONS_SECRETS_MASTER_KEY="$(openssl rand -hex 32)" \
scripts/verify-compose.sh
```

## Production Gate

`scripts/verify-compose.sh`는 self-hosted release acceptance gate입니다. readiness,
Deno, workspace setup, app A/B 격리, app별 같은 이메일 auth, Data/Storage/Functions
cross-app 거절, disabled app 차단/재활성화, Data CRUD, Storage CRUD, Function
lint/create/invoke, Push diagnostics/test message, backup download, restore 예약,
restore marker clear, clear 이후 readiness를 확인합니다.

## 주요 문서

- `docs/openapi.yaml`
- `docs/local-development.md`
- `docs/local-development.ko.md`
- `docs/docker-compose-deployment.md`
- `docs/docker-compose-deployment.ko.md`
- `docs/app-scoped-api.md`
- `docs/app-scoped-api.ko.md`
- `docs/getting-started.md`
- `docs/getting-started.ko.md`
- `docs/resource-limits.md`
- `docs/resource-limits.ko.md`
- `docs/auth-client.md`
- `docs/auth-client.ko.md`
- `docs/data-api.md`
- `docs/data-api.ko.md`
- `docs/production-ops-runbook.md`
- `docs/production-ops-runbook.ko.md`
- `docs/migration-backup-guide.md`
