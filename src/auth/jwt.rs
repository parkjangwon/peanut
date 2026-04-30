use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: i64,
    pub is_admin: bool,
}

fn jwt_validation() -> Validation {
    let mut validation = Validation::default();
    validation.algorithms = vec![Algorithm::HS256];
    validation.validate_exp = true;
    validation
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
        assert!(claims.is_admin);
    }

    #[test]
    fn test_validation_is_pinned_to_hs256() {
        let validation = jwt_validation();
        assert_eq!(validation.algorithms, vec![Algorithm::HS256]);
        assert!(validation.validate_exp);
    }
}
