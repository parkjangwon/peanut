use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub const DEFAULT_DATABASE_URL: &str = "sqlite://peanut.db";
pub const DEFAULT_STORAGE_DIR: &str = "data/storage";
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";
pub const DEFAULT_MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;
pub const DEFAULT_MULTIPART_STALE_HOURS: u64 = 24;
pub const DEFAULT_MULTIPART_CLEANUP_INTERVAL_SECONDS: u64 = 3600;
pub const DEFAULT_FUNCTIONS_MAX_CONCURRENT: usize = 4;
pub const DEFAULT_FUNCTIONS_MEMORY_MB: usize = 128;
pub const DEFAULT_FUNCTIONS_MAX_SOURCE_BYTES: usize = 256 * 1024;
pub const DEFAULT_FUNCTIONS_MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordResetDelivery {
    Inline,
    Log,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailConfig {
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password: String,
    pub smtp_from: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub database_url: String,
    pub storage_dir: PathBuf,
    pub bind_addr: SocketAddr,
    pub jwt_secret: String,
    pub mail: MailConfig,
    pub max_upload_bytes: usize,
    pub password_reset_delivery: PasswordResetDelivery,
    pub auth_allowed_origins: Vec<String>,
    pub auth_allowed_client_ids: Vec<String>,
    pub push_ntfy_enabled: bool,
    pub push_web_push_enabled: bool,
    pub functions_enabled: bool,
    pub backup_on_startup: bool,
    pub trust_proxy_headers: bool,
    pub multipart_stale_hours: u64,
    pub multipart_cleanup_interval_seconds: u64,
    pub functions_allow_network: bool,
    pub functions_work_dir: PathBuf,
    pub functions_max_concurrent: usize,
    pub functions_memory_mb: usize,
    pub functions_max_source_bytes: usize,
    pub functions_max_output_bytes: usize,
    pub functions_secrets_master_key: String,
}

pub fn load_config_from_env() -> Result<AppConfig, String> {
    let values: HashMap<String, String> = env::vars().collect();
    load_config_from_map(&values)
}

fn load_config_from_map(values: &HashMap<String, String>) -> Result<AppConfig, String> {
    let database_url = values
        .get("DATABASE_URL")
        .cloned()
        .unwrap_or_else(|| DEFAULT_DATABASE_URL.to_string());
    if !database_url.starts_with("sqlite:") {
        return Err("DATABASE_URL must use a sqlite: URL".to_string());
    }

    let storage_dir_raw = values
        .get("STORAGE_DIR")
        .cloned()
        .unwrap_or_else(|| DEFAULT_STORAGE_DIR.to_string());
    let storage_dir = PathBuf::from(storage_dir_raw.trim());
    if storage_dir.as_os_str().is_empty() {
        return Err("STORAGE_DIR must not be empty".to_string());
    }

    let bind_addr_raw = values
        .get("BIND_ADDR")
        .cloned()
        .unwrap_or_else(|| DEFAULT_BIND_ADDR.to_string());
    let bind_addr = bind_addr_raw
        .parse::<SocketAddr>()
        .map_err(|_| "BIND_ADDR must be a valid socket address".to_string())?;

    let jwt_secret = values
        .get("JWT_SECRET")
        .cloned()
        .ok_or_else(|| "JWT_SECRET must be set before starting Peanut".to_string())?;
    if jwt_secret.trim().is_empty() {
        return Err("JWT_SECRET must not be empty".to_string());
    }

    let max_upload_bytes = match values.get("MAX_UPLOAD_BYTES") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "MAX_UPLOAD_BYTES must be a positive integer".to_string())?;
            if parsed == 0 {
                return Err("MAX_UPLOAD_BYTES must be greater than zero".to_string());
            }
            parsed
        }
        None => DEFAULT_MAX_UPLOAD_BYTES,
    };

    let password_reset_delivery = match values
        .get("PASSWORD_RESET_DELIVERY")
        .map(|value| value.trim())
        .unwrap_or("inline")
    {
        "inline" => PasswordResetDelivery::Inline,
        "log" => PasswordResetDelivery::Log,
        _ => return Err("PASSWORD_RESET_DELIVERY must be either 'inline' or 'log'".to_string()),
    };

    let auth_allowed_origins = parse_origin_policy_list(values, "AUTH_ALLOWED_ORIGINS")?;
    let auth_allowed_client_ids = parse_client_id_policy_list(values, "AUTH_ALLOWED_CLIENT_IDS")?;

    let push_ntfy_enabled = values.get("NTFY_BASE_URL").is_some();
    let push_web_push_enabled = values.get("WEB_PUSH_VAPID_PRIVATE_KEY").is_some();
    let functions_enabled = parse_bool_setting(values, "FUNCTIONS_ENABLED", true)?;
    let backup_on_startup = parse_bool_setting(values, "BACKUP_ON_STARTUP", false)?;
    let trust_proxy_headers = parse_bool_setting(values, "TRUST_PROXY_HEADERS", false)?;
    let multipart_stale_hours = parse_positive_u64_setting(
        values,
        "MULTIPART_STALE_HOURS",
        DEFAULT_MULTIPART_STALE_HOURS,
    )?;
    let multipart_cleanup_interval_seconds = parse_positive_u64_setting(
        values,
        "MULTIPART_CLEANUP_INTERVAL_SECONDS",
        DEFAULT_MULTIPART_CLEANUP_INTERVAL_SECONDS,
    )?;
    let functions_allow_network = parse_bool_setting(values, "FUNCTIONS_ALLOW_NETWORK", false)?;
    let functions_work_dir = values
        .get("FUNCTIONS_WORK_DIR")
        .map(|value| PathBuf::from(value.trim()))
        .unwrap_or_else(|| env::temp_dir().join("peanut-functions"));
    if functions_work_dir.as_os_str().is_empty() {
        return Err("FUNCTIONS_WORK_DIR must not be empty".to_string());
    }
    validate_functions_work_dir(&functions_work_dir, &database_url)?;
    let functions_max_concurrent = parse_positive_usize_setting(
        values,
        "FUNCTIONS_MAX_CONCURRENT",
        DEFAULT_FUNCTIONS_MAX_CONCURRENT,
    )?;
    let functions_memory_mb =
        parse_positive_usize_setting(values, "FUNCTIONS_MEMORY_MB", DEFAULT_FUNCTIONS_MEMORY_MB)?;
    let functions_max_source_bytes = parse_positive_usize_setting(
        values,
        "FUNCTIONS_MAX_SOURCE_BYTES",
        DEFAULT_FUNCTIONS_MAX_SOURCE_BYTES,
    )?;
    let functions_max_output_bytes = parse_positive_usize_setting(
        values,
        "FUNCTIONS_MAX_OUTPUT_BYTES",
        DEFAULT_FUNCTIONS_MAX_OUTPUT_BYTES,
    )?;
    let functions_secrets_master_key = values
        .get("FUNCTIONS_SECRETS_MASTER_KEY")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| jwt_secret.clone());
    let mail = load_mail_config(values)?;

    Ok(AppConfig {
        database_url,
        storage_dir,
        bind_addr,
        jwt_secret,
        mail,
        max_upload_bytes,
        password_reset_delivery,
        auth_allowed_origins,
        auth_allowed_client_ids,
        push_ntfy_enabled,
        push_web_push_enabled,
        functions_enabled,
        backup_on_startup,
        trust_proxy_headers,
        multipart_stale_hours,
        multipart_cleanup_interval_seconds,
        functions_allow_network,
        functions_work_dir,
        functions_max_concurrent,
        functions_memory_mb,
        functions_max_source_bytes,
        functions_max_output_bytes,
        functions_secrets_master_key,
    })
}

