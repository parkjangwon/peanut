# 피넛 다음 개선 계획

**작성일**: 2026-05-01  
**브랜치**: master  
**현재 커밋**: `50e66e0` (chore: delete orphaned s3_object.rs…)

이 문서는 평가 세션에서 도출된 개선 항목을 우선순위 순서로 정리한 실행 계획이다.
각 항목은 독립적으로 커밋·푸시할 수 있도록 설계되었다.

---

## 항목 1 — Row 필터링을 SQL WHERE 절로 내려보내기 (High)

### 현황

`src/api/data/rows.rs` `execute_list_rows` 함수는 다음 순서로 동작한다:

1. SQLite에서 테이블의 **모든** 행을 읽어온다 (LIMIT 100 고정, `MAX_LIST_ROWS`).
2. 메모리에서 `apply_row_filters()` → `sort_rows()` → `skip/take` 로 필터·정렬·페이지를 처리한다.

문제:
- `filter_field` / `filter_op` / `filter_value`, `search`, `title_contains`, `done` 필터가 모두 인메모리 처리.
- offset이 커도 앞 행을 전부 읽은 후 버린다.
- `order_by`도 메모리 정렬이라 인덱스를 활용하지 못한다.

### 목표

`data_rows` 테이블의 `data_json` 컬럼은 JSON 텍스트로 저장된다.  
SQLite는 `json_extract(data_json, '$.field')` 함수를 지원하므로, 단순 비교 필터와 정렬은 SQL로 내릴 수 있다.

### 구현 방법

#### 대상 파일

| 파일 | 역할 |
|------|------|
| `src/api/data/rows.rs` | `execute_list_rows` 함수 수정 |
| `src/api/data/query.rs` | SQL 빌더 헬퍼 추가, 기존 `apply_row_filters` 유지(하위호환) |
| `src/api/data/internal.rs` | `DataRowRecord` sqlx 쿼리 관련 |

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

#### 지원할 필터 (SQL로 내릴 수 있는 것)

| 파라미터 | SQL 변환 |
|----------|---------|
| `filter_field` + `filter_op=eq` + `filter_value` | `json_extract(data_json, '$.{field}') = ?` |
| `filter_field` + `filter_op=neq` | `json_extract(...) != ?` |
| `filter_field` + `filter_op=gt` / `lt` / `gte` / `lte` | `json_extract(...) > ?` 등 |
| `filter_field` + `filter_op=contains` | `json_extract(...) LIKE '%' || ? || '%'` |
| `done=true/false` | `json_extract(data_json, '$.done') = ?` (1 또는 0) |
| `title_contains` | `json_extract(data_json, '$.title') LIKE ?` |
| `search` | 인메모리 유지 (여러 필드 OR — SQL FTS가 없으므로) |
| `order_by` | `created_at`, `updated_at` → 컬럼명 직접 사용, 스키마 필드 → `json_extract(data_json, '$.{field}')` |
| `limit` / `offset` | `LIMIT ? OFFSET ?` |

**주의**: `filter_field`, `order_by` 값은 반드시 스키마 필드 목록 + `['created_at', 'updated_at', 'id']` 허용 목록에 대조하여 검증한 후에만 SQL에 삽입한다 (이미 `validate_list_rows_params`에서 수행 중이므로 동일 로직 재사용).

#### execute_list_rows 변경 포인트 (rows.rs)

현재 (lines 143–160):
```rust
let rows_result = if table.access_policy.mode == POLICY_OWNER_PRIVATE && !claims.is_admin {
    sqlx::query_as::<_, DataRowRecord>(
        "SELECT ... FROM data_rows WHERE table_id = ? AND owner_user_id = ? ORDER BY created_at DESC LIMIT ?",
    )
    .bind(&table.id).bind(&claims.sub).bind(MAX_LIST_ROWS)
    .fetch_all(&state.pool).await
} else {
    sqlx::query_as::<_, DataRowRecord>(
        "SELECT ... FROM data_rows WHERE table_id = ? ORDER BY created_at DESC LIMIT ?",
    )
    .bind(&table.id).bind(MAX_LIST_ROWS)
    .fetch_all(&state.pool).await
};
```

변경 후:
```rust
let query = build_row_query(params, &table.schema, &table.id, owner_clause);
let sql = format!(
    "SELECT id, owner_user_id, data_json, created_at, updated_at \
     FROM data_rows \
     WHERE table_id = ? {where_part} \
     ORDER BY {order} \
     LIMIT ? OFFSET ?",
    where_part = if query.where_clauses.is_empty() { String::new() }
                 else { format!("AND {}", query.where_clauses.join(" AND ")) },
    order = query.order_sql,
);
// bind table_id, then each query.binds, then limit, offset
```

