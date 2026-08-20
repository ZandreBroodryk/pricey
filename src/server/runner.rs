//! Fetching prices for tracked sources and recording the results.
//!
//! Cron, the per-item button and the per-source button all funnel into [`run`], so there
//! is exactly one implementation of "check these sources and write down what you found".

use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use uuid::Uuid;

use super::price;
use crate::models::RefreshReport;

/// How many retailer pages to fetch at once.
///
/// Kept low on purpose: it bounds how long a cron invocation runs (Vercel caps that) and
/// avoids hammering a single shop when several sources share a host.
const CONCURRENCY: usize = 4;

#[derive(Debug)]
struct SourceRow {
    id: Uuid,
    url: String,
    css_selector: String,
    price_regex: Option<String>,
}

/// Every active source of every active item, across all users. This is the cron path.
pub async fn refresh_all(pool: &PgPool) -> Result<RefreshReport, String> {
    let sources = sqlx::query_as!(
        SourceRow,
        r#"
        select s.id, s.url, s.css_selector, s.price_regex
        from item_sources s
        join wishlist_items i on i.id = s.item_id
        where s.active and i.active
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("could not load sources: {e}"))?;

    run(pool, sources).await
}

/// Every active source of one user's active items. This is the "Refresh all" button.
pub async fn refresh_user(pool: &PgPool, user_id: Uuid) -> Result<RefreshReport, String> {
    let sources = sqlx::query_as!(
        SourceRow,
        r#"
        select s.id, s.url, s.css_selector, s.price_regex
        from item_sources s
        join wishlist_items i on i.id = s.item_id
        where i.user_id = $1 and s.active and i.active
        "#,
        user_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("could not load sources: {e}"))?;

    run(pool, sources).await
}

/// Every active source of one item, scoped to its owner.
pub async fn refresh_item(
    pool: &PgPool,
    item_id: Uuid,
    user_id: Uuid,
) -> Result<RefreshReport, String> {
    let sources = sqlx::query_as!(
        SourceRow,
        r#"
        select s.id, s.url, s.css_selector, s.price_regex
        from item_sources s
        join wishlist_items i on i.id = s.item_id
        where s.item_id = $1 and i.user_id = $2 and s.active
        "#,
        item_id,
        user_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("could not load sources: {e}"))?;

    run(pool, sources).await
}

/// A single source, scoped to its owner.
pub async fn refresh_source(
    pool: &PgPool,
    source_id: Uuid,
    user_id: Uuid,
) -> Result<RefreshReport, String> {
    let sources = sqlx::query_as!(
        SourceRow,
        r#"
        select s.id, s.url, s.css_selector, s.price_regex
        from item_sources s
        join wishlist_items i on i.id = s.item_id
        where s.id = $1 and i.user_id = $2
        "#,
        source_id,
        user_id
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("could not load source: {e}"))?;

    run(pool, sources).await
}

/// Fetches each source and records one snapshot per source, successful or not.
async fn run(pool: &PgPool, sources: Vec<SourceRow>) -> Result<RefreshReport, String> {
    if sources.is_empty() {
        return Ok(RefreshReport::default());
    }

    let client = price::client();
    let attempted = sources.len();

    let outcomes: Vec<(Uuid, price::FetchOutcome)> = stream::iter(sources)
        .map(|source| {
            let client = &client;
            async move {
                let outcome = price::fetch_price(
                    client,
                    &source.url,
                    &source.css_selector,
                    source.price_regex.as_deref(),
                )
                .await;

                if let Some(error) = &outcome.error {
                    tracing::warn!(source_id = %source.id, url = %source.url, %error, "price fetch failed");
                }

                (source.id, outcome)
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;

    let succeeded = outcomes
        .iter()
        .filter(|(_, o)| o.price_cents.is_some())
        .count();

    // One round trip for the whole batch rather than a statement per source.
    let ids: Vec<Uuid> = outcomes.iter().map(|(id, _)| *id).collect();
    let prices: Vec<Option<i64>> = outcomes.iter().map(|(_, o)| o.price_cents).collect();
    let oks: Vec<bool> = outcomes
        .iter()
        .map(|(_, o)| o.price_cents.is_some())
        .collect();
    let errors: Vec<Option<String>> = outcomes.iter().map(|(_, o)| o.error.clone()).collect();

    sqlx::query!(
        r#"
        insert into price_snapshots (source_id, price_cents, ok, error)
        select * from unnest($1::uuid[], $2::bigint[], $3::bool[], $4::text[])
        "#,
        &ids,
        &prices as &[Option<i64>],
        &oks,
        &errors as &[Option<String>]
    )
    .execute(pool)
    .await
    .map_err(|e| format!("could not record price snapshots: {e}"))?;

    Ok(RefreshReport {
        attempted,
        succeeded,
        failed: attempted - succeeded,
    })
}