fn parse_bool_setting(
    values: &HashMap<String, String>,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    match values
        .get(key)
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) if value == "true" || value == "1" || value == "yes" => Ok(true),
        Some(value) if value == "false" || value == "0" || value == "no" => Ok(false),
        Some(_) => Err(format!("{key} must be true or false")),
        None => Ok(default),
    }
}

fn parse_positive_u64_setting(
    values: &HashMap<String, String>,
    key: &str,
    default: u64,
) -> Result<u64, String> {
    match values.get(key) {
        Some(value) => {
            let parsed = value
                .parse::<u64>()
                .map_err(|_| format!("{key} must be a positive integer"))?;
            if parsed == 0 {
                return Err(format!("{key} must be greater than zero"));
            }
            Ok(parsed)
        }
        None => Ok(default),
    }
}

fn parse_positive_usize_setting(
    values: &HashMap<String, String>,
    key: &str,
    default: usize,
) -> Result<usize, String> {
    match values.get(key) {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| format!("{key} must be a positive integer"))?;
            if parsed == 0 {
                return Err(format!("{key} must be greater than zero"));
            }
            Ok(parsed)
        }
        None => Ok(default),
    }
}

fn validate_functions_work_dir(path: &Path, database_url: &str) -> Result<(), String> {
    if path == Path::new("/") {
        return Err("FUNCTIONS_WORK_DIR must not be the filesystem root".to_string());
    }

    if let Ok(home) = env::var("HOME") {
        if !home.trim().is_empty() && path == Path::new(&home) {
            return Err("FUNCTIONS_WORK_DIR must not be the user home directory".to_string());
        }
    }

    if let Some(db_path) = sqlite_db_path(database_url) {
        let db_dir = db_path.parent().unwrap_or_else(|| Path::new("."));
        if path == db_path || path == db_dir {
            return Err(
                "FUNCTIONS_WORK_DIR must not be the database path or directory".to_string(),
            );
        }
    }

    Ok(())
}

