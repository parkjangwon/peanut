# Peanut 운영 런북

현재 기본 운영 모델은 SQLite와 로컬 스토리지를 사용하는 single-node 배포입니다.

## 배포 전 확인

- `GET /api/ready`가 ready인지 확인합니다.
- `GET /api/admin/ops/diagnostics`가 `ok: true`인지 확인합니다.
- `POST /api/admin/backups`로 SQLite 백업을 만듭니다.
- `GET /api/admin/backups/restore-pending`으로 대기 중인 복구가 없는지 확인합니다.

백업 다운로드와 복구 예약은 platform `owner` 역할만 수행해야 합니다.

## 공개 베타 운영

공개 베타는 invite-only로 운영합니다.

1. `POST /api/admin/beta-invites`로 베타 초대를 만듭니다.
2. 파일럿 owner가 `POST /api/beta/signup`으로 조직을 만듭니다.
3. `GET /api/orgs`에서 조직이 보이는지 확인합니다.
4. `GET /api/orgs/:org_id/usage`에서 `beta_free` 플랜과 쿼터를 확인합니다.
5. 필요한 경우에만 `POST /api/orgs/:org_id/quotas`로 특정 쿼터를 조정합니다.

`quota_exceeded`가 발생하면 모든 제한을 넓히기보다 응답의 `quota_key`,
`used`, `limit`을 보고 필요한 쿼터만 조정합니다.

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
