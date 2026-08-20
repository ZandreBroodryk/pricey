//! Axum application state.

use axum::extract::FromRef;
use leptos::prelude::LeptosOptions;
use sqlx::PgPool;

#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    pub leptos_options: LeptosOptions,
}

// `leptos_axum::file_and_error_handler` extracts `LeptosOptions` from whatever state the
// router carries, so the custom state has to be able to hand one over.
impl FromRef<AppState> for LeptosOptions {
    fn from_ref(state: &AppState) -> Self {
        state.leptos_options.clone()
    }
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}
