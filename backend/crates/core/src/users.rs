use rand::RngCore;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone)]
pub struct InviteRow {
    pub code: String,
    pub max_uses: i32,
    pub used_count: i32,
    pub revoked: bool,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum InviteError {
    #[error("invite code not found")]
    NotFound,
    #[error("invite code revoked")]
    Revoked,
    #[error("invite code exhausted")]
    Exhausted,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UserError {
    #[error("username already taken")]
    Duplicate,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RegisterError {
    #[error("invite: {0}")]
    Invite(#[from] InviteError),
    #[error("username already taken")]
    DuplicateUsername,
    #[error("db: {0}")]
    Db(String),
}

impl From<sqlx::Error> for RegisterError {
    fn from(e: sqlx::Error) -> Self {
        RegisterError::Db(e.to_string())
    }
}

pub async fn create_user(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
    invite_id: Option<i64>,
) -> Result<i64, UserError> {
    let res = sqlx::query_scalar!(
        r#"INSERT INTO users (username, password_hash, created_by_invite)
           VALUES ($1, $2, $3)
           ON CONFLICT (username) DO NOTHING
           RETURNING id"#,
        username,
        password_hash,
        invite_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| UserError::Duplicate)?;
    res.ok_or(UserError::Duplicate)
}

pub async fn find_user_by_username(
    pool: &PgPool,
    username: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT id, username, password_hash FROM users WHERE username = $1",
        username
    )
    .fetch_optional(pool)
    .await?;
    Ok(rec.map(|r| UserRow {
        id: r.id,
        username: r.username,
        password_hash: r.password_hash,
    }))
}

pub async fn find_username_by_id(pool: &PgPool, user_id: i64) -> Result<Option<String>, sqlx::Error> {
    let rec = sqlx::query!("SELECT username FROM users WHERE id = $1", user_id)
        .fetch_optional(pool)
        .await?;
    Ok(rec.map(|r| r.username))
}

/// Bootstrap users (created via CLI, no invite) are treated as admins.
pub async fn is_bootstrap_admin(pool: &PgPool, user_id: i64) -> Result<bool, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT created_by_invite FROM users WHERE id = $1",
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(rec.map(|r| r.created_by_invite.is_none()).unwrap_or(false))
}

#[derive(Debug, Clone)]
pub struct UserListItem {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<UserListItem>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT id, username, created_by_invite, created_at
           FROM users
           ORDER BY id ASC"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| UserListItem {
            id: r.id,
            username: r.username,
            is_admin: r.created_by_invite.is_none(),
            created_at: r.created_at,
        })
        .collect())
}

pub fn generate_invite_code() -> String {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn create_invite(pool: &PgPool, max_uses: i32) -> Result<String, sqlx::Error> {
    let code = generate_invite_code();
    sqlx::query!(
        "INSERT INTO invite_codes (code, max_uses) VALUES ($1, $2)",
        code,
        max_uses
    )
    .execute(pool)
    .await?;
    Ok(code)
}

/// Atomically: lock invite code → validate → increment used_count → create user.
/// Rolls back on any failure.
pub async fn register_with_invite(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
    code: &str,
) -> Result<i64, RegisterError> {
    let mut tx = pool.begin().await?;

    let invite = sqlx::query!(
        "SELECT id, max_uses, used_count, revoked FROM invite_codes WHERE code = $1 FOR UPDATE",
        code
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(InviteError::NotFound)?;

    if invite.revoked {
        return Err(RegisterError::Invite(InviteError::Revoked));
    }
    if invite.used_count >= invite.max_uses {
        return Err(RegisterError::Invite(InviteError::Exhausted));
    }

    let user_id = sqlx::query_scalar!(
        r#"INSERT INTO users (username, password_hash, created_by_invite)
           VALUES ($1, $2, $3)
           ON CONFLICT (username) DO NOTHING
           RETURNING id"#,
        username,
        password_hash,
        Some(invite.id)
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RegisterError::DuplicateUsername)?;

    sqlx::query!(
        "UPDATE invite_codes SET used_count = used_count + 1 WHERE id = $1",
        invite.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(user_id)
}

pub async fn list_invites(pool: &PgPool) -> Result<Vec<InviteRow>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT code, max_uses, used_count, revoked FROM invite_codes ORDER BY id DESC"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| InviteRow {
            code: r.code,
            max_uses: r.max_uses,
            used_count: r.used_count,
            revoked: r.revoked,
        })
        .collect())
}

