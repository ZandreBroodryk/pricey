use leptos::prelude::*;

use crate::models::RefreshReport;

/// Refreshes every active source belonging to the signed-in user.
#[server(name = RefreshAllNow, prefix = "/api", endpoint = "refresh/all")]
pub async fn refresh_all_now() -> Result<RefreshReport, ServerFnError> {
    use crate::server::{auth, runner};

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;

    runner::refresh_user(&pool, user_id)
        .await
        .map_err(ServerFnError::new)
}

/// Refreshes every active source of one item.
#[server(name = RefreshItemNow, prefix = "/api", endpoint = "refresh/item")]
pub async fn refresh_item_now(item_id: String) -> Result<RefreshReport, ServerFnError> {
    use crate::server::{auth, runner};

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let item_id = auth::parse_id(&item_id, "item")?;

    runner::refresh_item(&pool, item_id, user_id)
        .await
        .map_err(ServerFnError::new)
}

/// Refreshes a single source.
#[server(name = RefreshSourceNow, prefix = "/api", endpoint = "refresh/source")]
pub async fn refresh_source_now(source_id: String) -> Result<RefreshReport, ServerFnError> {
    use crate::server::{auth, runner};

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let source_id = auth::parse_id(&source_id, "source")?;

    runner::refresh_source(&pool, source_id, user_id)
        .await
        .map_err(ServerFnError::new)
}