fn sqlite_db_path(database_url: &str) -> Option<PathBuf> {
    if database_url == "sqlite::memory:" {
        return None;
    }
    database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .map(PathBuf::from)
}

fn parse_origin_policy_list(
    values: &HashMap<String, String>,
    key: &str,
) -> Result<Vec<String>, String> {
    let entries = parse_csv_policy_list(values, key);
    for origin in &entries {
        if !(origin.starts_with("http://") || origin.starts_with("https://")) {
            return Err(format!("{key} entries must start with http:// or https://"));
        }
    }
    Ok(entries)
}

fn parse_client_id_policy_list(
    values: &HashMap<String, String>,
    key: &str,
) -> Result<Vec<String>, String> {
    let entries = parse_csv_policy_list(values, key);
    for client_id in &entries {
        if !client_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(format!(
                "{key} entries may only contain letters, digits, hyphens, and underscores"
            ));
        }
    }
    Ok(entries)
}

fn load_mail_config(values: &HashMap<String, String>) -> Result<MailConfig, String> {
    let smtp_host = values
        .get("SMTP_HOST")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let smtp_port = values
        .get("SMTP_PORT")
        .map(|value| value.trim().to_string())
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(587);
    let smtp_user = values
        .get("SMTP_USER")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let smtp_password = values
        .get("SMTP_PASSWORD")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let smtp_from = values
        .get("SMTP_FROM")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let smtp_enabled = smtp_host.is_some()
        && smtp_user.is_some()
        && smtp_password.is_some()
        && smtp_from.is_some();

    Ok(MailConfig {
        smtp_enabled,
        smtp_host: smtp_host.unwrap_or_default(),
        smtp_port,
        smtp_user: smtp_user.unwrap_or_default(),
        smtp_password: smtp_password.unwrap_or_default(),
        smtp_from: smtp_from.unwrap_or_default(),
    })
}

