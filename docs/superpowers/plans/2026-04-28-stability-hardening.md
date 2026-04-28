# Peanut 안정성 및 관측성 강화 구현 계획서

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SQLite 백업 시스템, IP 기반 속도 제한 미들웨어, 그리고 강화된 상태 확인 기능을 구현하여 Peanut 백엔드의 안정성을 프러덕션 수준으로 높입니다.

**Architecture:** 
1. **백업**: `VACUUM INTO`를 사용하는 백그라운드 워커를 구현하고 최신 7개 파일을 유지하는 Rotation 로직을 적용합니다.
2. **속도 제한**: `AppState`에 공유 상태를 저장하고 클라이언트 IP 주소를 기반으로 분당 요청 수를 제한하는 미들웨어를 추가합니다.
3. **상태 확인**: DB 파일 크기와 마지막 백업 시간 등 상세 지표를 `/api/ready` 응답에 포함합니다.

**Tech Stack:** Rust, Axum, SQLx, Tokio, DashMap (추가 예정)

---

### Task 1: SQLite 자동 백업 시스템 구현

**Files:**
- Modify: `src/db.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: DB 백업 함수 및 Rotation 로직 구현**

`src/db.rs`에 백업을 수행하고 오래된 파일을 정리하는 함수를 추가합니다.

```rust
use std::path::Path;
use chrono::Local;

pub async fn backup_db(pool: &SqlitePool, db_path: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_path = format!("{}.{}.backup", db_path, timestamp);
    
    // SQLite VACUUM INTO 명령으로 안전하게 백업
    sqlx::query(&format!("VACUUM INTO '{}'", backup_path))
        .execute(pool)
        .await?;
        
    // Rotation: .backup 파일들 중 최신 7개만 남기고 삭제
    let db_dir = Path::new(db_path).parent().unwrap_or(Path::new("."));
    let mut backups: Vec<_> = std::fs::read_dir(db_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("backup"))
        .collect();
        
    backups.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    
    if backups.len() > 7 {
        for old_backup in backups.iter().take(backups.len() - 7) {
            std::fs::remove_file(old_backup.path())?;
        }
    }
    
    Ok(backup_path)
}
```

- [ ] **Step 2: 백그라운드 백업 워커 시작**

`src/main.rs`에서 서버 시작 시 24시간 주기로 백업을 수행하는 태스크를 스폰합니다.

- [ ] **Step 3: 커밋**
```bash
git add src/db.rs src/main.rs
git commit -m "feat(db): add automated sqlite backup with rotation"
```

---

### Task 2: IP 기반 속도 제한 미들웨어 구현

**Files:**
- Create: `src/middleware/rate_limit.rs`
- Modify: `src/middleware/mod.rs`, `src/main.rs`
- Modify: `Cargo.toml` (dashmap 추가)

- [ ] **Step 1: DashMap 의존성 추가**

```bash
cargo add dashmap
```

- [ ] **Step 2: 속도 제한 미들웨어 구현**

`src/middleware/rate_limit.rs`를 생성하고 분당 100회 요청 제한 로직을 작성합니다. IP 주소를 추출하기 위해 `axum::extract::ConnectInfo` 또는 `x-forwarded-for` 헤더를 고려해야 합니다.

- [ ] **Step 3: 미들웨어 등록**

`src/main.rs`의 Router 설정에 `rate_limit_middleware`를 적용합니다. `AppState`에 `DashMap` 인스턴스를 추가해야 합니다.

- [ ] **Step 4: 커밋**
```bash
git add Cargo.toml src/middleware/ src/main.rs
git commit -m "feat(middleware): add IP-based rate limiting"
```

---

### Task 3: 강화된 상태 확인 (Enhanced Health Check)

**Files:**
- Modify: `src/api/health.rs`

- [ ] **Step 1: Readiness Check 데이터 확장**

`/api/ready` 응답에 DB 파일 크기 및 백업 정보를 추가합니다. `std::fs::metadata`를 사용하여 파일 크기를 가져오고, `AppState`에 마지막 백업 시간을 기록할 필드를 추가하는 것을 고려합니다.

- [ ] **Step 2: 테스트 코드 업데이트**

새로운 필드들이 포함되어 있는지 검증하는 테스트를 수정/추가합니다.

- [ ] **Step 3: 커밋**
```bash
git add src/api/health.rs
git commit -m "feat(health): enhance readiness check with DB and backup metrics"
```
