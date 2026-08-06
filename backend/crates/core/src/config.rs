pub const DEFAULT_LANGUAGES: &str =
    "TypeScript,JavaScript,Python,Java,Go,Rust,C,C++,C#,PHP,Ruby,Swift,Kotlin,Shell";

#[derive(Debug, Clone)]
pub struct Settings {
    pub database_url: String,
    pub jwt_secret: String,
    pub github_token: Option<String>,
    pub languages: Vec<String>,
    pub collect_time: String,
    pub cookie_secure: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(String),
    #[error("invalid COLLECT_TIME {0:?}, expected HH:MM")]
    BadCollectTime(String),
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
        Ok(Settings {
            database_url,
            jwt_secret,
            github_token: get("GITHUB_TOKEN").filter(|s| !s.is_empty()),
            languages: parse_languages(&languages_raw),
            collect_time,
            cookie_secure: get("COOKIE_SECURE").map(|v| v == "true").unwrap_or(false),
        })
    }
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
    Ok((t[..2].parse().unwrap(), t[3..].parse().unwrap()))
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
}
