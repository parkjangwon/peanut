# Peanut 앱 단위 API

Peanut의 안정적인 런타임 API는 앱 단위 경로만 사용합니다. 공개 레거시 전역 경로는 런타임 API 표면에 포함하지 않습니다.

## 앱 경계

- Auth 사용자는 `(app_id, email)` 기준으로 분리됩니다.
- Data, Storage, Functions, Push, App Key는 모두 `app_id`를 기준으로 분리됩니다.
- JWT에는 `app_id`가 포함되며, SDK 미들웨어는 path app id, app key, user bearer token이 서로 맞지 않으면 거절합니다.

## SDK 헤더

```http
X-Peanut-Api-Key: <client-or-server-key>
Authorization: Bearer <user-access-token>
```

`/auth/me`, 클라이언트 업로드, 푸시 구독처럼 사용자 컨텍스트가 필요한 요청은 bearer token도 필요합니다.

## 주요 경로

- `POST /api/apps/:app_id/auth/register`
- `POST /api/apps/:app_id/auth/login`
- `GET /api/apps/:app_id/data/tables`
- `GET|POST /api/apps/:app_id/data/tables/:table/rows`
- `GET|PUT|DELETE /api/apps/:app_id/storage/buckets/:bucket/objects/*key`
- `POST /api/apps/:app_id/function-endpoints/:endpoint_slug`
- `GET|POST|DELETE /api/apps/:app_id/push/subscriptions`
- `POST /api/apps/:app_id/push/messages`

## 공개 베타 컨트롤 플레인

공개 베타는 조직 기반이며 초대 코드가 필요합니다.

## Workspace 컨트롤 플레인

Peanut은 셀프호스팅 전용입니다. workspace는 하나의 Peanut 인스턴스 안에서
내부 팀/프로젝트를 나누는 경계입니다.

- `POST /api/admin/workspace-invites`: 제한 사용 가능한 workspace 설정 초대 코드를 만듭니다.
- `GET /api/admin/workspace-invites`: 평문 초대 코드 없이 초대 목록을 조회합니다.
- `POST /api/workspace-invites/accept`: 초대를 소비해 workspace owner를 만듭니다.
- `GET /api/workspaces`: 로그인한 콘솔 사용자가 볼 수 있는 workspace를 조회합니다.
- `GET /api/workspaces/:workspace_id/resource-usage`: 리소스 사용량을 조회합니다.
- `POST /api/workspaces/:workspace_id/resource-limits`: 특정 리소스 제한을 조정합니다.
- `POST /api/admin/workspaces/:workspace_id/disable|enable`: workspace 접근 상태를 제어합니다.
- `POST /api/admin/apps/:app_id/disable|enable`: 앱 접근 상태를 제어합니다.

앱에는 `workspace_id`, `disabled_at`, `disabled_reason`이 포함됩니다.
`POST /api/apps`는 선택적으로 `workspace_id`를 받으며, 생략하면 default
workspace를 사용합니다. 앱 생성, 앱 사용자 등록, 데이터 row 생성, 스토리지 쓰기,
Function 호출, Push 발송, 월 SDK API 요청은 해당 workspace 리소스 제한을
초과하면 `code: "resource_limit_exceeded"`로 거절합니다. workspace 또는 앱이
disabled 상태이면 SDK 요청은 `workspace_disabled` 또는 `app_disabled` 코드로
거절됩니다.
