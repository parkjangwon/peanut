# Peanut Docker Compose 배포 가이드

이 문서는 SQLite와 로컬 파일시스템 스토리지를 사용하는 single-node Docker Compose 배포 방법을 설명합니다. Peanut의 기본 production-minimum 경로입니다.

## Compose 파일

배포된 이미지를 사용:

```bash
docker compose up -d
```

현재 checkout에서 이미지를 직접 빌드:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

기본 서비스 이름은 `peanut`이고, `3000` 포트를 노출하며, 상태는 `./data` 아래에 저장합니다.

## 필수 `.env`

`docker-compose.yml` 옆에 `.env`를 만듭니다.

```env
PEANUT_IMAGE=ghcr.io/parkjangwon/peanut:latest
JWT_SECRET=replace-with-a-long-random-secret
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret
DATABASE_URL=sqlite://data/peanut.db
STORAGE_DIR=data/storage
BIND_ADDR=0.0.0.0:3000
MAX_UPLOAD_BYTES=5242880
FUNCTIONS_ENABLED=true
FUNCTIONS_ALLOW_NETWORK=false
FUNCTIONS_MAX_CONCURRENT=4
FUNCTIONS_MEMORY_MB=128
FUNCTIONS_MAX_SOURCE_BYTES=262144
FUNCTIONS_MAX_OUTPUT_BYTES=65536
FUNCTIONS_WORK_DIR=/tmp/peanut-functions
BACKUP_ON_STARTUP=false
TRUST_PROXY_HEADERS=false
MULTIPART_STALE_HOURS=24
MULTIPART_CLEANUP_INTERVAL_SECONDS=3600
RUST_LOG=info
```

시크릿 생성:

```bash
openssl rand -hex 32
```

`JWT_SECRET`을 바꾸면 모든 기존 세션이 무효화됩니다. Function secret을 사용한다면 `FUNCTIONS_SECRETS_MASTER_KEY`도 안정적으로 유지해야 합니다.

## 시작과 확인

```bash
mkdir -p data
docker compose up -d
docker compose ps
docker compose logs -f peanut
```

readiness 확인:

```bash
curl -fsS http://127.0.0.1:3000/api/ready
```

브라우저:

```text
http://127.0.0.1:3000
```

## 첫 관리자

콘솔의 setup flow를 사용하거나 API로 bootstrap합니다.

```bash
curl -s -X POST "http://127.0.0.1:3000/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

초기 자동화를 위해 반환된 admin token을 저장합니다.

```bash
ADMIN_TOKEN="..."
```

## Compose 스모크 검증

새 설치에서 bootstrap credential로 검증:

```bash
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
scripts/verify-compose.sh
```

이미 설치된 환경에서 admin token으로 검증:

```bash
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

CI와 같은 로컬 빌드 이미지로 검증:

```bash
COMPOSE_FILES="docker-compose.yml docker-compose.build.yml" \
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
scripts/verify-compose.sh
```

검증 스크립트는 self-hosted 배포의 release acceptance gate입니다. readiness, Deno, workspace invite setup, app A/B 격리, 앱별 같은 이메일 auth, Data/Storage/Functions cross-app 거절, disabled app 차단/재활성화, Data CRUD, Storage CRUD, Function lint/create/invoke, Push diagnostics/test message, backup download, restore 예약, restore marker clear, clear 이후 readiness를 확인합니다.

## Reverse Proxy 주의사항

공개 트래픽에는 TLS reverse proxy를 앞에 둡니다. 다음 설정은 Peanut이 신뢰할 수 있는 proxy 뒤에서만 접근 가능할 때만 켭니다.

```env
TRUST_PROXY_HEADERS=true
```

직접 노출하거나 proxy chain을 확신할 수 없으면 `false`로 둡니다.

## 업그레이드

업그레이드 전 백업:

```bash
curl -fsS -X POST "$BASE_URL/api/admin/backups" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
tar -czf peanut-data-$(date +%Y%m%d-%H%M%S).tgz data
```

대기 중인 restore가 없는지 확인:

```bash
curl -fsS "$BASE_URL/api/admin/backups/restore-pending" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

업그레이드:

```bash
docker compose pull
docker compose up -d
docker compose logs --tail=200 peanut
curl -fsS "$BASE_URL/api/ready"
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

로컬 빌드 이미지를 사용할 때:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

## 복구와 롤백

SQLite 백업 복구 예약:

```bash
curl -fsS -X POST "$BASE_URL/api/admin/backups/<backup>.backup/restore" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"confirmation":"<backup>.backup","reason":"rollback"}'
```

재시작하면 Peanut이 restore marker를 적용합니다.

```bash
docker compose restart peanut
curl -fsS "$BASE_URL/api/ready"
```

파일시스템 전체 롤백은 서비스를 중지한 뒤 known-good archive에서 `./data` 전체를 복원합니다.

## 중지

데이터를 유지하고 서비스만 중지:

```bash
docker compose down
```

운영 데이터가 아니라는 확신이 있을 때만 로컬 데이터를 삭제합니다.

```bash
docker compose down
rm -rf data
```
