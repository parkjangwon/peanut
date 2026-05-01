# Peanut 시작하기

Peanut은 관리자 콘솔이 내장된 Rust 단일 바이너리로 배포됩니다. 첫 운영자는
인스턴스 관리자를 만들고, workspace 설정 초대를 발급해 내부 팀이 격리된
workspace를 만들 수 있게 합니다.

## 부트스트랩

```bash
curl -s -X POST "$BASE_URL/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

콘솔 `/`에 접속해 로그인한 뒤 언어 전환으로 English 또는 한국어를 선택할 수
있습니다. 선택값은 브라우저 local storage에 저장됩니다.

## Workspace 초대

```bash
curl -s -X POST "$BASE_URL/api/admin/workspace-invites" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"label":"mobile team setup","max_uses":1}'
```

응답의 `invite_code`를 workspace owner에게 전달합니다. owner는
`POST /api/workspace-invites/accept`로 초대를 수락할 수 있습니다.

## 앱 만들기

콘솔을 사용하거나 다음 API를 호출합니다.

```bash
curl -s -X POST "$BASE_URL/api/apps" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"workspace_id":"default","name":"mobile-prod","display_name":"Mobile Prod"}'
```

앱 생성은 workspace의 `apps` 리소스 제한을 사용합니다.
