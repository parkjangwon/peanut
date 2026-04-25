# Peanut Auth 클라이언트 가이드

Peanut은 이제 외부 프론트 앱이 붙을 수 있는 auth backend로 사용할 수 있다.

이 문서는 아래를 정리한다.
- 어떤 엔드포인트를 호출해야 하는지
- access token + refresh token을 어떻게 다뤄야 하는지
- 최소 브라우저 클라이언트를 어떻게 붙일지
- logout, password reset, session revoke를 어떻게 처리할지

같이 보면 좋은 문서:
- `README.ko.md`
- `docs/auth-client.md`
- `examples/auth-client-web/`

## 1. 현재 Auth 모델

Peanut이 현재 제공하는 엔드포인트:
- `POST /api/register`
- `POST /api/login`
- `POST /api/auth/refresh`
- `POST /api/auth/logout`
- `GET /api/me`
- `POST /api/auth/change-password`
- `POST /api/auth/forgot-password`
- `POST /api/auth/reset-password`
- `GET /api/auth/sessions`
- `DELETE /api/auth/sessions/:session_id`
- `POST /api/auth/sessions/revoke-all`

현재 동작 방식:
- access token은 짧은 수명의 JWT bearer token이다
- refresh token은 서버가 추적하는 opaque token이다
- `POST /api/auth/refresh` 호출 시 refresh token은 회전한다
- logout, password change, password reset, admin deactivate 시 refresh session이 revoke된다

## 2. 권장 프론트 연동 형태

### 간단한 프로토타입 기본값
- access token은 메모리에 저장
- 로컬 데모나 내부 도구라면 refresh token도 메모리에 저장 가능
- `/api/me` 같은 보호 API가 `401`을 주면 `/api/auth/refresh`를 호출

### 더 나은 프로덕션 형태
- access token은 메모리에만 둔다
- 장수명 refresh token을 `localStorage`에 넣지 않는다
- 가능하면 작은 BFF(backend-for-frontend) 레이어를 두고 refresh token은 secure cookie나 서버 세션에 둔다
- 프론트는 login/refresh/logout을 그 BFF를 통해 호출한다

Peanut은 아직 auth cookie를 직접 내려주지 않는다. 현재 API는 token 중심이므로 브라우저 앱은 아래 중 하나를 택하면 된다.
- Peanut에 직접 붙는 SPA
- Peanut auth를 감싼 BFF 기반 앱

## 3. 핵심 요청 흐름

### 회원가입

```http
POST /api/register
content-type: application/json

{
  "email": "admin@example.com",
  "password": "correct horse battery staple"
}
```

### 로그인

```http
POST /api/login
content-type: application/json

{
  "email": "admin@example.com",
  "password": "correct horse battery staple"
}
```

대표 응답:

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_at": "2026-04-26T12:00:00Z",
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "is_active": true,
    "is_admin": true
  }
}
```

### 보호 API 호출

```http
GET /api/me
authorization: Bearer <access_token>
```

### 세션 refresh

```http
POST /api/auth/refresh
content-type: application/json

{
  "refresh_token": "..."
}
```

중요 포인트:
- refresh가 성공하면 새로 내려온 refresh token으로 즉시 교체해야 한다
- 이전 refresh token은 바로 버려야 한다
- refresh가 `401`로 실패하면 강제로 다시 로그인시키는 게 맞다

### 로그아웃

```http
POST /api/auth/logout
content-type: application/json

{
  "refresh_token": "..."
}
```

### 세션 관리

```http
GET /api/auth/sessions
authorization: Bearer <access_token>
```

```http
DELETE /api/auth/sessions/<session_id>
authorization: Bearer <access_token>
```

```http
POST /api/auth/sessions/revoke-all
authorization: Bearer <access_token>
```

## 4. 비밀번호 흐름

### 비밀번호 변경

유효한 access token이 필요하다.

```http
POST /api/auth/change-password
authorization: Bearer <access_token>
content-type: application/json

{
  "current_password": "old-password",
  "new_password": "new-password-123"
}
```

결과:
- 비밀번호가 변경된다
- 기존 refresh session이 전부 revoke된다
- 현재 앱은 다시 로그인 흐름으로 보내는 게 맞다

### 비밀번호 찾기 / 재설정

```http
POST /api/auth/forgot-password
content-type: application/json

{
  "email": "admin@example.com"
}
```

현재 self-host 우선 동작:
- Peanut은 JSON 응답에 `reset_token`을 바로 반환한다
- self-host / 내부 운영 환경에서 바로 실험 가능한 형태를 우선한 것이다
- 이후 email, webhook, 운영자 전달 흐름으로 감쌀 수 있다

```http
POST /api/auth/reset-password
content-type: application/json

{
  "reset_token": "...",
  "new_password": "new-password-123"
}
```

결과:
- 비밀번호가 재설정된다
- 기존 refresh session이 전부 revoke된다
- 사용자는 다시 로그인해야 한다

## 5. 최소 브라우저 클라이언트 패턴

`examples/auth-client-web/app.js`는 아래 형태를 쓴다.

```js
const authState = {
  accessToken: null,
  refreshToken: null,
  user: null,
};
```

권장 동작:
1. login 시 두 토큰을 메모리에 저장
2. 보호 API 호출 시 `Authorization: Bearer <access_token>` 전송
3. 보호 API가 `401`을 주면 refresh를 1회 시도
4. 새 access token으로 원래 요청을 재시도
5. refresh도 실패하면 auth state를 지우고 로그인 화면으로 복귀

## 6. 예제 helper

```js
async function api(path, init = {}, retry = true) {
  const headers = new Headers(init.headers || {});
  if (authState.accessToken) {
    headers.set('Authorization', `Bearer ${authState.accessToken}`);
  }
  if (!headers.has('Content-Type') && init.body) {
    headers.set('Content-Type', 'application/json');
  }

  const response = await fetch(`${baseUrl}${path}`, {
    ...init,
    headers,
  });

  if (response.status === 401 && retry && authState.refreshToken) {
    await refreshSession();
    return api(path, init, false);
  }

  return response;
}
```

## 7. 앱 개발자 운영 메모

- 첫 등록 유저는 자동으로 active admin이 된다.
- 이후 유저는 admin activation이 필요할 수 있다.
- admin이 유저를 deactivate하면 access token 만료 전이라도 보호 API 접근이 즉시 차단된다.
- 현재 계약은 email/password 기반 앱, 내부 도구, self-host 대시보드에는 충분하다.
- OAuth, magic link, MFA는 이후 단계다.

## 8. 예제가 보여주는 것

`examples/auth-client-web/` 예제는 아래를 보여준다.
- register
- login
- `GET /api/me`
- 401 시 refresh 후 재시도
- logout
- session list
- 단일 session revoke
- 전체 session revoke
- forgot/reset password

의존성 없이 만든 이유는 auth 흐름 자체를 쉽게 읽게 하려는 목적이다.

## 9. 로컬 데모

1. Peanut을 실행한다.
2. 브라우저에서 `examples/auth-client-web/index.html`을 연다.
3. Peanut base URL을 보통 `http://127.0.0.1:3000`으로 맞춘다.
4. 첫 admin 유저를 등록하거나 기존 유저로 로그인한다.
5. refresh, session 관리, password reset 흐름을 직접 확인한다.

환경에 따라 브라우저가 `file://` fetch를 막으면 아무 정적 파일 서버로 예제 폴더를 서빙하면 된다.
