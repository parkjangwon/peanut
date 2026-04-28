# Peanut 안정성 및 관측성 강화 상세 설계서 (Stability & Observability Hardening)

**날짜:** 2026-04-28
**상태:** 승인됨

## 1. 개요
Peanut 백엔드의 프러덕션 릴리즈를 위해 데이터 보호(백업), 서버 자원 보호(속도 제한), 그리고 시스템 상태 가시성(상태 확인 강화)을 개선합니다.

## 2. 세부 설계

### 2.1. SQLite 자동 백업 시스템
*   **파일 구조**: 
    *   기본 DB 파일명: `peanut.db`
    *   백업 파일명 패턴: `peanut.db.YYYYMMDD_HHMMSS.backup`
*   **실행 로직**:
    *   `tokio::spawn`을 통해 백그라운드 워커 실행.
    *   24시간 주기로 `sqlx`를 통해 `VACUUM INTO '파일명'` 실행.
    *   백업 완료 후 디렉토리 내 `.backup` 파일을 스캔하여 날짜 순으로 정렬.
    *   파일 개수가 7개를 초과하면 가장 오래된 파일 삭제.
*   **에러 처리**: 백업 실패 시 로그(`error!`)를 남기되 서버 실행에는 지장을 주지 않음.

### 2.2. IP 기반 속도 제한 (Rate Limiting) 미들웨어
*   **기본 정책**: IP 주소당 **분당 100회** 요청 허용.
*   **데이터 구조**: `AppState` 내에 `DashMap<IpAddr, (u32, Instant)>` (또는 유사한 동시성 지원 맵) 사용.
*   **작동 방식**:
    *   Axum 미들웨어 계층에서 요청 가로채기.
    *   현재 시간 윈도우 내 카운트 확인 및 증가.
    *   제한 초과 시 `429 Too Many Requests` 반환.
*   **확장성**: 향후 대시보드 연동을 위해 `RateLimitConfig` 구조체를 `AppState`에 포함하여 실시간 정책 변경이 가능하도록 설계.

### 2.3. 강화된 상태 확인 (Enhanced Readiness Check)
*   **엔드포인트**: `/api/ready`
*   **추가 메타데이터**:
    *   `database`: `size_bytes`, `wal_mode_enabled`
    *   `storage`: `available_space_bytes` (플랫폼 독립적인 방식으로 시도)
    *   `backup`: `last_backup_at`, `backup_count`
*   **목적**: 모니터링 대시보드에서 시스템의 물리적 상태를 한눈에 파악.

## 3. 변경 대상 파일
*   `src/main.rs`: 미들웨어 등록 및 백업 워커 시작.
*   `src/db.rs`: `VACUUM INTO` 래퍼 함수 추가.
*   `src/middleware/mod.rs`: `rate_limit.rs` 모듈 추가.
*   `src/api/health.rs`: 응답 데이터 구조 확장.

## 4. 테스트 전략
*   **백업**: 인메모리 DB가 아닌 파일 DB 환경에서 백업 파일 생성 및 회전(Rotation) 로직 검증.
*   **속도 제한**: 짧은 시간 내 100회 이상의 요청을 보내 `429` 응답 확인.
*   **상태 확인**: 반환되는 JSON 데이터에 새로운 필드들이 올바르게 포함되어 있는지 검증.
