use sha2::{Digest, Sha256};

pub const DEFAULT_LANGUAGES: &str =
    "TypeScript,JavaScript,Python,Java,Go,Rust,C,C++,C#,PHP,Ruby,Swift,Kotlin,Shell";

/// Domain separator mixed into the JWT-derived encryption key.
pub const TOKEN_KEY_DOMAIN_SEP: &str = "ght-github-token-v1";

#[derive(Debug, Clone)]
pub struct Settings {
    pub database_url: String,
    pub jwt_secret: String,
    pub github_token: Option<String>,
    /// GitHub REST API base (override with `GITHUB_API_BASE` for tests / proxies).
    pub github_api_base: String,
    pub languages: Vec<String>,
    pub collect_time: String,
    pub cookie_secure: bool,
    /// AES-256 key for per-user GitHub PAT ciphertext.
    ///
    /// From `TOKEN_ENCRYPTION_KEY` when set (see [`parse_token_encryption_key`]),
    /// otherwise `SHA-256(JWT_SECRET || "ght-github-token-v1")`.
    pub token_encryption_key: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(String),
    #[error("invalid COLLECT_TIME {0:?}, expected HH:MM")]
    BadCollectTime(String),
    #[error(
        "invalid TOKEN_ENCRYPTION_KEY: expected 64 hex chars or standard/URL-safe base64 of 32 bytes"
    )]
    BadTokenEncryptionKey,
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(|k| std::env::var(k).ok())
    }

    pub fn from_map<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, ConfigError> {
        let database_url = get("DATABASE_URL").ok_or_else(|| ConfigError::Missing("DATABASE_URL".into()))?;
        let jwt_secret = get("JWT_SECRET").ok_or_else(|| ConfigError::Missing("JWT_SECRET".into()))?;
        let languages_raw = get("LANGUAGES").unwrap_or_else(|| DEFAULT_LANGUAGES.to_string());
        let collect_time = get("COLLECT_TIME").unwrap_or_else(|| "09:00".to_string());
        validate_collect_time(&collect_time)?;
        let token_encryption_key = match get("TOKEN_ENCRYPTION_KEY").filter(|s| !s.is_empty()) {
            Some(raw) => parse_token_encryption_key(&raw)?,
            None => derive_token_encryption_key(&jwt_secret),
        };
        Ok(Settings {
            database_url,
            jwt_secret,
            github_token: get("GITHUB_TOKEN").filter(|s| !s.is_empty()),
            github_api_base: get("GITHUB_API_BASE")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "https://api.github.com".into()),
            languages: parse_languages(&languages_raw),
            collect_time,
            cookie_secure: get("COOKIE_SECURE").map(|v| v == "true").unwrap_or(false),
            token_encryption_key,
        })
    }
}

/// `SHA-256(jwt_secret_bytes || domain_sep_bytes)` → 32-byte AES key.
pub fn derive_token_encryption_key(jwt_secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(jwt_secret.as_bytes());
    hasher.update(TOKEN_KEY_DOMAIN_SEP.as_bytes());
    hasher.finalize().into()
}

/// Parse `TOKEN_ENCRYPTION_KEY`.
///
/// Accepted formats (exactly 32 bytes after decode):
/// - **64 hex characters** (case-insensitive), optional `0x` prefix
/// - **standard or URL-safe base64** (with or without padding) of 32 raw bytes
pub fn parse_token_encryption_key(raw: &str) -> Result<[u8; 32], ConfigError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ConfigError::BadTokenEncryptionKey);
    }

    // Hex: optional 0x prefix, exactly 64 hex digits.
    let hex_candidate = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if hex_candidate.len() == 64 && hex_candidate.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = [0u8; 32];
        for i in 0..32 {
            let byte = u8::from_str_radix(&hex_candidate[i * 2..i * 2 + 2], 16)
                .map_err(|_| ConfigError::BadTokenEncryptionKey)?;
            out[i] = byte;
        }
        return Ok(out);
    }

    use base64::Engine;
    let engines: &[&dyn Fn(&str) -> Result<Vec<u8>, base64::DecodeError>] = &[
        &|s| base64::engine::general_purpose::STANDARD.decode(s),
        &|s| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s),
        &|s| base64::engine::general_purpose::URL_SAFE.decode(s),
        &|s| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s),
    ];
    for decode in engines {
        if let Ok(bytes) = decode(s) {
            if let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) {
                return Ok(arr);
            }
        }
    }

    Err(ConfigError::BadTokenEncryptionKey)
}

