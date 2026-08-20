use leptos::prelude::*;

use crate::models::AuthUser;

/// Minimum password length. Short enough not to be annoying, long enough to matter.
pub const MIN_PASSWORD_LEN: usize = 10;

/// Shallow email check: enough to catch typos, without pretending to validate deliverability.
pub fn looks_like_email(email: &str) -> bool {
    let email = email.trim();
    match email.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !email.contains(char::is_whitespace)
        }
        None => false,
    }
}

/// Who is signed in, if anyone. Drives the nav bar and the client-side route guard.
#[server(name = CurrentUser, prefix = "/api", endpoint = "auth/current")]
pub async fn current_user() -> Result<Option<AuthUser>, ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    Ok(auth::current_session(&pool).await.map(|s| s.user))
}

#[server(name = Signup, prefix = "/api", endpoint = "auth/signup")]
pub async fn signup(email: String, password: String, confirm: String) -> Result<(), ServerFnError> {
    use crate::server::auth;

    if !auth::signup_allowed() {
        return Err(ServerFnError::new("Sign-ups are closed."));
    }

    let email = email.trim().to_string();
    if !looks_like_email(&email) {
        return Err(ServerFnError::new(
            "That does not look like an email address.",
        ));
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ServerFnError::new(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters."
        )));
    }
    if password != confirm {
        return Err(ServerFnError::new("The two passwords do not match."));
    }

    let pool = crate::server::pool()?;
    let hash = auth::hash_password(&password).map_err(ServerFnError::new)?;

    // The unique index on lower(email) is what actually prevents duplicates; checking
    // first would still race, so let the insert fail and translate the violation.
    let user_id = sqlx::query_scalar!(
        "insert into users (email, password_hash) values ($1, $2) returning id",
        email,
        hash
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ServerFnError::new("An account with that email already exists.")
        }
        _ => ServerFnError::new(format!("Could not create the account: {e}")),
    })?;

    start_session(&pool, user_id).await?;
    leptos_axum::redirect("/");
    Ok(())
}

#[server(name = Login, prefix = "/api", endpoint = "auth/login")]
pub async fn login(email: String, password: String) -> Result<(), ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let email = email.trim().to_string();

    let found = sqlx::query!(
        r#"select id, password_hash from users where lower(email) = lower($1)"#,
        email
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not sign you in: {e}")))?;

    // One message for both "no such account" and "wrong password", and comparable timing
    // for both, so neither reveals whether an email is registered.
    let Some(record) = found else {
        auth::burn_verify_time(&password);
        return Err(ServerFnError::new("Invalid email or password."));
    };

    if !auth::verify_password(&password, &record.password_hash) {
        return Err(ServerFnError::new("Invalid email or password."));
    }

    start_session(&pool, record.id).await?;
    leptos_axum::redirect("/");
    Ok(())
}

#[server(name = Logout, prefix = "/api", endpoint = "auth/logout")]
pub async fn logout() -> Result<(), ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;

    // Deleting the row is what revokes access; clearing the cookie is just tidiness.
    if let Some(session) = auth::current_session(&pool).await {
        auth::delete_session(&pool, session.session_id)
            .await
            .map_err(|e| ServerFnError::new(format!("Could not sign you out: {e}")))?;
    }

    set_cookie(auth::clearing_cookie())?;
    leptos_axum::redirect("/login");
    Ok(())
}

#[cfg(feature = "ssr")]
async fn start_session(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> Result<(), ServerFnError> {
    use crate::server::auth;

    let session_id = auth::create_session(pool, user_id)
        .await
        .map_err(|e| ServerFnError::new(format!("Could not start a session: {e}")))?;
    set_cookie(auth::session_cookie(session_id))
}

#[cfg(feature = "ssr")]
fn set_cookie(cookie: axum_extra::extract::cookie::Cookie<'static>) -> Result<(), ServerFnError> {
    use axum::http::header::{HeaderValue, SET_COOKIE};

    let value = HeaderValue::from_str(&cookie.to_string())
        .map_err(|e| ServerFnError::new(format!("Could not set the session cookie: {e}")))?;
    expect_context::<leptos_axum::ResponseOptions>().insert_header(SET_COOKIE, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::looks_like_email;

    #[test]
    fn accepts_ordinary_addresses() {
        assert!(looks_like_email("zandre@pipetocode.dev"));
        assert!(looks_like_email("a.b+tag@sub.example.co.za"));
    }

    #[test]
    fn rejects_obvious_typos() {
        assert!(!looks_like_email("no-at-sign"));
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("user@nodot"));
        assert!(!looks_like_email("user@.leading"));
        assert!(!looks_like_email("user@trailing."));
        assert!(!looks_like_email("has space@example.com"));
        assert!(!looks_like_email(""));
    }
}
