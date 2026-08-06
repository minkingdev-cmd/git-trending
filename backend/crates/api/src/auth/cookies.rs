pub fn access_cookie(jwt: &str, secure: bool) -> String {
    format!(
        "access_token={jwt}; HttpOnly; SameSite=Lax; Path=/; Max-Age=900{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn refresh_cookie(token: &str, secure: bool) -> String {
    format!(
        "refresh_token={token}; HttpOnly; SameSite=Lax; Path=/api/auth; Max-Age=2592000{}",
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookies() -> Vec<String> {
    vec![
        "access_token=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string(),
        "refresh_token=; HttpOnly; SameSite=Lax; Path=/api/auth; Max-Age=0".to_string(),
    ]
}

pub fn read_cookie(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k.trim() == name {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_access_cookie() {
        let c = access_cookie("jwt.value", false);
        assert!(c.starts_with(
            "access_token=jwt.value; HttpOnly; SameSite=Lax; Path=/; Max-Age=900"
        ));
        assert!(!c.contains("Secure"));
        assert!(access_cookie("j", true).ends_with("; Secure"));
    }

    #[test]
    fn refresh_cookie_scoped_to_auth_path() {
        assert!(refresh_cookie("t", false).contains("Path=/api/auth"));
    }

    #[test]
    fn reads_named_cookie() {
        let header = "a=1; access_token=xyz; b=2";
        assert_eq!(read_cookie(header, "access_token").as_deref(), Some("xyz"));
        assert_eq!(read_cookie(header, "missing"), None);
    }
}
