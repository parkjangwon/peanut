# Peanut Data API 가이드

Peanut Data는 앱 단위로 격리된다. 관리자는
`/api/apps/:app_id/data/...` 경로로 테이블을 정의하고, 애플리케이션
클라이언트는 같은 앱 경로와 `X-Peanut-Api-Key`로 row를 읽고 쓴다.

## 경로

관리자 경로:

- `POST /api/apps/:app_id/data/tables`
- `PATCH /api/apps/:app_id/data/tables/:table`
- `DELETE /api/apps/:app_id/data/tables/:table`
- `GET /api/apps/:app_id/data/tables/:table/events`
- `GET /api/apps/:app_id/data/tables/:table/events/checkpoint`
- `GET /api/apps/:app_id/data/tables/:table/events/stream`
- `GET /api/apps/:app_id/data/tables/:table/presets`
- `POST /api/apps/:app_id/data/tables/:table/presets`
- `GET /api/apps/:app_id/data/tables/:table/presets/:preset_id/run`
- `PATCH /api/apps/:app_id/data/tables/:table/presets/:preset_id`
- `DELETE /api/apps/:app_id/data/tables/:table/presets/:preset_id`
- `GET /api/apps/:app_id/data/tables/:table/export`
- `POST /api/apps/:app_id/data/tables/:table/import`

SDK 경로:

- `GET /api/apps/:app_id/data/tables`
- `GET /api/apps/:app_id/data/tables/:table`
- `GET /api/apps/:app_id/data/tables/:table/rows`
- `POST /api/apps/:app_id/data/tables/:table/rows`
- `GET /api/apps/:app_id/data/tables/:table/rows/:row_id`
- `PATCH /api/apps/:app_id/data/tables/:table/rows/:row_id`
- `DELETE /api/apps/:app_id/data/tables/:table/rows/:row_id`
- `POST /api/apps/:app_id/data/query`

## 테이블 예시

```json
{
  "name": "todos",
  "display_name": "Todos",
  "schema": {
    "fields": {
      "title": { "type": "string", "required": true, "max_length": 200 },
      "done": { "type": "boolean", "default": false }
    }
  },
  "access_policy": { "mode": "authenticated_shared_rw" }
}
```

필드에는 데이터베이스다운 제약도 선언할 수 있습니다.

```json
{
  "name": "orders",
  "display_name": "Orders",
  "schema": {
    "fields": {
      "order_number": { "type": "string", "required": true, "unique": true },
      "account_id": {
        "type": "string",
        "required": true,
        "reference": { "table": "accounts" }
      }
    }
  },
  "access_policy": {
    "mode": "custom",
    "rules": {
      "read": { "allow": "owner" },
      "create": { "allow": "authenticated" },
      "update": { "allow": "owner" },
      "delete": { "allow": "admin" }
    }
  }
}
```

`unique`는 같은 Peanut 테이블 안에서 행 생성/수정 전에 검사됩니다.
`reference.table`은 같은 앱의 다른 테이블을 가리키며, 필드 값은 그
테이블의 row id여야 합니다. custom rule은 CRUD 작업마다 `admin`,
`authenticated`, `owner` 중 하나를 허용값으로 사용합니다.

## Row 예시

```json
{
  "data": {
    "title": "buy milk",
    "done": false
  }
}
```

## Query 범위

`GET /api/apps/:app_id/data/tables/:table/rows`는 제한된 list query를
지원한다.

- `limit`
- `offset`
- `order_by`
- `order=asc|desc`
- `search`
- `title_contains`
- `filter_field`
- `filter_op`
- `filter_value`

filter 연산자는 schema 타입에 맞춰 검증된다. 문자열은 `eq`, `ne`,
`contains`, `starts_with`, `ends_with`를 지원하고 숫자와 datetime은 비교
연산자를 지원한다. boolean은 equality 계열만 지원한다.

모든 table, row, preset, import/export, event query는 `app_id`로 격리된다.
서로 다른 앱은 같은 table 이름을 안전하게 사용할 수 있다.

SQL endpoint는 ANSI 스타일의 `SELECT`, `INSERT`, `UPDATE`, `DELETE` 중
하나의 문장을 받는다. 내부적으로 row CRUD API와 같은 schema 검증,
access policy, unique, reference, audit, event 경로를 사용한다. `UPDATE`와
`DELETE`에는 `WHERE id = ...` 조건이 필요하다.
