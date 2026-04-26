use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf};

pub const DEFAULT_DATABASE_URL: &str = "sqlite://peanut.db";
pub const DEFAULT_STORAGE_DIR: &str = "data/storage";
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";
pub const DEFAULT_MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordResetDelivery {
    Inline,
    Log,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub database_url: String,
    pub storage_dir: PathBuf,
    pub bind_addr: SocketAddr,
    pub jwt_secret: String,
    pub max_upload_bytes: usize,
    pub password_reset_delivery: PasswordResetDelivery,
    pub auth_allowed_origins: Vec<String>,
    pub auth_allowed_client_ids: Vec<String>,
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
        _ => {
            return Err(
                "PASSWORD_RESET_DELIVERY must be either 'inline' or 'log'".to_string(),
            )
        }
    };

    let auth_allowed_origins = parse_origin_policy_list(values, "AUTH_ALLOWED_ORIGINS")?;
    let auth_allowed_client_ids = parse_client_id_policy_list(values, "AUTH_ALLOWED_CLIENT_IDS")?;

    Ok(AppConfig {
        database_url,
        storage_dir,
        bind_addr,
        jwt_secret,
        max_upload_bytes,
        password_reset_delivery,
        auth_allowed_origins,
        auth_allowed_client_ids,
    })
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
        assert_eq!(config.password_reset_delivery, PasswordResetDelivery::Inline);
        assert_eq!(config.auth_allowed_origins, Vec::<String>::new());
        assert_eq!(config.auth_allowed_client_ids, Vec::<String>::new());
        assert_eq!(config.jwt_secret, "test-secret");
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
        let values = config(&[("JWT_SECRET", "test-secret"), ("DATABASE_URL", "postgres://example")]);

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
}