pub fn parse_languages(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn validate_collect_time(t: &str) -> Result<(), ConfigError> {
    let ok = t.len() == 5
        && t.as_bytes()[2] == b':'
        && t[..2].chars().all(|c| c.is_ascii_digit())
        && t[3..].chars().all(|c| c.is_ascii_digit())
        && t[..2].parse::<u32>().map(|h| h < 24).unwrap_or(false)
        && t[3..].parse::<u32>().map(|m| m < 60).unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ConfigError::BadCollectTime(t.to_string()))
    }
}

pub fn collect_time_parts(t: &str) -> Result<(u32, u32), ConfigError> {
    validate_collect_time(t)?;
    let hour: u32 = t[..2]
        .parse()
        .map_err(|_| ConfigError::BadCollectTime(t.to_string()))?;
    let minute: u32 = t[3..]
        .parse()
        .map_err(|_| ConfigError::BadCollectTime(t.to_string()))?;
    Ok((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_languages_trims_and_drops_empty() {
        assert_eq!(parse_languages("Rust, Go , ,Python"), vec!["Rust", "Go", "Python"]);
    }

    #[test]
    fn defaults_applied_when_optional_missing() {
        let s = Settings::from_map(|k| match k {
            "DATABASE_URL" => Some("postgres://x".into()),
            "JWT_SECRET" => Some("secret".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(s.collect_time, "09:00");
        assert!(s.languages.contains(&"Rust".to_string()));
        assert!(!s.cookie_secure);
        assert!(s.github_token.is_none());
        assert_eq!(s.github_api_base, "https://api.github.com");
        assert_eq!(s.token_encryption_key, derive_token_encryption_key("secret"));
    }

    #[test]
    fn token_encryption_key_from_hex_env() {
        let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let s = Settings::from_map(|k| match k {
            "DATABASE_URL" => Some("postgres://x".into()),
            "JWT_SECRET" => Some("secret".into()),
            "TOKEN_ENCRYPTION_KEY" => Some(hex.into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            s.token_encryption_key,
            parse_token_encryption_key(hex).unwrap()
        );
        // Explicit key overrides JWT-derived default.
        assert_ne!(s.token_encryption_key, derive_token_encryption_key("secret"));
    }

    #[test]
    fn parse_token_encryption_key_accepts_hex_and_base64() {
        let key = [0xab_u8; 32];
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(parse_token_encryption_key(&hex).unwrap(), key);
        assert_eq!(parse_token_encryption_key(&format!("0x{hex}")).unwrap(), key);

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(key);
        assert_eq!(parse_token_encryption_key(&b64).unwrap(), key);
        let b64url = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key);
        assert_eq!(parse_token_encryption_key(&b64url).unwrap(), key);
    }

    #[test]
    fn parse_token_encryption_key_rejects_bad() {
        assert!(matches!(
            parse_token_encryption_key("too-short"),
            Err(ConfigError::BadTokenEncryptionKey)
        ));
        assert!(matches!(
            parse_token_encryption_key("zz"),
            Err(ConfigError::BadTokenEncryptionKey)
        ));
    }

    #[test]
    fn missing_required_is_error() {
        assert!(matches!(Settings::from_map(|_| None).unwrap_err(), ConfigError::Missing(_)));
    }

    #[test]
    fn validates_collect_time() {
        assert!(validate_collect_time("09:00").is_ok());
        assert!(validate_collect_time("25:00").is_err());
        assert!(validate_collect_time("9:00").is_err());
        assert!(validate_collect_time("09:60").is_err());
        assert!(validate_collect_time("0900").is_err());
    }

    #[test]
    fn collect_time_parts_splits_hh_mm() {
        assert_eq!(collect_time_parts("09:05").unwrap(), (9, 5));
        assert!(collect_time_parts("09:60").is_err());
    }

    #[test]
    fn collect_time_parts_boundary_ok() {
        assert_eq!(collect_time_parts("00:00").unwrap(), (0, 0));
        assert_eq!(collect_time_parts("23:59").unwrap(), (23, 59));
    }

    #[test]
    fn collect_time_parts_rejects_invalid() {
        for bad in ["24:00", "12:60", "", "9:00", "0900"] {
            assert!(
                collect_time_parts(bad).is_err(),
                "expected err for {bad:?}"
            );
        }
    }
}
