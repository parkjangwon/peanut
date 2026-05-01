# Peanut 시작하기

Peanut은 관리자 콘솔이 내장된 Rust 단일 바이너리입니다. 첫 운영자는 플랫폼 관리자를 만들고, 베타 초대를 발급한 뒤 초대받은 팀이 조직을 만들도록 운영합니다.

## 부트스트랩

```bash
curl -s -X POST "$BASE_URL/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

`/`에서 콘솔을 열고 로그인하세요. 콘솔 헤더의 언어 선택기로 영어와 한국어를 전환할 수 있으며, 선택값은 브라우저 localStorage에 저장됩니다.

## 베타 조직 초대

```bash
curl -s -X POST "$BASE_URL/api/admin/beta-invites" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"label":"pilot","max_uses":1}'
```

응답의 `invite_code`를 조직 owner에게 전달합니다. owner는 `POST /api/beta/signup`으로 조직을 만들 수 있습니다.

## 앱 만들기

콘솔을 사용하거나 다음 API를 호출합니다.

```bash
curl -s -X POST "$BASE_URL/api/apps" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"organization_id":"default","name":"mobile-prod","display_name":"Mobile Prod"}'
```

앱 생성은 조직의 `apps` 쿼터를 사용합니다.