pub async fn revoke_invite(pool: &PgPool, code: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query!("UPDATE invite_codes SET revoked = true WHERE code = $1", code)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Store AES-GCM ciphertext for the user's GitHub PAT and stamp `github_token_set_at`.
pub async fn set_user_github_token(
    pool: &PgPool,
    user_id: i64,
    ciphertext: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE users
           SET github_token_ciphertext = $2,
               github_token_set_at = now()
           WHERE id = $1"#,
        user_id,
        ciphertext
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Clear stored PAT ciphertext and set timestamp.
pub async fn clear_user_github_token(pool: &PgPool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE users
           SET github_token_ciphertext = NULL,
               github_token_set_at = NULL
           WHERE id = $1"#,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Raw ciphertext blob (`nonce || ct || tag`), if set.
pub async fn get_user_github_token_ciphertext(
    pool: &PgPool,
    user_id: i64,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let rec = sqlx::query!(
        "SELECT github_token_ciphertext FROM users WHERE id = $1",
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(rec.and_then(|r| r.github_token_ciphertext))
}

/// Whether the user has a non-null stored PAT ciphertext (does not decrypt).
pub async fn user_has_github_token(pool: &PgPool, user_id: i64) -> Result<bool, sqlx::Error> {
    let rec = sqlx::query!(
        r#"SELECT (github_token_ciphertext IS NOT NULL) AS "has!: bool"
           FROM users WHERE id = $1"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(rec.map(|r| r.has).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use serial_test::serial;

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://postgres@localhost:5432/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    #[serial]
    async fn register_with_invite_consumes_once() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 1).await.unwrap();
        let uid = register_with_invite(&pool, "alice", "hash1", &code).await.unwrap();
        assert!(uid > 0);
        // One-time invite code fails on second use
        assert_eq!(
            register_with_invite(&pool, "bob", "hash2", &code).await.unwrap_err(),
            RegisterError::Invite(InviteError::Exhausted)
        );
        let invites = list_invites(&pool).await.unwrap();
        assert_eq!(invites[0].used_count, 1);
    }

    #[tokio::test]
    #[serial]
    async fn register_rejects_revoked_and_unknown_codes() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 5).await.unwrap();
        assert!(revoke_invite(&pool, &code).await.unwrap());
        assert_eq!(
            register_with_invite(&pool, "carol", "h", &code).await.unwrap_err(),
            RegisterError::Invite(InviteError::Revoked)
        );
        assert_eq!(
            register_with_invite(&pool, "dave", "h", "NOPE").await.unwrap_err(),
            RegisterError::Invite(InviteError::NotFound)
        );
    }

    #[tokio::test]
    #[serial]
    async fn duplicate_username_rejected_and_invite_not_consumed() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 1).await.unwrap();
        register_with_invite(&pool, "alice", "h1", &code).await.unwrap();
        let code2 = create_invite(&pool, 1).await.unwrap();
        assert_eq!(
            register_with_invite(&pool, "alice", "h2", &code2).await.unwrap_err(),
            RegisterError::DuplicateUsername
        );
        // code2 was not consumed
        let invites = list_invites(&pool).await.unwrap();
        let c2 = invites.iter().find(|i| i.code == code2).unwrap();
        assert_eq!(c2.used_count, 0);
    }

    #[tokio::test]
    #[serial]
    async fn multi_use_invite_serves_n_users() {
        let pool = test_pool().await;
        let code = create_invite(&pool, 2).await.unwrap();
        register_with_invite(&pool, "u1", "h", &code).await.unwrap();
        register_with_invite(&pool, "u2", "h", &code).await.unwrap();
        assert_eq!(
            register_with_invite(&pool, "u3", "h", &code).await.unwrap_err(),
            RegisterError::Invite(InviteError::Exhausted)
        );
    }

    #[tokio::test]
    #[serial]
    async fn invite_code_shape() {
        let code = generate_invite_code();
        assert_eq!(code.len(), 24);
        assert!(code.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[tokio::test]
    #[serial]
    async fn github_token_set_get_clear() {
        let pool = test_pool().await;
        let uid = create_user(&pool, "tokuser", "hash", None).await.unwrap();
        assert!(!user_has_github_token(&pool, uid).await.unwrap());
        assert!(get_user_github_token_ciphertext(&pool, uid)
            .await
            .unwrap()
            .is_none());

        let blob = b"nonce-and-ciphertext-blob".to_vec();
        set_user_github_token(&pool, uid, &blob).await.unwrap();
        assert!(user_has_github_token(&pool, uid).await.unwrap());
        assert_eq!(
            get_user_github_token_ciphertext(&pool, uid).await.unwrap(),
            Some(blob)
        );

        clear_user_github_token(&pool, uid).await.unwrap();
        assert!(!user_has_github_token(&pool, uid).await.unwrap());
        assert!(get_user_github_token_ciphertext(&pool, uid)
            .await
            .unwrap()
            .is_none());
    }
}
