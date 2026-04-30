# 피넛 다음 개선 계획

작성일: 2026-05-01  
브랜치: master  
현재 커밋: 50e66e0 (chore: delete orphaned s3_object.rs…)

이 문서는 평가 세션에서 도출된 개선 항목을 우선순위 순서로 정리한 실행 계획이다.
각 항목은 독립적으로 커밋·푸시할 수 있도록 설계되었다.

---

## 항목 1 — Row 필터링을 SQL WHERE 절로 내려보내기 (High)

### 현황

src/api/data/rows.rs execute_list_rows 함수는 다음 순서로 동작한다:

1. SQLite에서 테이블의 모든 행을 읽어온다 (LIMIT 100 고정, MAX_LIST_ROWS).
2. 메모리에서 apply_row_filters() → sort_rows() → skip/take 로 필터·정렬·페이지를 처리한다.

문제:
- filter_field / filter_op / filter_value, search, title_contains, done 필터가 모두 인메모리 처리.
- offset이 커도 앞 행을 전부 읽은 후 버린다.
- order_by도 메모리 정렬이라 인덱스를 활용하지 못한다.

### 목표

data_rows 테이블의 data_json 컬럼은 JSON 텍스트로 저장된다.  
SQLite는 json_extract(data_json, '$.field') 함수를 지원하므로, 단순 비교 필터와 정렬은 SQL로 내릴 수 있다.

### 구현 방법

#### 대상 파일

| 파일 | 역할 |
|------|------|
| src/api/data/rows.rs | execute_list_rows 함수 수정 |
| src/api/data/query.rs | SQL 빌더 헬퍼 추가, 기존 apply_row_filters 유지(하위호환) |
| src/api/data/internal.rs | DataRowRecord sqlx 쿼리 관련 |

#### SQL 빌더 구현 (query.rs에 추가)

```rust
pub(crate) struct RowQuery {
    pub where_clauses: Vec<String>,
    pub binds: Vec<RowQueryBind>,
    pub order_sql: String,
    pub limit: i64,
    pub offset: i64,
}

pub(crate) enum RowQueryBind {
    Text(String),
    Bool(bool),
    Int(i64),
}

/// ListRowsParams와 access policy를 받아 SQL 파라미터를 구성한다.
/// 반환된 where_clauses는 AND로 조인하여 WHERE 절에 사용한다.
/// 반환된 binds는 sqlx query에 순서대로 bind한다.
pub(crate) fn build_row_query(
    params: &ListRowsParams,
    schema: &DataTableSchema,
    table_id: &str,
    owner_user_id: Option<&str>,
) -> RowQuery { ... }
```

구현 규칙:
- 기본 WHERE는 `table_id = ?`
- owner_private 정책이면 `owner_user_id = ?`
- `search`는 title/body 같은 known field에 한정하지 말고, 기존 로직이 JSON 문자열 전체에 대해 부분 문자열 검사라면 우선 `data_json LIKE ?`로 맞춘다.
- `title_contains`는 `json_extract(data_json, '$.title') LIKE ?`
- `done`은 `json_extract(data_json, '$.done') = 1/0`
- `filter_field/filter_op/filter_value`는 schema에서 field 존재 검증 후 SQL 생성
- `order_by`는 허용 목록만 SQL 문자열로 매핑해 injection 방지
  - 예: created_at, updated_at, title
  - title은 `json_extract(data_json, '$.title')`
- limit/offset은 validated params 사용

주의:
- field path는 절대 사용자 입력을 그대로 SQL 문자열에 넣지 말고, schema의 field name 허용 목록을 거쳐 escape-free path로 조립
- op도 eq/ne/gt/gte/lt/lte/contains 정도만 enum 매핑
- contains는 `LIKE '%' || ? || '%'`

#### rows.rs 수정

기존:
- load_rows_for_table(...)
- apply_row_filters(...)
- sort_rows(...)
- skip/take

변경 후:
- validated params 확보
- build_row_query(...) 호출
- SQL 문자열 조립:
  ```sql
  SELECT id, table_id, owner_user_id, data_json, created_at, updated_at
  FROM data_rows
  WHERE ...
  ORDER BY ...
  LIMIT ? OFFSET ?
  ```
- sqlx::query_as::<_, DataRowRecord>(&sql) + binds 적용
- 결과를 그대로 RowResponse 변환

#### 테스트

추가/수정할 테스트:
- search 필터 결과 동일성
- title_contains 결과 동일성
- done=true/false 결과 동일성
- numeric field gt/gte/lt/lte
- owner_private 정책에서 자기 row만 나오는지
- order_by=title / created_at / updated_at
- offset, limit 동작

테스트 위치:
- src/api/data/rows.rs 의 #[cfg(test)] 또는 query.rs 테스트 모듈

#### 완료 기준

- 기존 list_rows API 응답 스키마 불변
- 기존 테스트 통과
- 새 SQL 푸시다운 테스트 추가
- apply_row_filters()는 당장 삭제하지 말고 fallback/test helper로 남겨도 됨

#### 권장 커밋 메시지

`refactor: push row filters down into sqlite queries`

---

## 항목 2 — Auth 엔드포인트 전용 레이트 리밋 추가 (Medium)

### 현황

현재 middleware/rate_limit.rs는 전역 단일 버킷으로 IP당 분당 100 요청을 제한한다.
로그인, 비밀번호 재설정, 세션 갱신 같은 auth 엔드포인트도 같은 정책을 사용한다.

문제:
- 로그인 brute-force 방지 관점에서 너무 느슨함
- 일반 API burst와 auth endpoint를 구분하지 않음

### 목표

auth endpoint 전용 미들웨어를 하나 더 둬서 민감 경로에 stricter rate limit을 적용한다.