sqlx는 동적 쿼리에 대해 `query_as` + 개별 `bind` 체인을 지원한다.  
bind 개수가 런타임에 달라지므로 `sqlx::query_as` 의 반환 타입을 `QueryAs`로 받아 반복 bind 후 실행한다.

#### search는 인메모리 유지

`search` 파라미터는 스키마의 모든 string 타입 필드에 대한 OR 검색이다.  
SQLite FTS5 없이 이를 SQL로 표현하면 필드 수만큼 `OR json_extract(...) LIKE ?` 가 생성되어 관리가 복잡해진다.  
→ `search`가 있으면 SQL LIMIT을 크게 잡고(MAX_LIST_ROWS 그대로) 인메모리 필터 후 페이지 적용하는 현행 방식 유지.  
→ `search`가 없으면 SQL에서 limit/offset을 직접 적용.

#### 테스트 요구사항

- `tests/data_test.rs` (새 파일) 에 다음 케이스 추가:
  - `filter_op=eq` 로 특정 필드 값 필터링
  - `filter_op=contains` 로 부분 문자열 필터링
  - `done=true` 필터
  - `order_by=created_at&order=asc` 정렬
  - `limit=2&offset=2` 페이지네이션
  - owner_private 테이블에서 본인 rows만 반환 확인

---

## 항목 2 — Auth 엔드포인트 전용 레이트 리밋 (Medium)

### 현황

`src/middleware/rate_limit.rs`의 `rate_limit_middleware`는 IP당 100req/60s 전역 버킷 하나만 사용한다.  
`/api/login` 브루트포스는 다른 API 호출과 같은 버킷을 공유하므로, 공격자가 다른 경로를 호출하지 않으면 분당 100번 패스워드 시도가 가능하다.

### 목표

`/api/login`, `/api/register`, `/api/auth/refresh` 에 대해 **IP + 엔드포인트** 단위로 별도의 엄격한 제한을 적용한다.

### 구현 방법

#### 대상 파일

| 파일 | 변경 내용 |
|------|---------|
| `src/middleware/rate_limit.rs` | auth 전용 미들웨어 추가 |
| `src/app.rs` | auth route group에 새 미들웨어 레이어 추가 |
| `src/lib.rs` (`AppState`) | auth 전용 레이트리밋 상태 필드 추가 |

#### AppState 변경 (lib.rs)

```rust
// 기존
pub rate_limit_state: Arc<DashMap<IpAddr, (u32, Instant)>>,

// 추가
pub auth_rate_limit_state: Arc<DashMap<IpAddr, (u32, Instant)>>,
```

`main.rs`에서 초기화 시 `auth_rate_limit_state: Arc::new(DashMap::new())` 추가.

#### 새 미들웨어 (rate_limit.rs에 추가)

```rust
/// /api/login, /api/register, /api/auth/refresh 전용 엄격한 레이트리밋
/// IP당 10req/60s
pub async fn auth_rate_limit_middleware(
    State(state): State<crate::AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    const AUTH_LIMIT: u32 = 10;

    let client_ip = get_client_ip(&req, addr, state.trust_proxy_headers);
    let now = Instant::now();
    let mut entry = state.auth_rate_limit_state.entry(client_ip).or_insert((0, now));
    let (count, last_reset) = entry.value_mut();

    if now.duration_since(*last_reset) > Duration::from_secs(60) {
        *count = 1;
        *last_reset = now;
    } else {
        if *count >= AUTH_LIMIT {
            return Err(json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many authentication attempts. Please try again later.",
            ));
        }
        *count += 1;
    }
    drop(entry);
    Ok(next.run(req).await)
}
```

#### app.rs 라우터 변경

```rust
// 현재 public_auth_routes 그룹에 .layer(middleware::from_fn_with_state(...auth_rate_limit...)) 추가
let public_auth_routes = Router::new()
    .route("/api/register", post(auth::register))
    .route("/api/login", post(auth::login))
    .route("/api/auth/refresh", post(auth::refresh_token))
    // ... 기타 공개 auth 라우트
    .layer(middleware::from_fn_with_state(state.clone(), auth_rate_limit_middleware));
```

#### 테스트 요구사항

- `tests/auth_test.rs` 또는 `tests/rate_limit_test.rs`에 추가:
  - 10번 로그인 시도 후 11번째에 429 반환 확인
  - 60초 후 카운트 리셋 확인 (mock 시간 사용 불가하면 단위 테스트로)
  - 일반 API 엔드포인트는 auth 레이트리밋에 영향받지 않음 확인

---

