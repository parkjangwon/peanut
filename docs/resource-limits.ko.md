# Peanut 리소스 제한

셀프호스팅 workspace에는 기본적으로 `self_hosted_default` 제한 프로필이
배정됩니다. 이 제한은 과금 플랜이 아니라 한 Peanut 인스턴스 안에서 안전하게
운영하기 위한 가드레일입니다.

## 기본 제한

- workspace당 앱: 3개
- workspace당 앱 사용자: 10,000명
- workspace당 데이터 row: 250,000개
- workspace당 스토리지: 2GB
- 월 Function 호출: 50,000회
- 월 Push 발송: 50,000회
- 월 API 요청: 1,000,000회

## 적용 방식

쓰기 계열 작업에서 제한을 검사합니다. 현재 첫 번째 가드는 앱 생성입니다.
workspace의 `apps` 제한을 넘으면 Peanut은 다음 응답을 반환합니다.

```json
{
  "code": "resource_limit_exceeded",
  "resource_key": "apps",
  "used": 3,
  "limit": 3
}
```

읽기 작업은 열어두어 운영자가 상태를 확인하고 복구할 수 있게 합니다.

## 조정

인스턴스 관리자는 workspace별로 특정 리소스 제한을 조정할 수 있습니다.

```bash
curl -s -X POST "$BASE_URL/api/workspaces/$WORKSPACE_ID/resource-limits" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"resource_key":"apps","limit":10}'
```
