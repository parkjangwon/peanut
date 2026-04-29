use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path as FsPath, PathBuf};

use crate::{api::common::json_error, auth::jwt::Claims};

pub const RESTORE_MARKER_FILE: &str = crate::db::RESTORE_MARKER_FILE;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSummary {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupsResponse {
    pub backups: Vec<BackupSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResponse {
    pub backup: BackupSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreBackupResponse {
    pub message: String,
    pub backup_name: String,
    pub restart_required: bool,
}

pub async fn list_backups(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    match list_backup_summaries(&state.database_url).await {
        Ok(backups) => (StatusCode::OK, Json(BackupsResponse { backups })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to list backups"),
    }
}

pub async fn create_backup(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let backup_path = match crate::db::backup_db(&state.pool, &state.database_url).await {
        Ok(path) => path,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to create backup"),
    };

    match backup_summary_from_path(PathBuf::from(backup_path)).await {
        Ok(backup) => (StatusCode::CREATED, Json(BackupResponse { backup })).into_response(),
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to inspect backup"),
    }
}

pub async fn download_backup(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(backup_name): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let backup_path = match resolve_backup_path(&state.database_url, &backup_name) {
        Ok(path) => path,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    match tokio::fs::read(&backup_path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", backup_name),
                ),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "backup not found")
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read backup"),
    }
}

pub async fn restore_backup(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(backup_name): Path<String>,
) -> Response {
    if !claims.is_admin {
        return json_error(StatusCode::FORBIDDEN, "admin access required");
    }

    let backup_path = match resolve_backup_path(&state.database_url, &backup_name) {
        Ok(path) => path,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    if !backup_path.exists() {
        return json_error(StatusCode::NOT_FOUND, "backup not found");
    }

    let Some(db_dir) = backup_path.parent() else {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "invalid backup path");
    };

    match write_restore_marker(db_dir, &backup_name).await {
        Ok(()) => (
            StatusCode::OK,
            Json(RestoreBackupResponse {
                message: "backup restore scheduled; restart Peanut to apply it".to_string(),
                backup_name,
                restart_required: true,
            }),
        )
            .into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to schedule backup restore",
        ),
    }
}

async fn list_backup_summaries(database_url: &str) -> std::io::Result<Vec<BackupSummary>> {
    let db_path = FsPath::new(crate::db::extract_db_path(database_url));
    let db_dir = db_path.parent().unwrap_or(FsPath::new("."));
    let db_filename = db_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("peanut.db");
    let prefix = format!("{db_filename}.");

    let mut backups = Vec::new();
    let mut entries = tokio::fs::read_dir(db_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with(&prefix) && file_name.ends_with(".backup") {
            backups.push(backup_summary_from_path(entry.path()).await?);
        }
    }
    backups.sort_by(|left, right| right.modified_at.cmp(&left.modified_at));
    Ok(backups)
}

async fn backup_summary_from_path(path: PathBuf) -> std::io::Result<BackupSummary> {
    let metadata = tokio::fs::metadata(&path).await?;
    let modified_at = metadata
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "1970-01-01T00:00:00+00:00".to_string());
    Ok(BackupSummary {
        name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string(),
        size_bytes: metadata.len(),
        modified_at,
    })
}

fn resolve_backup_path(database_url: &str, backup_name: &str) -> Result<PathBuf, String> {
    validate_backup_name(backup_name)?;
    let db_path = FsPath::new(crate::db::extract_db_path(database_url));
    let db_dir = db_path.parent().unwrap_or(FsPath::new("."));
    Ok(db_dir.join(backup_name))
}

fn validate_backup_name(backup_name: &str) -> Result<(), String> {
    if backup_name.trim().is_empty()
        || backup_name.contains('/')
        || backup_name.contains('\\')
        || backup_name.contains("..")
        || !backup_name.ends_with(".backup")
    {
        return Err("invalid backup name".to_string());
    }
    Ok(())
}

async fn write_restore_marker(db_dir: &FsPath, backup_name: &str) -> std::io::Result<()> {
    validate_backup_name(backup_name).map_err(std::io::Error::other)?;
    tokio::fs::write(db_dir.join(RESTORE_MARKER_FILE), backup_name).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::StatusCode};

    #[test]
    fn test_validate_backup_name_rejects_path_traversal() {
        assert!(validate_backup_name("../peanut.db.backup").is_err());
        assert!(validate_backup_name("nested/peanut.db.backup").is_err());
    }

    #[tokio::test]
    async fn test_restore_marker_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        write_restore_marker(dir.path(), "peanut.db.20260429_010203.backup")
            .await
            .unwrap();

        let marker = tokio::fs::read_to_string(dir.path().join(RESTORE_MARKER_FILE))
            .await
            .unwrap();
        assert_eq!(marker, "peanut.db.20260429_010203.backup");
    }

    #[tokio::test]
    async fn test_non_admin_cannot_list_backups() {
        let (state, _dir) = crate::test_support::make_test_state().await;
        let response = list_backups(
            State(state),
            Extension(Claims {
                sub: "member".to_string(),
                exp: 9999999999,
                is_admin: false,
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_backup_create_and_list_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let database_url = format!("sqlite://{}", dir.path().join("peanut.db").display());
        let pool = crate::db::init_db(&database_url).await.unwrap();
        let mut state = crate::test_support::make_test_state().await.0;
        state.pool = pool;
        state.database_url = std::sync::Arc::new(database_url);

        let claims = Extension(Claims {
            sub: "admin".to_string(),
            exp: 9999999999,
            is_admin: true,
        });
        let create_response = create_backup(State(state.clone()), claims.clone()).await;
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let list_response = list_backups(State(state), claims).await;
        assert_eq!(list_response.status(), StatusCode::OK);
        let body: BackupsResponse = crate::test_support::response_json(list_response).await;
        assert_eq!(body.backups.len(), 1);
        assert!(body.backups[0].name.ends_with(".backup"));
    }
}
