pub const MIN_PASSWORD_LEN: usize = 8;
pub const MAX_PASSWORD_LEN: usize = 256;
pub const MIN_USERNAME_LEN: usize = 2;
pub const MAX_USERNAME_LEN: usize = 64;

pub fn hash_password(plain: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(plain, bcrypt::DEFAULT_COST)
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    bcrypt::verify(plain, hash).unwrap_or(false)
}

/// Validate username/password shape before hashing or DB work.
pub fn validate_credentials(username: &str, password: &str) -> Result<(), &'static str> {
    let u = username.trim();
    if u.len() < MIN_USERNAME_LEN || u.len() > MAX_USERNAME_LEN {
        return Err("username must be 2-64 characters");
    }
    if !u
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err("username may only contain letters, digits, _ - .");
    }
    if password.len() < MIN_PASSWORD_LEN || password.len() > MAX_PASSWORD_LEN {
        return Err("password must be 8-256 characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("s3cret-pw").unwrap();
        assert!(verify_password("s3cret-pw", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn validates_credentials() {
        assert!(validate_credentials("ab", "12345678").is_ok());
        assert!(validate_credentials("a", "12345678").is_err());
        assert!(validate_credentials("alice", "short").is_err());
        assert!(validate_credentials("bad name", "12345678").is_err());
    }
}
