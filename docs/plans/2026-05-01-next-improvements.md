# Peanut 개선 계획 (갱신)

작성일: 2026-06-19  
브랜치: `cursor/code-improvements-1213`

이 문서는 2026-05-01 계획의 후속 실행 상태를 정리한다.

---

## 완료된 항목

| 항목 | 상태 | 비고 |
|------|------|------|
| Row 필터 SQL pushdown | ✅ 완료 | `build_row_query()` + `execute_list_rows()` |
| Auth 전용 rate limit | ✅ 완료 | 60초/10회, `auth_rate_limit_middleware` |
| JWT HS256 고정 | ✅ 완료 | `jwt_validation()` |
| AppState 통합 | ✅ 완료 | `src/state.rs`, `RateLimitState` 그룹화 |
| App scope 보일러플레이트 축소 | ✅ 완료 | `app_claims.rs` + `app_developer!` 매크로 |
| Data 페이지네이션 메타 | ✅ 완료 | `total`, `limit`, `offset`, `has_more` |
| dead code 정리 | ✅ 완료 | `AppContext`, 인메모리 row filter 제거 |
| tracing 보강 | ✅ 완료 | rate limit / auth 실패 경로 |
| Console Error Boundary | ✅ 완료 | `error-boundary.tsx` |
| Console api.ts 분리 | ✅ 완료 | `lib/api/{types,session,client,auth}.ts` |
| providers 모듈화 | ✅ 부분 | `providers/mod.rs` 디렉터리 구조 |

---

## 남은 항목 (후속)

| 항목 | 우선순위 | 설명 |
|------|----------|------|
| `auth/providers/oauth.rs` 분리 | Medium | OAuth 핸들러·헬퍼를 별도 파일로 |
| `auth.rs` / `workspaces.rs` 분할 | Medium | 1,000줄+ 모듈 도메인별 분리 |
| Redis 기반 rate limit | Low | 멀티 인스턴스 확장 시 |
| `json_extract` 정렬 인덱스 | Low | 대용량 테이블 `order_by=title` 성능 |
| Console E2E 테스트 | Low | Playwright 등 |

---

## 실행 검증

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cd console && npm ci && npm run lint && npm run build
```

Function invoke 통합 테스트는 로컬 Deno 런타임 설치 여부에 따라 실패할 수 있다.