fn parse_csv_policy_list(values: &HashMap<String, String>, key: &str) -> Vec<String> {
    values
        .get(key)
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn test_load_config_from_env_uses_defaults_for_optional_values() {
        let values = config(&[("JWT_SECRET", "test-secret")]);

        let config = load_config_from_map(&values).unwrap();
        assert_eq!(config.database_url, DEFAULT_DATABASE_URL);
        assert_eq!(config.storage_dir, PathBuf::from(DEFAULT_STORAGE_DIR));
        assert_eq!(
            config.bind_addr,
            DEFAULT_BIND_ADDR.parse::<SocketAddr>().unwrap()
        );
        assert_eq!(config.max_upload_bytes, DEFAULT_MAX_UPLOAD_BYTES);
        assert_eq!(
            config.password_reset_delivery,
            PasswordResetDelivery::Inline
        );
        assert_eq!(config.auth_allowed_origins, Vec::<String>::new());
        assert_eq!(config.auth_allowed_client_ids, Vec::<String>::new());
        assert_eq!(config.jwt_secret, "test-secret");
        assert!(!config.push_ntfy_enabled);
        assert!(!config.push_web_push_enabled);
        assert!(config.functions_enabled);
        assert!(!config.backup_on_startup);
        assert!(!config.trust_proxy_headers);
        assert_eq!(config.multipart_stale_hours, 24);
        assert_eq!(config.multipart_cleanup_interval_seconds, 3600);
        assert!(!config.functions_allow_network);
        assert!(config.functions_work_dir.ends_with("peanut-functions"));
        assert_eq!(
            config.functions_max_concurrent,
            DEFAULT_FUNCTIONS_MAX_CONCURRENT
        );
        assert_eq!(config.functions_memory_mb, DEFAULT_FUNCTIONS_MEMORY_MB);
        assert_eq!(
            config.functions_max_source_bytes,
            DEFAULT_FUNCTIONS_MAX_SOURCE_BYTES
        );
        assert_eq!(
            config.functions_max_output_bytes,
            DEFAULT_FUNCTIONS_MAX_OUTPUT_BYTES
        );
        assert_eq!(config.functions_secrets_master_key, "test-secret");
    }

    #[test]
    fn test_load_config_from_env_rejects_missing_jwt_secret() {
        let values = config(&[]);
        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("JWT_SECRET"));
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_bind_addr() {
        let values = config(&[("JWT_SECRET", "test-secret"), ("BIND_ADDR", "not-an-addr")]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("BIND_ADDR"));
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_database_url_scheme() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("DATABASE_URL", "postgres://example"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("sqlite:"));
    }

    #[test]
    fn test_load_config_from_env_rejects_zero_max_upload_bytes() {
        let values = config(&[("JWT_SECRET", "test-secret"), ("MAX_UPLOAD_BYTES", "0")]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("greater than zero"));
    }

    #[test]
    fn test_load_config_from_env_supports_log_password_reset_delivery() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("PASSWORD_RESET_DELIVERY", "log"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert_eq!(config.password_reset_delivery, PasswordResetDelivery::Log);
    }

    #[test]
    fn test_load_config_from_env_rejects_unknown_password_reset_delivery() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("PASSWORD_RESET_DELIVERY", "webhook"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("PASSWORD_RESET_DELIVERY"));
    }

    #[test]
    fn test_load_config_from_env_parses_auth_origin_and_client_policy() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            (
                "AUTH_ALLOWED_ORIGINS",
                "https://app.example.com, https://admin.example.com",
            ),
            ("AUTH_ALLOWED_CLIENT_IDS", "web-app, admin-console"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert_eq!(
            config.auth_allowed_origins,
            vec![
                "https://app.example.com".to_string(),
                "https://admin.example.com".to_string(),
            ]
        );
        assert_eq!(
            config.auth_allowed_client_ids,
            vec!["web-app".to_string(), "admin-console".to_string()]
        );
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_auth_origin_policy() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("AUTH_ALLOWED_ORIGINS", "app.example.com"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("AUTH_ALLOWED_ORIGINS"));
    }

    #[test]
    fn test_load_config_from_env_detects_push_status() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("NTFY_BASE_URL", "https://ntfy.sh/topic"),
            ("WEB_PUSH_VAPID_PRIVATE_KEY", "secret-key"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert!(config.push_ntfy_enabled);
        assert!(config.push_web_push_enabled);
    }

    #[test]
    fn test_load_config_from_env_parses_function_runtime_switch() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("FUNCTIONS_ENABLED", "false"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert!(!config.functions_enabled);
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_function_runtime_switch() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("FUNCTIONS_ENABLED", "sometimes"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("FUNCTIONS_ENABLED"));
    }

    #[test]
    fn test_load_config_from_env_parses_startup_backup_switch() {
        let values = config(&[("JWT_SECRET", "test-secret"), ("BACKUP_ON_STARTUP", "true")]);

        let config = load_config_from_map(&values).unwrap();
        assert!(config.backup_on_startup);
    }

    #[test]
    fn test_load_config_from_env_parses_trust_proxy_headers_switch() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("TRUST_PROXY_HEADERS", "true"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert!(config.trust_proxy_headers);
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_trust_proxy_headers_switch() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("TRUST_PROXY_HEADERS", "maybe"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("TRUST_PROXY_HEADERS"));
    }

    #[test]
    fn test_load_config_from_env_parses_multipart_cleanup_settings() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("MULTIPART_STALE_HOURS", "12"),
            ("MULTIPART_CLEANUP_INTERVAL_SECONDS", "600"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert_eq!(config.multipart_stale_hours, 12);
        assert_eq!(config.multipart_cleanup_interval_seconds, 600);
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_multipart_cleanup_settings() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("MULTIPART_STALE_HOURS", "0"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("MULTIPART_STALE_HOURS"));
    }

    #[test]
    fn test_load_config_from_env_parses_function_isolation_settings() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("FUNCTIONS_ALLOW_NETWORK", "true"),
            ("FUNCTIONS_WORK_DIR", "/tmp/peanut-test-functions"),
            ("FUNCTIONS_MAX_CONCURRENT", "8"),
            ("FUNCTIONS_MEMORY_MB", "256"),
            ("FUNCTIONS_MAX_SOURCE_BYTES", "1024"),
            ("FUNCTIONS_MAX_OUTPUT_BYTES", "2048"),
            ("FUNCTIONS_SECRETS_MASTER_KEY", "dedicated-functions-key"),
        ]);

        let config = load_config_from_map(&values).unwrap();
        assert!(config.functions_allow_network);
        assert_eq!(
            config.functions_work_dir,
            PathBuf::from("/tmp/peanut-test-functions")
        );
        assert_eq!(config.functions_max_concurrent, 8);
        assert_eq!(config.functions_memory_mb, 256);
        assert_eq!(config.functions_max_source_bytes, 1024);
        assert_eq!(config.functions_max_output_bytes, 2048);
        assert_eq!(
            config.functions_secrets_master_key,
            "dedicated-functions-key"
        );
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_functions_max_concurrent() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("FUNCTIONS_MAX_CONCURRENT", "0"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("FUNCTIONS_MAX_CONCURRENT"));
    }

    #[test]
    fn test_load_config_from_env_rejects_invalid_function_limits() {
        for key in [
            "FUNCTIONS_MEMORY_MB",
            "FUNCTIONS_MAX_SOURCE_BYTES",
            "FUNCTIONS_MAX_OUTPUT_BYTES",
        ] {
            let values = config(&[("JWT_SECRET", "test-secret"), (key, "0")]);
            let error = load_config_from_map(&values).unwrap_err();
            assert!(error.contains(key));
        }
    }

    #[test]
    fn test_load_config_from_env_rejects_dangerous_functions_work_dir() {
        let values = config(&[
            ("JWT_SECRET", "test-secret"),
            ("DATABASE_URL", "sqlite:///tmp/peanut.db"),
            ("FUNCTIONS_WORK_DIR", "/tmp"),
        ]);

        let error = load_config_from_map(&values).unwrap_err();
        assert!(error.contains("FUNCTIONS_WORK_DIR"));
    }
}
