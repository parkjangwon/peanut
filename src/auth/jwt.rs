use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: i64,
    pub is_admin: bool,
}

pub fn create_jwt(
    user_id: &str,
    is_admin: bool,
    secret: &str,
    expires_at: DateTime<Utc>,
) -> String {
    let claims = Claims {
        sub: user_id.to_owned(),
        exp: expires_at.timestamp(),
        is_admin,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .unwrap()
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_jwt_flow() {
        let secret = "test_secret";
        let token = create_jwt("user123", true, secret, Utc::now() + Duration::minutes(15));
        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.sub, "user123");
        assert!(claims.is_admin);
    }
}
