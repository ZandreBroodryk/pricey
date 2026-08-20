//! Axum wiring: server-function handling with context, and the cron endpoint.

use axum::body::Body;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use leptos::prelude::*;
use leptos_axum::{handle_server_fns_with_context, render_app_to_stream_with_context};
use serde_json::json;

use super::{runner, state::AppState};
use crate::app::shell;

/// Server functions need the pool; `provide_context` is how it reaches them.
pub async fn server_fn_handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> impl IntoResponse {
    let pool = state.pool.clone();
    handle_server_fns_with_context(move || provide_context(pool.clone()), req).await
}

/// Page renders need the pool too, since resources run on the server during SSR.
pub async fn leptos_routes_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let pool = state.pool.clone();
    let options = state.leptos_options.clone();

    render_app_to_stream_with_context(
        move || provide_context(pool.clone()),
        move || shell(options.clone()),
    )(req)
    .await
    .into_response()
}

/// `GET`/`POST /api/fetch-prices` -- refreshes every tracked source.
///
/// Vercel Cron calls this with `Authorization: Bearer $CRON_SECRET`. If `CRON_SECRET` is
/// not configured the endpoint refuses outright rather than running unauthenticated: an
/// unset variable is a misconfiguration, not permission to let anyone trigger scraping.
pub async fn fetch_prices(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Ok(secret) = std::env::var("CRON_SECRET") else {
        tracing::error!("CRON_SECRET is not set; refusing to run the price fetch endpoint");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "CRON_SECRET is not configured" })),
        )
            .into_response();
    };

    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();

    if !constant_time_eq(presented.as_bytes(), secret.as_bytes()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid or missing bearer token" })),
        )
            .into_response();
    }

    match runner::refresh_all(&state.pool).await {
        Ok(report) => {
            tracing::info!(
                attempted = report.attempted,
                succeeded = report.succeeded,
                failed = report.failed,
                "scheduled price refresh finished"
            );
            (StatusCode::OK, Json(report)).into_response()
        }
        Err(error) => {
            tracing::error!(%error, "scheduled price refresh failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )
                .into_response()
        }
    }
}

/// Compares without an early exit, so a wrong token cannot be recovered byte by byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn compares_bytes_correctly() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
