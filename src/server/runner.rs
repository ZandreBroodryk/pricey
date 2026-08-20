//! Fetching prices for tracked sources and recording the results.
//!
//! Cron, the per-item button and the per-source button all funnel into [`run`], so there
//! is exactly one implementation of "check these sources and write down what you found".

use std::collections::HashMap;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use uuid::Uuid;

use super::price;
use crate::models::RefreshReport;

/// How many *hosts* to fetch from at once.
///
/// Kept low on purpose: it bounds how long a cron invocation runs (Vercel caps that).
const CONCURRENCY: usize = 4;

/// Pause between two requests to the same host.
///
/// Retailers behind Cloudflare start returning 403 challenges when several requests
/// arrive back to back, so sources are grouped by host and each host is walked serially
/// with this gap. Observed directly against Wootware: five rapid requests tripped it, and
/// the same request succeeded again after a pause.
const SAME_HOST_DELAY: Duration = Duration::from_millis(1500);

/// The host a source points at, used only for grouping. Sources whose URL will not parse
/// get their own bucket -- the fetch is going to fail anyway, and it should not serialise
/// unrelated work behind it.
fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| format!("unparseable:{url}"))
}

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

    // Group by host so that several products from one shop are never fetched at once.
    let mut by_host: HashMap<String, Vec<SourceRow>> = HashMap::new();
    for source in sources {
        by_host
            .entry(host_of(&source.url))
            .or_default()
            .push(source);
    }

    // Different hosts proceed in parallel; within a host, one at a time with a pause.
    let outcomes: Vec<(Uuid, price::FetchOutcome)> = stream::iter(by_host.into_values())
        .map(|group| {
            let client = &client;
            async move {
                let mut results = Vec::with_capacity(group.len());
                for (index, source) in group.into_iter().enumerate() {
                    if index > 0 {
                        tokio::time::sleep(SAME_HOST_DELAY).await;
                    }

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

                    results.push((source.id, outcome));
                }
                results
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect();

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

#[cfg(test)]
mod tests {
    use super::host_of;

    #[test]
    fn groups_by_host_ignoring_path_and_scheme_details() {
        assert_eq!(
            host_of("https://www.wootware.co.za/a.html"),
            "www.wootware.co.za"
        );
        assert_eq!(
            host_of("https://www.wootware.co.za/b.html"),
            "www.wootware.co.za"
        );
        assert_eq!(
            host_of("http://www.wootware.co.za:80/c"),
            "www.wootware.co.za"
        );
    }

    #[test]
    fn distinct_hosts_stay_distinct() {
        assert_ne!(
            host_of("https://www.wootware.co.za/a"),
            host_of("https://www.evetech.co.za/a")
        );
    }

    #[test]
    fn unparseable_urls_do_not_all_collapse_into_one_bucket() {
        // Otherwise a batch of broken URLs would serialise behind each other for no reason.
        assert_ne!(host_of("not a url"), host_of("also not a url"));
    }
}
