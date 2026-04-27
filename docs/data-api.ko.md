# Peanut Data API 가이드

Peanut은 앱용 CRUD와 운영 흐름을 위해 bounded SQLite 기반 Data API를 제공한다.

이 문서는 아래에 집중한다.
- table/row 모델
- 현재 지원하는 query 표면
- 바로 써볼 수 있는 예제 스키마
- replay/checkpoint/SSE 흐름

같이 보면 좋은 문서:
- `README.ko.md`
- `docs/auth-client.ko.md`
- `docs/data-api.md`

## 1. 제품 경계

Peanut Data API는 의도적으로 좁다.

잘 맞는 용도:
- self-host 앱 백엔드
- admin이 정의하는 logical table
- schema-aware row CRUD
- bounded filtering / sorting / pagination
- 운영 친화적인 replay / export / import 흐름

목표가 아닌 것:
- HTTP raw SQL
- arbitrary join
- 범용 DB 콘솔
- 자유로운 쿼리 실행 엔진

## 2. 핵심 모델

현재 구조:
- admin이 logical table을 만든다
- 각 table은 아래를 가진다
  - `name`
  - `display_name`
  - `schema`
  - `access_policy`
- row는 Peanut이 관리하는 SQLite 테이블에 저장된다
- row 변경은 내부 event log에 기록된다

현재 유용한 access policy:
- `owner_private`
  - 각 row가 인증 사용자에게 귀속된다
  - 일반 row 접근은 owner 기준으로 격리된다
- `authenticated_shared_rw`
  - 인증 사용자가 공유 테이블을 함께 사용한다
- `admin_only`
  - 운영자 전용 테이블이다

## 3. 엔드포인트

Table 엔드포인트:
- `GET /api/data/tables`
- `POST /api/data/tables`
- `GET /api/data/tables/:table`
- `PATCH /api/data/tables/:table`
- `DELETE /api/data/tables/:table`

Row 엔드포인트:
- `GET /api/data/tables/:table/rows`
- `POST /api/data/tables/:table/rows`
- `GET /api/data/tables/:table/rows/:row_id`
- `PATCH /api/data/tables/:table/rows/:row_id`
- `DELETE /api/data/tables/:table/rows/:row_id`

Event / replay 엔드포인트:
- `GET /api/data/tables/:table/events`
- `GET /api/data/tables/:table/events/checkpoint`
- `GET /api/data/tables/:table/events/stream`

Preset / export / import 엔드포인트:
- `GET /api/data/tables/:table/presets`
- `POST /api/data/tables/:table/presets`
- `GET /api/data/tables/:table/presets/:preset_id/run`
- `PATCH /api/data/tables/:table/presets/:preset_id`
- `DELETE /api/data/tables/:table/presets/:preset_id`
- `GET /api/data/tables/:table/export`
- `POST /api/data/tables/:table/import`

## 4. 빠른 예제: todos 테이블

테이블 생성:

```json
{
  "name": "todos",
  "display_name": "Todos",
  "schema": {
    "fields": {
      "title": { "type": "string", "required": true, "max_length": 200 },
      "done": { "type": "boolean", "required": false, "default": false },
      "priority": { "type": "integer", "required": false, "default": 0 }
    }
  },
  "access_policy": { "mode": "owner_private" }
}
```

row 추가:

```json
{
  "title": "buy milk",
  "priority": 2
}
```

대표 row 응답 형태:

```json
{
  "row": {
    "id": "uuid",
    "owner_user_id": "uuid",
    "data": {
      "title": "buy milk",
      "done": false,
      "priority": 2
    },
    "created_at": "2026-04-27 12:00:00",
    "updated_at": "2026-04-27 12:00:00"
  }
}
```

## 5. 현재 지원하는 query 표면

`GET /api/data/tables/:table/rows` 는 bounded query 계약을 지원한다.

유용한 파라미터:
- `limit`
- `offset`
- `order_by`
- `order=asc|desc`
- `search`
- `filter_field`
- `filter_op`
- `filter_value`

현재 string filter op:
- `eq`
- `ne`
- `contains`
- `starts_with`
- `ends_with`

그 외 현재 지원하는 filter 형태:
- `integer`, `number`, `datetime`: `eq`, `ne`, `gt`, `gte`, `lt`, `lte`
- `boolean`: `eq`, `ne`
- `json`: `eq`, `ne`

