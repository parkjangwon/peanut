# Peanut service token 가이드

Peanut은 운영 자동화를 위한 좁은 범위의 server-to-server token 모델을 지원한다.

현재 범위:
- admin이 생성/관리하는 opaque token
- 생성 시 1회만 plaintext token 반환
- SQLite에는 hash만 저장
- 기존 protected API에 Bearer auth로 사용
- 현재 access mode는 admin-only 고정

의도적으로 하지 않는 것:
- OAuth client credentials
- 동적인 scope 매트릭스
- 앱별 복잡한 secret 관리
- end-user impersonation

## 엔드포인트

Admin 엔드포인트:
- `GET /api/admin/service-tokens`
- `POST /api/admin/service-tokens`
- `DELETE /api/admin/service-tokens/:token_id`
- curl 예제: `examples/service-tokens/`
- jq 보조 예제: `examples/service-tokens/create-token-jq.sh`
- 통합 운영 예제: `examples/operations-e2e/`

## 토큰 생성

요청:

```json
{
  "name": "deploy-worker",
  "expires_in_days": 30
}
```

응답:

```json
{
  "service_token": {
    "id": "uuid",
    "name": "deploy-worker",
    "access_mode": "admin",
    "user_id": "uuid",
    "created_at": "2026-04-27 15:00:00",
    "last_used_at": null,
    "expires_at": "2026-05-27 15:00:00",
    "revoked_at": null
  },
  "token": "pst_..."
}
```

중요:
- plaintext `token` 값은 바로 복사해둬야 한다
- Peanut은 hash만 저장하므로 raw token은 나중에 다시 볼 수 없다
- 바로 실행 가능한 curl 파일은 `examples/service-tokens/` 참고
- `jq` 가 있으면 `examples/service-tokens/create-token-jq.sh` 가 바로 붙여넣을 export 라인을 출력한다

## 토큰 사용

기존 protected API에 일반 bearer token처럼 사용하면 된다:

```bash
curl -s "$BASE_URL/api/admin/users" \
  -H "authorization: Bearer pst_..."
```

같은 토큰으로 Data API admin 작업 같은 다른 protected admin route도 호출할 수 있다.

## 조회와 revoke

토큰 목록 조회:

```bash
curl -s "$BASE_URL/api/admin/service-tokens" \
  -H "authorization: Bearer $ADMIN_JWT"
```

토큰 revoke:

```bash
curl -s -X DELETE "$BASE_URL/api/admin/service-tokens/$TOKEN_ID" \
  -H "authorization: Bearer $ADMIN_JWT"
```

## 현재 규칙

- service token 생성/조회/revoke는 admin만 가능
- 현재 token은 `access_mode=admin` 으로 고정된다
- revoke된 token은 즉시 동작이 멈춘다
- 만료 시간이 지나면 자동으로 막힌다
- 성공적으로 사용되면 `last_used_at` 이 갱신된다

## 실전 사용처

잘 맞는 용도:
- deploy hook
- backup/export worker
- 내부 admin 자동화
- Data API나 storage admin 작업이 필요한 cron job

아직 잘 맞지 않는 용도:
- 고객용 third-party app auth
- multi-tenant app client 관리
- route별 세밀한 권한 제어
