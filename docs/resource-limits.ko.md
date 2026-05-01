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

쓰기 계열 작업과 SDK app key 요청에서 제한을 검사합니다. 현재 Peanut은
앱 생성, 앱 사용자 등록, 데이터 row 생성, 스토리지 쓰기, Function 호출,
Push 발송, 월 SDK API 요청을 제한합니다. workspace 제한을 넘으면 Peanut은
다음 응답을 반환합니다.

```json
{
  "code": "resource_limit_exceeded",
  "resource_key": "apps",
  "used": 3,
  "limit": 3,
  "period_start": "all",
  "reset_at": null,
  "source": "count"
}
```

월간 리소스는 달력 월 기준 `period_start`와 `reset_at`을 제공합니다. API
요청 제한 자체가 소진된 경우를 제외하면 읽기 작업은 열어두어 운영자가 상태를
확인하고 복구할 수 있게 합니다.

## 조정

인스턴스 관리자는 workspace별로 특정 리소스 제한을 조정할 수 있습니다.

```bash
curl -s -X POST "$BASE_URL/api/workspaces/$WORKSPACE_ID/resource-limits" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"resource_key":"apps","limit":10}'
```
