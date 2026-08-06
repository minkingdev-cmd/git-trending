use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

pub const ACCESS_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub iat: i64,
    pub exp: i64,
}

pub fn issue_access(secret: &str, user_id: i64, username: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + ACCESS_TTL_SECS,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn verify_access(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_claims() {
        let token = issue_access("secret", 7, "alice").unwrap();
        let claims = verify_access("secret", &token).unwrap();
        assert_eq!(claims.sub, 7);
        assert_eq!(claims.username, "alice");
        assert_eq!(claims.exp - claims.iat, ACCESS_TTL_SECS);
    }

    #[test]
    fn wrong_secret_rejected() {
        let token = issue_access("secret", 7, "alice").unwrap();
        assert!(verify_access("other", &token).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let claims = Claims {
            sub: 1,
            username: "x".into(),
            iat: 0,
            exp: 1,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(b"secret"),
        )
        .unwrap();
        assert!(verify_access("secret", &token).is_err());
    }
}
