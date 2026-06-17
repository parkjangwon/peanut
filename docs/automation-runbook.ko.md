# Peanut 운영 자동화 런북

이 문서는 Peanut service token을 실제 운영 자동화에 연결하는 용도다.

이런 경우에 쓰면 좋다:
- nightly export
- cron 기반 health/storage 체크
- 브라우저 세션 없이 bounded admin 작업 자동화

같이 보면 좋은 문서:
- `docs/service-tokens.ko.md`
- `examples/service-tokens/`
- `examples/operations-e2e/`
- `examples/automation/`

## 권장 패턴

1. admin으로 한 번 로그인해서 전용 service token을 발급한다
2. automation이 도는 머신의 안전한 env 파일에 plaintext token을 저장한다
3. Peanut protected API를 `Authorization: Bearer <pst_...>` 헤더로 호출한다
4. 자동화 목적이 끝나면 token을 revoke하고 새 것으로 교체한다

## service token 자동화에 잘 맞는 것

- Data API export 작업
- bounded table maintenance 작업
- storage metadata 체크
- 내부 deploy hook
- protected endpoint가 필요한 operator-only health/readiness probe

## 아직 여기에 쓰지 않는 것이 좋은 것

- third-party 고객 앱 auth
- workspace 단위 delegation
- 광범위한 user impersonation
- unbounded SQL 스타일 DB 접근

## env 파일 예시

시작점으로는 커밋된 샘플 파일을 그대로 복사하면 된다:
- `examples/automation/peanut.env.sample`

예시 로컬 파일:

```bash
BASE_URL=http://127.0.0.1:3492
APP_ID=default
SERVICE_TOKEN=pst_replace_me
TABLE_NAME=ops_todos
STORAGE_BUCKET=assets
STORAGE_KEY=ops/hello.txt
AUTOMATION_OUT_DIR=/opt/peanut/backups
```

Docker Compose 기본 패키징이 아니라 `cargo run`으로 직접 띄운 로컬 프로세스를
자동화한다면 `http://127.0.0.1:3000`을 사용합니다.
`APP_ID`에는 자동화가 접근할 Data와 Storage 리소스의 앱 ID를 넣습니다.

## 바로 실행 가능한 automation 예제

커밋된 스크립트:
- `examples/automation/export-ops-todos.sh`
- `examples/automation/check-storage-head.sh`

예시:

```bash
cp examples/automation/peanut.env.sample /opt/peanut/peanut.env
$EDITOR /opt/peanut/peanut.env

PEANUT_ENV_FILE=/opt/peanut/peanut.env \
  ./examples/automation/export-ops-todos.sh

PEANUT_ENV_FILE=/opt/peanut/peanut.env \
  ./examples/automation/check-storage-head.sh
```

## Cron 예시

```cron
15 2 * * * PEANUT_ENV_FILE=/opt/peanut/peanut.env /opt/peanut/examples/automation/export-ops-todos.sh
*/30 * * * * PEANUT_ENV_FILE=/opt/peanut/peanut.env /opt/peanut/examples/automation/check-storage-head.sh
```

## 권장 도입 순서

전체 부트스트랩 흐름은 아래 순서로 보면 된다:
1. `examples/service-tokens/create-token.sh` 또는 `create-token-jq.sh`
2. `examples/operations-e2e/`
3. `examples/automation/peanut.env.sample` 을 머신 로컬 비밀 파일로 복사하고 plaintext token을 넣는다
4. 검증된 시퀀스를 로컬 cron/systemd/CI job으로 옮긴다

## Rotation 가이드

- 자동화 목적마다 token을 분리하는 편이 좋다
- token 이름은 `nightly-export`, `storage-head-check`, `deploy-hook` 처럼 명확하게 짓는다
- 모든 작업에 하나의 token을 재사용하기보다 오래된 token은 revoke한다
- 머신을 폐기하면 관련 token도 즉시 revoke한다
