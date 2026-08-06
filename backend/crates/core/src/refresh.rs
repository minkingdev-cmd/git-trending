use chrono::Utc;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

pub const REFRESH_TTL_SECS: i64 = 30 * 24 * 3600;

pub fn new_refresh_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RefreshError {
    #[error("token not found")]
    Invalid,
    #[error("token expired")]
    Expired,
    #[error("token reuse detected; all sessions revoked")]
    Stolen,
}

pub async fn create_refresh_token(pool: &PgPool, user_id: i64) -> Result<String, sqlx::Error> {
    let token = new_refresh_token();
    let hash = hash_token(&token);
    let expires = Utc::now() + chrono::Duration::seconds(REFRESH_TTL_SECS);
    sqlx::query!(
        "INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
        user_id,
        hash,
        expires
    )
    .execute(pool)
    .await?;
    Ok(token)
}

pub async fn rotate_refresh_token(pool: &PgPool, presented: &str) -> Result<i64, RefreshError> {
    let hash = hash_token(presented);
    let mut tx = pool.begin().await.map_err(|_| RefreshError::Invalid)?;

    let row = sqlx::query!(
        "SELECT id, user_id, expires_at, used_at FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
        hash
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| RefreshError::Invalid)?
    .ok_or(RefreshError::Invalid)?;

    if row.used_at.is_some() {
        let _ = sqlx::query!("DELETE FROM refresh_tokens WHERE user_id = $1", row.user_id)
            .execute(&mut *tx)
            .await;
        let _ = tx.commit().await;
        return Err(RefreshError::Stolen);
    }
    if row.expires_at < Utc::now() {
        let _ = sqlx::query!("DELETE FROM refresh_tokens WHERE id = $1", row.id)
            .execute(&mut *tx)
            .await;
        let _ = tx.commit().await;
        return Err(RefreshError::Expired);
    }

    sqlx::query!("UPDATE refresh_tokens SET used_at = now() WHERE id = $1", row.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Invalid)?;

    tx.commit().await.map_err(|_| RefreshError::Invalid)?;
    Ok(row.user_id)
}

pub async fn delete_refresh_token(pool: &PgPool, presented: &str) -> Result<(), sqlx::Error> {
    let hash = hash_token(presented);
    sqlx::query!("DELETE FROM refresh_tokens WHERE token_hash = $1", hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_all_for_user(pool: &PgPool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM refresh_tokens WHERE user_id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use serial_test::serial;

    async fn test_pool_with_user() -> (PgPool, i64) {
        let url = std::env::var("DATABASE_URL_TEST")
            .unwrap_or_else(|_| "postgres://ght:ght@localhost:5433/ghtrending_test".into());
        let pool = db::pg_pool(&url).await.expect("test db unreachable; run `make db`");
        db::migrate(&pool).await.unwrap();
        sqlx::query("TRUNCATE repos, snapshots, users, invite_codes, refresh_tokens")
            .execute(&pool)
            .await
            .unwrap();
        let uid: i64 = sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ('u', 'h') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
        (pool, uid)
    }

    #[tokio::test]
    #[serial]
    async fn rotate_returns_user_and_invalidates_old_token() {
        let (pool, uid) = test_pool_with_user().await;
        let token = create_refresh_token(&pool, uid).await.unwrap();
        let got = rotate_refresh_token(&pool, &token).await.unwrap();
        assert_eq!(got, uid);
        // Reusing old token = stolen; all tokens deleted
        assert_eq!(rotate_refresh_token(&pool, &token).await.unwrap_err(), RefreshError::Stolen);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    #[serial]
    async fn unknown_token_is_invalid() {
        let (pool, _uid) = test_pool_with_user().await;
        assert_eq!(rotate_refresh_token(&pool, "bogus").await.unwrap_err(), RefreshError::Invalid);
    }

    #[tokio::test]
    #[serial]
    async fn expired_token_rejected() {
        let (pool, uid) = test_pool_with_user().await;
        let token = create_refresh_token(&pool, uid).await.unwrap();
        sqlx::query("UPDATE refresh_tokens SET expires_at = now() - interval '1 day'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(rotate_refresh_token(&pool, &token).await.unwrap_err(), RefreshError::Expired);
    }

    #[tokio::test]
    #[serial]
    async fn delete_token_and_delete_all_for_user() {
        let (pool, uid) = test_pool_with_user().await;
        let t1 = create_refresh_token(&pool, uid).await.unwrap();
        let _t2 = create_refresh_token(&pool, uid).await.unwrap();
        delete_refresh_token(&pool, &t1).await.unwrap();
        delete_all_for_user(&pool, uid).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