### 구현 방법

#### 대상 파일

| 파일 | 역할 |
|------|------|
| src/middleware/rate_limit.rs | AuthRateLimiter 추가 |
| src/app.rs | auth public/protected routes에 미들웨어 장착 |
| src/config.rs | auth_rate_limit_* 설정 추가 (선택) |

#### 정책

최소 구현:
- IP당 60초에 10회
- 대상 경로:
  - /login
  - /auth/refresh
  - /auth/forgot-password
  - /auth/reset-password
  - 필요 시 /auth/change-password

구조 제안:

```rust
#[derive(Clone)]
pub struct AuthRateLimiter {
    inner: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    max_requests: usize,
    window: Duration,
}
```

또는 기존 RateLimiter를 일반화해 두 개 인스턴스를 app state에 둘 수도 있음.

#### app.rs 적용

build_auth_public_routes / build_auth_protected_routes 레벨에:
- global rate limit은 유지
- auth 전용 stricter middleware를 추가로 레이어링

#### 테스트

- 같은 IP에서 10회까지 허용, 11회째 429
- 60초 window 경과 후 다시 허용
- 일반 data endpoint는 영향 없음

#### 완료 기준

- 로그인/비밀번호 재설정 계열 경로에 stricter limit 적용
- 기존 global limiter 동작 유지
- 429 응답 포맷 기존 에러 구조와 일치

#### 권장 커밋 메시지

`feat: add stricter rate limiting for auth endpoints`

---

## 항목 3 — JWT Validation에 알고리즘 허용 목록 명시 (Low, quick win)

### 현황

src/auth/jwt.rs 에서 jsonwebtoken::Validation::default() 사용.
라이브러리 기본값이 HS256이긴 하지만, 코드상 의도가 명시되지 않는다.

### 목표

허용 알고리즘을 명시적으로 고정한다.

### 구현 방법

#### 대상 파일

| 파일 | 역할 |
|------|------|
| src/auth/jwt.rs | validation.algorithms 명시 |

#### 변경

```rust
use jsonwebtoken::Algorithm;

let mut validation = Validation::default();
validation.algorithms = vec![Algorithm::HS256];
validation.validate_exp = true;
```

가능하면 issuer / audience 검증도 현 config 모델과 맞으면 추가 검토하되, 이번 단계에서는 scope를 최소화한다.

#### 테스트

- 기존 JWT encode/decode 테스트 유지
- 가능하면 algorithms list 명시 후도 정상 decode 확인

#### 완료 기준

- HS256만 허용됨이 코드상 명확
- 테스트 통과

#### 권장 커밋 메시지

`security: pin accepted jwt algorithm to hs256`

---

## 항목 4 — AppState 기능 그룹화 (Medium)

### 현황

src/lib.rs 의 AppState가 현재 약 17개 필드의 평탄 구조다.
새 기능이 추가될 때마다 테스트 helper, 라우터 setup, clone 지점이 광범위하게 깨진다.

### 목표

AppState를 완전히 DI 프레임워크화하지 말고, 최소한 기능별 config 묶음으로 그룹화한다.

### 구현 방향

#### 대상 파일

| 파일 | 역할 |
|------|------|
| src/lib.rs | AppState 구조 재정의 |
| src/app.rs | field access 변경 |
| src/api/* | state.<field> 접근 일부 수정 |
| tests/* | state 생성 helper 수정 |

#### 제안 구조

```rust
#[derive(Clone)]
pub struct AuthState {
    pub jwt_secret: Arc<String>,
    pub access_token_ttl_seconds: i64,
    pub refresh_token_ttl_seconds: i64,
    pub auth_clients: Arc<Vec<AuthClientConfig>>,
    pub password_reset_token_ttl_seconds: i64,
}

#[derive(Clone)]
pub struct FunctionsState {
    pub functions_store: FunctionsStore,
    pub functions_queue: FunctionInvocationQueue,
    pub functions_event_bus: EventBus<FunctionInvocationEvent>,
    pub functions_runtime: FunctionsRuntimeHandle,
}

#[derive(Clone)]
pub struct PushState {
    pub vapid: Option<VapidConfig>,
    pub push_queue: PushDeliveryQueue,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub storage: Arc<dyn ObjectStorage>,
    pub auth: AuthState,
    pub functions: FunctionsState,
    pub push: PushState,
    pub service_tokens_enabled: bool,
    pub max_upload_bytes: usize,
    pub request_id_header: Arc<String>,
    ...
}
```

핵심:
- 너무 과하게 쪼개지 말 것
- config/runtime/bus/queue가 섞인 현재 필드만 논리적으로 묶어 테스트 영향을 줄이는 수준

#### 리팩터링 순서

1. 새 nested state struct 추가
2. app bootstrap에서 조립
3. compile error 따라 state 접근부 수정
4. test helper 업데이트
5. cargo test 전체 검증

#### 완료 기준

- 기능 변화 없음
- AppState 필드 수 체감 감소
- auth/functions/push 관련 state access가 grouped path로 정리

#### 권장 커밋 메시지

`refactor: group app state by feature area`

---

## 실행 순서 제안

1. 항목 3 (JWT 알고리즘 명시) — 5분, 빠른 보안 개선
2. 항목 2 (Auth rate limit) — 범위 작고 효과 큼
3. 항목 1 (Row filter SQL pushdown) — 가장 큰 성능 개선
4. 항목 4 (AppState 그룹화) — 리스크가 있어 마지막

실제 작업 시에는 항목별로:
- 구현
- cargo fmt
- cargo test
- 필요 시 ./scripts/build.sh
- 커밋
- 푸시

순으로 독립 진행하는 것을 권장한다.
