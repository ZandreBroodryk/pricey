use leptos::prelude::*;

use crate::models::{SourceInput, SourceTest};

/// Adds a retailer page to an item.
#[server(name = CreateSource, prefix = "/api", endpoint = "sources/create")]
pub async fn create_source(item_id: String, input: SourceInput) -> Result<String, ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let item_id = auth::parse_id(&item_id, "item")?;
    let input = validate(input)?;

    // Confirm the item belongs to this user before hanging anything off it -- the source
    // table has no user_id of its own, so ownership has to be established here.
    let owns = sqlx::query_scalar!(
        r#"select exists(select 1 from wishlist_items where id = $1 and user_id = $2) as "owns!""#,
        item_id,
        user_id
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not check the item: {e}")))?;

    if !owns {
        return Err(ServerFnError::new("That item does not exist."));
    }

    let id = sqlx::query_scalar!(
        r#"
        insert into item_sources (item_id, label, url, css_selector, price_regex, active)
        values ($1, $2, $3, $4, $5, $6)
        returning id
        "#,
        item_id,
        input.label,
        input.url,
        input.css_selector,
        input.price_regex,
        input.active
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ServerFnError::new("That URL is already tracked for this item.")
        }
        _ => ServerFnError::new(format!("Could not add the source: {e}")),
    })?;

    Ok(id.to_string())
}

#[server(name = UpdateSource, prefix = "/api", endpoint = "sources/update")]
pub async fn update_source(id: String, input: SourceInput) -> Result<(), ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let source_id = auth::parse_id(&id, "source")?;
    let input = validate(input)?;

    // Ownership is enforced by joining back to the owning item inside the statement.
    let result = sqlx::query!(
        r#"
        update item_sources s
        set label = $3, url = $4, css_selector = $5, price_regex = $6, active = $7,
            updated_at = now()
        from wishlist_items i
        where s.id = $1 and i.id = s.item_id and i.user_id = $2
        "#,
        source_id,
        user_id,
        input.label,
        input.url,
        input.css_selector,
        input.price_regex,
        input.active
    )
    .execute(&pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ServerFnError::new("That URL is already tracked for this item.")
        }
        _ => ServerFnError::new(format!("Could not update the source: {e}")),
    })?;

    if result.rows_affected() == 0 {
        return Err(ServerFnError::new("That source does not exist."));
    }
    Ok(())
}

#[server(name = DeleteSource, prefix = "/api", endpoint = "sources/delete")]
pub async fn delete_source(id: String) -> Result<(), ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let source_id = auth::parse_id(&id, "source")?;

    let result = sqlx::query!(
        r#"
        delete from item_sources s
        using wishlist_items i
        where s.id = $1 and i.id = s.item_id and i.user_id = $2
        "#,
        source_id,
        user_id
    )
    .execute(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not delete the source: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(ServerFnError::new("That source does not exist."));
    }
    Ok(())
}

/// Runs a fetch and reports what the selector would extract, **without** recording it.
///
/// Getting a CSS selector right on the first try is unusual, so this exists to make the
/// configure-and-check loop fast instead of polluting the price history with attempts.
#[server(name = TestSource, prefix = "/api", endpoint = "sources/test")]
pub async fn test_source(input: SourceInput) -> Result<SourceTest, ServerFnError> {
    use crate::server::{auth, price};

    let pool = crate::server::pool()?;
    // Signed-in only: this makes the server fetch an arbitrary URL, which is not something
    // to expose anonymously.
    auth::require_user(&pool).await?;
    let input = validate(input)?;

    let client = price::client();
    let outcome = price::fetch_price(
        &client,
        &input.url,
        &input.css_selector,
        input.price_regex.as_deref(),
    )
    .await;

    Ok(SourceTest {
        price_cents: outcome.price_cents,
        matched_text: outcome.matched_text,
        error: outcome.error,
    })
}

/// Trims fields, checks the URL, and defaults a blank label to the URL's host.
#[cfg(feature = "ssr")]
fn validate(mut input: SourceInput) -> Result<SourceInput, ServerFnError> {
    input.url = input.url.trim().to_string();
    input.label = input.label.trim().to_string();
    input.css_selector = input.css_selector.trim().to_string();
    input.price_regex = input
        .price_regex
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let parsed =
        url::Url::parse(&input.url).map_err(|_| ServerFnError::new("That URL is not valid."))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ServerFnError::new("The URL must be http or https."));
    }

    if input.css_selector.is_empty() {
        return Err(ServerFnError::new("A CSS selector is required."));
    }
    // Reject a broken selector here rather than storing it and failing on every run.
    // `scraper`'s error Display asks the reader to report a bug, which is wrong here --
    // an invalid selector is user input, not a library fault.
    scraper::Selector::parse(&input.css_selector)
        .map_err(|_| ServerFnError::new("That CSS selector is not valid."))?;

    if let Some(regex) = &input.price_regex {
        regex::Regex::new(regex)
            .map_err(|e| ServerFnError::new(format!("That regex is not valid: {e}")))?;
    }

    if input.label.is_empty() {
        input.label = parsed
            .host_str()
            .map(|h| h.trim_start_matches("www.").to_string())
            .unwrap_or_else(|| "Unnamed source".to_string());
    }

    Ok(input)
}