## 항목 3 — JWT 알고리즘 명시적 허용 목록 (Low, 빠른 작업)

### 현황

`src/auth/jwt.rs:36`:
```rust
&Validation::default()
```

`jsonwebtoken` 의 `Validation::default()`는 현재 HS256 알고리즘 허용 목록을 포함하지만, 라이브러리 버전 업데이트 시 기본값이 바뀔 수 있다.

### 구현 방법

```rust
// src/auth/jwt.rs
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &validation,
    )?;
    Ok(token_data.claims)
}
```

**단일 파일, 5줄 변경.** 기존 테스트(`test_jwt_flow`)가 그대로 통과해야 한다.

---

## 항목 4 — AppState 필드 그룹화 (Medium, 아키텍처)

### 현황

`src/lib.rs`의 `AppState`는 17개 공개 필드를 가진 평탄한 구조체다.  
기능이 추가될 때마다 필드가 늘어나고, `test_support.rs`의 `make_test_state()`도 함께 수정해야 한다.

### 목표

논리적 그룹 3개로 분리하여 응집도를 높인다.

```rust
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub storage: Arc<crate::storage::local::LocalStorage>,
    pub auth: AuthConfig,
    pub functions: FunctionsConfig,
    pub system: SystemConfig,
}

#[derive(Clone)]
pub struct AuthConfig {
    pub jwt_secret: Arc<String>,
    pub password_reset_delivery: crate::config::PasswordResetDelivery,
    pub allowed_origins: Arc<Vec<String>>,
    pub allowed_client_ids: Arc<Vec<String>>,
    pub rate_limit_state: Arc<DashMap<IpAddr, (u32, Instant)>>,
    pub auth_rate_limit_state: Arc<DashMap<IpAddr, (u32, Instant)>>,
}

#[derive(Clone)]
pub struct FunctionsConfig {
    pub enabled: bool,
    pub allow_network: bool,
    pub work_dir: PathBuf,
    pub max_concurrent: usize,
    pub semaphore: Arc<tokio::sync::Semaphore>,
    pub event_sender: tokio::sync::broadcast::Sender<...>,
}

#[derive(Clone)]
pub struct SystemConfig {
    pub database_url: Arc<String>,
    pub trust_proxy_headers: bool,
    pub multipart_stale_hours: u64,
    pub last_backup_at: Arc<tokio::sync::RwLock<Option<...>>>,
    pub started_at: std::time::Instant,
    pub data_event_sender: tokio::sync::broadcast::Sender<...>,
}
```

### 주의사항

- 이 작업은 **전체 코드베이스에 영향**을 준다 (`state.jwt_secret` → `state.auth.jwt_secret` 등).
- `grep -rn "state\." src/` 로 참조 위치 전체 파악 후 진행할 것.
- `src/test_support.rs` 의 `make_test_state()`도 함께 업데이트해야 한다.
- **항목 1, 2, 3을 먼저 완료한 후 별도 커밋으로 처리** 권장.

---

## 실행 순서

```
항목 3 (JWT 알고리즘)   → 5분, 위험 없음, 먼저 처리
항목 2 (auth 레이트리밋) → 1~2시간, 중간 난이도
항목 1 (SQL 필터 pushdown) → 3~4시간, 가장 임팩트 큼
항목 4 (AppState 리팩토링) → 2~3시간, 마지막 처리
```

각 항목은 독립 커밋으로 `master`에 직접 푸시한다.  
커밋 컨벤션: `fix:`, `feat:`, `perf:`, `refactor:` 프리픽스 사용.  
커밋 사용자: `parkjangwon <vim@kakao.com>`

---

## 참조 파일 목록

| 파일 | 관련 항목 |
|------|---------|
| `src/api/data/rows.rs` | 항목 1 (execute_list_rows 함수) |
| `src/api/data/query.rs` | 항목 1 (apply_row_filters, SQL 빌더 추가) |
| `src/api/data/types.rs` | 항목 1 (ListRowsParams 구조체) |
| `src/api/data/internal.rs` | 항목 1 (DataRowRecord, load_row) |
| `src/middleware/rate_limit.rs` | 항목 2 (auth_rate_limit_middleware 추가) |
| `src/app.rs` | 항목 2 (라우터에 미들웨어 추가) |
| `src/auth/jwt.rs` | 항목 3 (verify_jwt Validation 수정) |
| `src/lib.rs` | 항목 2 (AppState 필드), 항목 4 (그룹화) |
| `src/main.rs` | 항목 2 (AppState 초기화), 항목 4 |
| `src/test_support.rs` | 항목 2, 4 (make_test_state 업데이트) |
| `tests/auth_test.rs` | 항목 2 (레이트리밋 테스트) |
