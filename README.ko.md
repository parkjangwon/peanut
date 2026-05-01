# Peanut

Peanut은 Rust 서비스 하나로 배포되는 self-hosted 단일 바이너리 BaaS입니다.
workspace와 app 격리를 중심으로 설계되어, 각 app은 auth user namespace,
data table, storage bucket, function, push state, key, activity feed를 독립적으로 가집니다.

Peanut의 운영 모델은 작고 명확합니다.

- SQLite 영속성
- 로컬 파일시스템 object storage
- 서버 추적 refresh token을 포함한 JWT auth
- 명시적 scope를 가진 app-scoped API key
- workspace setup invite와 membership
- self-hosted resource limit과 usage counter
- owner/developer/operator/viewer admin role
- app-scoped Data, Storage, Push, Functions API
- Rust 바이너리에 내장되어 서빙되는 Next.js admin console
- Auth, Data, Storage, Functions, Push, Activity, Operations 콘솔
- 영어/한국어 콘솔 locale 전환
- single-node 운영 런북과 diagnostics

## API 형태

외부 애플리케이션 API는 app-scoped 경로만 사용합니다.

- Auth: `/api/apps/:app_id/auth/...`
- Data: `/api/apps/:app_id/data/...`
- Storage: `/api/apps/:app_id/storage/...`
- Push: `/api/apps/:app_id/push/...`
- Functions: `/api/apps/:app_id/functions/...`

애플리케이션 호출에는 `X-Peanut-Api-Key`가 필요합니다. 사용자 보호 호출에는
`Authorization: Bearer <access_token>`도 필요합니다. JWT에는 `app_id`가 포함되며,
Peanut은 다른 app 경로에 사용된 bearer token을 거절합니다.

Peanut은 pre-public 상태이므로 legacy global compatibility route를 공개 runtime
API surface로 제공하지 않습니다.

## 빠른 시작

로컬 개발에서는 내장 콘솔을 빌드한 뒤 Rust 서버를 실행합니다.

```bash
cd console && npm install && npm run build && cd ..
JWT_SECRET="$(openssl rand -hex 32)" cargo run
```

그 다음 `http://127.0.0.1:3000`을 열고 첫 platform admin을 만듭니다.

Docker Compose에서는 `.env`에 `JWT_SECRET`을 만든 뒤 실행합니다.

```bash
docker compose up -d
```

새 배포를 신뢰하기 전 production gate를 실행합니다.

```bash
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

## 주요 문서

- `docs/openapi.yaml`
- `docs/local-development.ko.md`
- `docs/docker-compose-deployment.ko.md`
- `docs/app-scoped-api.ko.md`
- `docs/getting-started.ko.md`
- `docs/resource-limits.ko.md`
- `docs/auth-client.ko.md`
- `docs/data-api.ko.md`
- `docs/production-ops-runbook.ko.md`
- `docs/migration-backup-guide.md`

