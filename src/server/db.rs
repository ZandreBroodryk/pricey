//! Database pool construction and migrations.

use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

/// Connects using `DATABASE_URL`.
///
/// The pool is deliberately small: Neon's free tier and a single Vercel container both
/// prefer few connections, and this workload is bursty rather than concurrent.
///
/// Note the connection string carries its own TLS mode. Neon needs `?sslmode=require`;
/// local Postgres has none. sqlx defaults to `prefer`, so both work without a code change.
pub async fn connect() -> Result<PgPool, String> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set".to_string())?;

    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .map_err(|e| format!("could not connect to the database: {e}"))
}

/// Applies any pending migrations, so a fresh Neon branch provisions itself on first boot.
pub async fn migrate(pool: &PgPool) -> Result<(), String> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| format!("migrations failed: {e}"))
}
