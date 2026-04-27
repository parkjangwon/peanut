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

## 권장 패턴

1. admin으로 한 번 로그인해서 전용 service token을 발급한다
2. automation이 도는 머신의 안전한 env 파일에 plaintext token을 저장한다
3. Peanut protected API를 `Authorization: Bearer pst_...` 로 호출한다
4. 자동화 목적이 끝나면 token을 revoke하고 새 것으로 교체한다

## service token 자동화에 잘 맞는 것

- Data API export 작업
- bounded table maintenance 작업
- storage metadata 체크
- 내부 deploy hook
- protected endpoint가 필요한 operator-only health/readiness probe

## 아직 여기에 쓰지 않는 것이 좋은 것

- third-party 고객 앱 auth
- tenant 단위 delegation
- 광범위한 user impersonation
- unbounded SQL 스타일 DB 접근

## env 파일 예시

```bash
export BASE_URL=http://127.0.0.1:3000
export SERVICE_TOKEN='pst_...'
```

## Cron 예시: table export snapshot

```bash
#!/usr/bin/env bash
set -euo pipefail

source /opt/peanut/peanut.env
TIMESTAMP=$(date +%F-%H%M%S)
OUT_DIR=/opt/peanut/backups
mkdir -p "$OUT_DIR"

curl -s "$BASE_URL/api/data/tables/ops_todos/export" \
  -H "authorization: Bearer $SERVICE_TOKEN" \
  > "$OUT_DIR/ops_todos-$TIMESTAMP.json"
```

예시 crontab:

```cron
15 2 * * * /opt/peanut/scripts/export-ops-todos.sh
```

## Cron 예시: protected storage HEAD 체크

```bash
#!/usr/bin/env bash
set -euo pipefail

source /opt/peanut/peanut.env
curl -fsSI "$BASE_URL/api/s3/assets/ops/hello.txt" \
  -H "authorization: Bearer $SERVICE_TOKEN"
```

예시 crontab:

```cron
*/30 * * * * /opt/peanut/scripts/check-storage-head.sh
```

## 권장 도입 순서

전체 부트스트랩 흐름은 아래 순서로 보면 된다:
1. `examples/service-tokens/create-token.sh` 또는 `create-token-jq.sh`
2. `examples/operations-e2e/`
3. 검증된 시퀀스를 로컬 cron/systemd/CI job으로 옮긴다

## Rotation 가이드

- 자동화 목적마다 token을 분리하는 편이 좋다
- token 이름은 `nightly-export`, `storage-head-check`, `deploy-hook` 처럼 명확하게 짓는다
- 모든 작업에 하나의 token을 재사용하기보다 오래된 token은 revoke한다
- 머신을 폐기하면 관련 token도 즉시 revoke한다
