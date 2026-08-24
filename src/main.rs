#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use std::net::SocketAddr;

    use axum::routing::get;
    use axum::Router;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use pricey::app::*;
    use pricey::server::{db, routes, state::AppState};

    // A missing .env is normal in production, where the platform injects the environment.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,pricey=debug".into()),
        )
        .init();

    let pool = db::connect().await.unwrap_or_else(|e| panic!("{e}"));
    db::migrate(&pool).await.unwrap_or_else(|e| panic!("{e}"));
    tracing::info!("database ready");

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;

    // Vercel routes container traffic to port 80 unless the project sets PORT, and that
    // same variable is what tells us where to listen -- so the deployment needs PORT=8080
    // configured on the project to agree with the image (see the README). Locally there is
    // no PORT and cargo-leptos' site-addr is the right answer.
    let addr = match std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        Some(port) => SocketAddr::from(([0, 0, 0, 0], port)),
        None => leptos_options.site_addr,
    };

    let routes_list = generate_route_list(App);
    let state = AppState {
        pool,
        leptos_options: leptos_options.clone(),
    };

    let app = Router::new()
        // Vercel Cron issues GET; POST is kept for triggering it by hand with curl.
        .route(
            "/api/fetch-prices",
            get(routes::fetch_prices).post(routes::fetch_prices),
        )
        // The link people click in their verification email.
        .route("/verify", get(routes::verify_email))
        // Server functions are registered explicitly so they receive the database pool.
        //
        // The body limit is raised from Axum's 2 MB default because `sources/record-html`
        // takes a whole product page pasted by the user, and a page can exceed that.
        // `MAX_PASTED_HTML` in `api::sources` mirrors this number so an oversized paste is
        // answered with a sentence rather than a bare 413.
        .route(
            "/api/{*fn_name}",
            get(routes::server_fn_handler)
                .post(routes::server_fn_handler)
                .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .leptos_routes_with_handler(routes_list, get(routes::leptos_routes_handler))
        .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
        .with_state(state);

    tracing::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for pure client-side testing
    // see lib.rs for hydration function instead
}
