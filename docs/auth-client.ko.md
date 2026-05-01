# Peanut Auth 클라이언트 가이드

Peanut Auth는 앱 단위로 격리된다. 클라이언트와 서버 연동은
`/api/apps/:app_id/auth/...` 경로를 사용하고 `X-Peanut-Api-Key`를 보내야
한다. JWT에도 같은 `app_id`가 들어가며, 경로의 앱과 토큰의 앱이 다르면
거절된다.

## 부트스트랩

새 설치에는 아직 관리자 토큰이 없다. 최초 1회만 플랫폼 관리자를 만든다.

```http
POST /api/bootstrap/admin
content-type: application/json

{
  "email": "owner@example.com",
  "password": "password123"
}
```

응답은 일반 로그인 응답과 같다. 이미 관리자가 있으면 이 엔드포인트는
`409`를 반환한다.

## 앱 Auth 경로

- `POST /api/apps/:app_id/auth/register`
- `POST /api/apps/:app_id/auth/login`
- `POST /api/apps/:app_id/auth/refresh`
- `POST /api/apps/:app_id/auth/logout`
- `GET /api/apps/:app_id/auth/me`
- `POST /api/apps/:app_id/auth/change-password`
- `POST /api/apps/:app_id/auth/forgot-password`
- `POST /api/apps/:app_id/auth/reset-password`
- `GET /api/apps/:app_id/auth/sessions`
- `DELETE /api/apps/:app_id/auth/sessions/:session_id`
- `POST /api/apps/:app_id/auth/sessions/revoke-all`
- `GET /api/apps/:app_id/auth/events`
- `GET /api/apps/:app_id/auth/public-config`
- `GET /api/apps/:app_id/auth/oauth/:provider/start`
- `GET /api/apps/:app_id/auth/oauth/:provider/callback`

부트스트랩을 제외한 앱 Auth 경로는 다음 헤더가 필요하다.

```http
X-Peanut-Api-Key: <client-or-server-key>
```

보호 경로는 추가로 다음 헤더가 필요하다.

```http
Authorization: Bearer <access_token>
```

## 로그인 응답

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_at": "2026-05-01T12:00:00Z",
  "user": {
    "id": "uuid",
    "app_id": "default",
    "email": "owner@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

refresh 이후에는 새 refresh token만 저장한다. refresh가 실패하면 로컬
세션을 버리고 다시 로그인시킨다.

## 클라이언트 정책

`AUTH_ALLOWED_ORIGINS` 또는 `AUTH_ALLOWED_CLIENT_IDS`를 설정한 경우 브라우저
Auth 요청은 허용된 `Origin` 및 `x-peanut-client-id`를 보내야 한다.
