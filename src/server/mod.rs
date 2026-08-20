//! Server-only code. Never compiled into the wasm bundle.

pub mod auth;
pub mod db;
pub mod price;
pub mod routes;
pub mod runner;
pub mod state;

use leptos::prelude::{use_context, ServerFnError};
use sqlx::PgPool;

/// The request's database pool, from the context `routes::server_fn_handler` provides.
///
/// This uses `use_context` rather than `expect_context` deliberately: `generate_route_list`
/// renders the app at startup to discover its routes, and there is no pool in scope then.
/// Panicking there would take the server down before it ever listened.
pub fn pool() -> Result<PgPool, ServerFnError> {
    use_context::<PgPool>()
        .ok_or_else(|| ServerFnError::new("The database is not available in this context."))
}
