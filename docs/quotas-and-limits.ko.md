# Peanut 쿼터와 제한

공개 베타 조직에는 기본적으로 `beta_free` 플랜이 배정됩니다.

## 기본 제한

- 조직당 앱: 3개
- 조직당 앱 사용자: 10,000명
- 조직당 데이터 row: 250,000개
- 조직당 스토리지: 2GB
- 월 Function 호출: 50,000회
- 월 Push 전송: 50,000회
- 월 API 요청: 1,000,000회

## 적용 방식

쿼터는 쓰기 성격의 작업에서 적용합니다. 우선 앱 생성에 `apps` 쿼터가 적용됩니다. 초과 시 Peanut은 다음 형태로 응답합니다.

```json
{
  "code": "quota_exceeded",
  "quota_key": "apps",
  "used": 3,
  "limit": 3
}
```

읽기 요청은 계속 허용해 운영자가 상태를 확인하고 복구할 수 있게 합니다.

## 파일럿 override

플랫폼 관리자는 베타 운영 중 특정 쿼터를 조정할 수 있습니다.

```bash
curl -s -X POST "$BASE_URL/api/orgs/$ORG_ID/quotas" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"quota_key":"apps","limit":10}'
```