중요한 동작:
- `search` 는 table schema에 선언된 string field만 스캔한다
- `offset` 은 filtering/sorting 이후에 적용된다
- 알 수 없는 `order_by` 값은 거부된다
- `filter_field`, `filter_op`, `filter_value` 는 함께 보내야 한다

예시:

```bash
curl -s "$BASE_URL/api/data/tables/todos/rows?search=buy&filter_field=title&filter_op=starts_with&filter_value=buy&order_by=title&order=asc&limit=10&offset=0" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

## 6. 바로 써볼 수 있는 예제 스키마

### A. Todos

잘 맞는 용도:
- 개인별 task list
- `owner_private` 검증
- string filter와 default 테스트

추천 field:
- `title: string`
- `done: boolean`
- `priority: integer`
- `due_at: datetime`

### B. Contacts

잘 맞는 용도:
- 간단한 공유형 CRM-lite
- sort/search 동작 검증

추천 access policy:
- `authenticated_shared_rw`

추천 field:
- `name: string`
- `email: string`
- `company: string`
- `notes: string`

### C. Posts

잘 맞는 용도:
- 간단한 content backend
- draft/publish 상태 관리
- export/import 흐름 검증

추천 field:
- `title: string`
- `slug: string`
- `body: string`
- `status: string`

## 7. 안전한 schema evolution 규칙

`PATCH /api/data/tables/:table` 는 의도적으로 보수적이다.

현재 규칙:
- field type은 in-place 변경할 수 없다
- row가 이미 있는 table은 기존 field를 삭제할 수 없다
- row가 이미 있는 table에 required field를 추가하려면 default가 필요하다

즉, 가능한 것:
- optional field 추가
- default가 있는 required field 추가
- `display_name` 변경

거부되는 것:
- `title: string -> integer`
- row가 있는 뒤 `done` 삭제
- row가 있는 상태에서 default 없는 required `priority` 추가

## 8. Replay와 realtime

Peanut은 row event에 대해 서로 보완적인 두 흐름을 제공한다.

### A. Checkpoint + replay

sync worker나 운영 프로세스가 durable resume point가 필요할 때 쓴다.

1. 현재 checkpoint 조회:

```bash
curl -s "$BASE_URL/api/data/tables/todos/events/checkpoint" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

대표 응답:

```json
{
  "table_name": "todos",
  "latest_event_id": 42
}
```

2. 이후 더 최신 event만 replay:

```bash
curl -s "$BASE_URL/api/data/tables/todos/events?since_id=42&limit=50" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

잘 맞는 용도:
- 재시작 후 resume worker
- admin sync 도구
- SSE 끊김 이후 복구

### B. SSE stream

row mutation 실시간 이벤트가 필요할 때 쓴다.

```bash
curl -N "$BASE_URL/api/data/tables/todos/events/stream" \
  -H 'authorization: Bearer YOUR_DATA_API_TOKEN'
```

실전 패턴:
1. checkpoint 조회
2. SSE stream 연결
3. 마지막으로 본 event id 저장
4. stream이 끊기면 `since_id=<last_seen_id>` 로 replay

## 9. Query preset

preset은 운영자나 도구가 같은 조회를 반복할 때 유용하다.

좋은 사용 예:
- open todos
- high-priority contacts
- draft posts

preset은 saved bounded query로 생각하면 되고, custom report engine처럼 쓰지 않는 게 좋다.

## 10. Export / import

Export:
- `GET /api/data/tables/:table/export`

Import:
- `POST /api/data/tables/:table/import`

유용한 mode:
- `append`
- `replace`

실전 용도:
- 백업
- 환경 초기 데이터 주입
- fixture replay
- known-good table snapshot 복원

## 11. 실전 추천

Peanut Data API로 새 앱을 시작한다면:
1. `owner_private` 또는 `authenticated_shared_rw` 부터 시작
2. schema는 작고 명확하게 유지
3. additive schema 변경은 default 중심으로 설계
4. sync worker에는 replay/checkpoint 사용
5. 반복 운영 조회는 query preset 사용

## 12. 현재 비목표

여전히 범위 밖:
- `/api/sql`
- arbitrary join
- unbounded full-text search
- DB 내부 확장을 직접 클라이언트에 노출하는 것
