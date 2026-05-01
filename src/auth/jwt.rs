use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    #[serde(default = "default_app_id")]
    pub app_id: String,
    pub exp: i64,
    pub is_admin: bool,
}

fn default_app_id() -> String {
    crate::app_context::DEFAULT_APP_ID.to_string()
}

fn jwt_validation() -> Validation {
    let mut validation = Validation::default();
    validation.algorithms = vec![Algorithm::HS256];
    validation.validate_exp = true;
    validation
}

#[allow(dead_code)]
pub fn create_jwt(
    user_id: &str,
    is_admin: bool,
    secret: &str,
    expires_at: DateTime<Utc>,
) -> String {
    create_app_jwt(
        crate::app_context::DEFAULT_APP_ID,
        user_id,
        is_admin,
        secret,
        expires_at,
    )
}

pub fn create_app_jwt(
    app_id: &str,
    user_id: &str,
    is_admin: bool,
    secret: &str,
    expires_at: DateTime<Utc>,
) -> String {
    let claims = Claims {
        sub: user_id.to_owned(),
        app_id: app_id.to_owned(),
        exp: expires_at.timestamp(),
        is_admin,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .expect("JWT encode is infallible with valid Claims and non-empty secret")
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &jwt_validation(),
    )?;
    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use jsonwebtoken::Algorithm;

    #[test]
    fn test_jwt_flow() {
        let secret = "test_secret";
        let token = create_jwt("user123", true, secret, Utc::now() + Duration::minutes(15));
        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.sub, "user123");
        assert_eq!(claims.app_id, crate::app_context::DEFAULT_APP_ID);
        assert!(claims.is_admin);
    }

    #[test]
    fn test_app_jwt_flow() {
        let secret = "test_secret";
        let token = create_app_jwt(
            "app_a",
            "user123",
            false,
            secret,
            Utc::now() + Duration::minutes(15),
        );
        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.sub, "user123");
        assert_eq!(claims.app_id, "app_a");
        assert!(!claims.is_admin);
    }

    #[test]
    fn test_validation_is_pinned_to_hs256() {
        let validation = jwt_validation();
        assert_eq!(validation.algorithms, vec![Algorithm::HS256]);
        assert!(validation.validate_exp);
    }
}
