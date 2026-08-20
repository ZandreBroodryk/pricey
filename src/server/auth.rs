//! Email + password authentication with server-side sessions.
//!
//! Sessions are rows in the database rather than signed tokens, so logging out genuinely
//! revokes access instead of waiting for an expiry to lapse.

use std::sync::LazyLock;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use leptos::prelude::*;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::AuthUser;

pub const SESSION_COOKIE: &str = "pricey_session";
const SESSION_DAYS: i64 = 30;

/// A real hash of a throwaway password, computed once, used to burn the same CPU time on
/// a missing account as on a real one so login timing does not reveal whether an email is
/// registered. Generated rather than hardcoded so it is always a valid PHC string.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("this password exists only to make failed logins cost the same")
        .expect("hashing a static password cannot fail")
});

/// The signed-in user together with the session backing it.
#[derive(Clone, Debug)]
pub struct Session {
    pub user: AuthUser,
    pub session_id: Uuid,
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("could not hash password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Spends comparable time to a real verification against a known-bad hash.
pub fn burn_verify_time(password: &str) {
    let _ = verify_password(password, &DUMMY_HASH);
}

/// Whether new accounts may be created.
///
/// Signup is currently open. Gating the deployment later means changing only this
/// function -- for example, requiring an `INVITE_CODE` env var to match a submitted code.
pub fn signup_allowed() -> bool {
    true
}

/// Session cookies are only marked `Secure` in production, so plain-HTTP localhost works.
fn production() -> bool {
    std::env::var("APP_ENV")
        .map(|v| v == "production")
        .unwrap_or(false)
}

pub fn session_cookie(session_id: Uuid) -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE, session_id.to_string());
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_secure(production());
    cookie.set_max_age(Some(time::Duration::days(SESSION_DAYS)));
    cookie
}

pub fn clearing_cookie() -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE, "");
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_secure(production());
    cookie.set_max_age(Some(time::Duration::seconds(0)));
    cookie
}

pub async fn create_session(pool: &PgPool, user_id: Uuid) -> Result<Uuid, sqlx::Error> {
    let expires_at = Utc::now() + Duration::days(SESSION_DAYS);
    let id = sqlx::query_scalar!(
        "insert into sessions (user_id, expires_at) values ($1, $2) returning id",
        user_id,
        expires_at
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn delete_session(pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("delete from sessions where id = $1", session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolves the session cookie on the current request into a user, if it is still valid.
pub async fn current_session(pool: &PgPool) -> Option<Session> {
    let jar = leptos_axum::extract::<axum_extra::extract::CookieJar>()
        .await
        .ok()?;
    let raw = jar.get(SESSION_COOKIE)?.value().to_string();
    let session_id = Uuid::parse_str(&raw).ok()?;

    let row = sqlx::query!(
        r#"
        select u.id as "user_id!", u.email as "email!"
        from sessions s
        join users u on u.id = s.user_id
        where s.id = $1 and s.expires_at > now()
        "#,
        session_id
    )
    .fetch_optional(pool)
    .await
    .ok()??;

    Some(Session {
        user: AuthUser {
            id: row.user_id.to_string(),
            email: row.email,
        },
        session_id,
    })
}

/// The guard every data-touching server function starts with.
///
/// Client-side redirects are cosmetic; this is what actually protects the data.
pub async fn require_user(pool: &PgPool) -> Result<AuthUser, ServerFnError> {
    current_session(pool)
        .await
        .map(|s| s.user)
        .ok_or_else(|| ServerFnError::new("You must be signed in to do that."))
}

/// Parses a user id that arrived from the client.
pub fn parse_id(raw: &str, what: &str) -> Result<Uuid, ServerFnError> {
    Uuid::parse_str(raw).map_err(|_| ServerFnError::new(format!("invalid {what} id")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trips() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn salts_differ_between_hashes_of_the_same_password() {
        let a = hash_password("same").unwrap();
        let b = hash_password("same").unwrap();
        assert_ne!(a, b, "each hash must use a fresh salt");
    }

    #[test]
    fn rejects_a_malformed_stored_hash() {
        assert!(!verify_password("anything", "not-a-phc-string"));
    }

    #[test]
    fn dummy_hash_is_parseable_so_timing_defence_actually_runs() {
        assert!(
            PasswordHash::new(DUMMY_HASH.as_str()).is_ok(),
            "DUMMY_HASH must parse, otherwise verification short-circuits and burns no time"
        );
    }
}
