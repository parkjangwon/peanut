# Peanut 운영 런북

현재 기본 운영 모델은 SQLite와 로컬 스토리지를 사용하는 single-node 배포입니다.

## 배포 전 확인

- `GET /api/ready`가 ready인지 확인합니다.
- `GET /api/admin/ops/diagnostics`가 `ok: true`인지 확인합니다.
- `POST /api/admin/backups`로 SQLite 백업을 만듭니다.
- `GET /api/admin/backups/restore-pending`으로 대기 중인 복구가 없는지 확인합니다.
- 새 이미지 승격 전 `scripts/verify-compose.sh` 전체 gate를 통과시킵니다.

백업 다운로드와 복구 예약은 platform `owner` 역할만 수행해야 합니다.

## Docker Compose 운영 Gate

CI와 같은 로컬 빌드 이미지로 production gate를 실행합니다.

```bash
COMPOSE_FILES="docker-compose.yml docker-compose.build.yml" \
JWT_SECRET=replace-with-a-long-random-secret \
FUNCTIONS_SECRETS_MASTER_KEY=replace-with-a-different-long-random-secret \
PEANUT_BOOTSTRAP_EMAIL=owner@example.com \
PEANUT_BOOTSTRAP_PASSWORD=password123 \
scripts/verify-compose.sh
```

검증 스크립트의 기본 URL은 패키징된 Compose 호스트 포트와 맞춘
`http://127.0.0.1:3492`입니다. `PEANUT_HOST_PORT`를 바꾸거나 reverse proxy를
통해 검증할 때만 `BASE_URL`을 덮어씁니다.

이미 운영 중인 인스턴스에서는 admin token을 사용합니다.

```bash
PEANUT_ADMIN_TOKEN="$ADMIN_TOKEN" scripts/verify-compose.sh
```

`PEANUT_ADMIN_TOKEN`이나 bootstrap credential 없이 실행하면 compose startup, readiness, Deno만 확인합니다. production gate로 인정하려면 둘 중 하나를 반드시 제공해야 합니다.

통과 기준:

- app A/B가 독립된 auth user, key, data, storage, function, push 상태를 갖습니다.
- 같은 이메일은 서로 다른 app에 존재할 수 있고, 같은 app 중복은 거절됩니다.
- app A credential로 app B Data/Storage/Function endpoint 접근이 403으로 거절됩니다.
- app disable 시 SDK 요청이 막히고 enable 후 정상화됩니다.
- backup create/download와 restore-pending schedule/read/clear가 동작합니다.
- restore-pending clear 후 `/api/ready`가 다시 clean ready입니다.

## Workspace 운영

Workspace 설정은 invite-only로 운영합니다.

1. `POST /api/admin/workspace-invites`로 workspace 초대를 만듭니다.
2. workspace owner가 `POST /api/workspace-invites/accept`로 workspace를 만듭니다.
3. `GET /api/workspaces`에서 workspace가 보이는지 확인합니다.
4. `GET /api/workspaces/:workspace_id/resource-usage`에서 `self_hosted_default` 제한 프로필을 확인합니다.
5. 필요한 경우에만 `POST /api/workspaces/:workspace_id/resource-limits`로 특정 리소스 제한을 조정합니다.

`resource_limit_exceeded`가 발생하면 모든 제한을 넓히기보다 응답의
`resource_key`, `used`, `limit`을 보고 필요한 리소스 제한만 조정합니다.

## 콘솔 다국어

내장 콘솔은 영어와 한국어를 지원합니다. 언어 선택은 브라우저 locale로 초기화되고, 사용자가 바꾸면 localStorage에 저장됩니다. 콘솔 변경 후에는 두 언어 모두에서 로그인 화면과 주요 대시보드가 깨지지 않는지 확인합니다.

## 복구

Peanut은 운영 down migration을 사용하지 않습니다. 롤백은 백업 기반입니다.

```bash
curl -fsS -X POST "$BASE_URL/api/admin/backups/<backup>.backup/restore" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"confirmation":"<backup>.backup","reason":"rollback"}'
```

복구 예약 후 서비스를 재시작하고 `/api/ready`를 확인합니다.

## 백업 주기와 복구 Drill

- 배포 전마다 API backup을 만듭니다.
- schema 변경 업그레이드 전에는 `./data` 전체를 archive합니다.
- 최소 월 1회 운영 데이터 사본에서 restore drill을 수행합니다.
- restore drill 후 `scripts/verify-compose.sh`를 통과해야 완료로 봅니다.
- `JWT_SECRET`과 `FUNCTIONS_SECRETS_MASTER_KEY`는 호스트 외부에 보관합니다.
