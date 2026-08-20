use leptos::prelude::*;

use crate::models::ItemHistory;

/// Everything the item detail page renders: the item, its sources with current status,
/// the per-source series for the chart, and the flat rows for the table.
#[server(name = ItemHistoryFn, prefix = "/api", endpoint = "history/item")]
pub async fn item_history(item_id: String) -> Result<ItemHistory, ServerFnError> {
    use crate::models::{
        BestPrice, HistoryRow, ItemSource, PricePoint, SourceSeries, SourceStatus, WishlistItem,
    };
    use crate::server::auth;
    use std::collections::HashMap;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let item_id = auth::parse_id(&item_id, "item")?;

    let item = sqlx::query!(
        r#"
        select id, name, currency, target_price_cents, notes, active
        from wishlist_items
        where id = $1 and user_id = $2
        "#,
        item_id,
        user_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not load the item: {e}")))?
    .ok_or_else(|| ServerFnError::new("That item does not exist."))?;

    // Sources with the outcome of their most recent fetch, successful or not, so a
    // broken selector is visible in the editor rather than just missing from the chart.
    let source_rows = sqlx::query!(
        r#"
        select s.id, s.label, s.url, s.css_selector, s.price_regex, s.active,
               p.ok as "latest_ok?", p.price_cents as "latest_price_cents?",
               p.error as "latest_error?", p.fetched_at as "latest_fetched_at?"
        from item_sources s
        left join lateral (
            select ok, price_cents, error, fetched_at
            from price_snapshots
            where source_id = s.id
            order by fetched_at desc
            limit 1
        ) p on true
        where s.item_id = $1
        order by lower(s.label)
        "#,
        item_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not load the sources: {e}")))?;

    let sources: Vec<ItemSource> = source_rows
        .into_iter()
        .map(|r| ItemSource {
            latest: r.latest_fetched_at.map(|fetched_at| SourceStatus {
                ok: r.latest_ok.unwrap_or(false),
                price_cents: r.latest_price_cents,
                error: r.latest_error,
                fetched_at: fetched_at.timestamp_millis(),
            }),
            id: r.id.to_string(),
            item_id: item_id.to_string(),
            label: r.label,
            url: r.url,
            css_selector: r.css_selector,
            price_regex: r.price_regex,
            active: r.active,
        })
        .collect();

    // Every snapshot for the item, newest first. One query feeds both views: the table
    // renders these directly, and the chart series are grouped out of the successes.
    let snapshots = sqlx::query!(
        r#"
        select s.id as "source_id!", s.label as "label!",
               p.price_cents, p.ok as "ok!", p.error, p.fetched_at as "fetched_at!"
        from price_snapshots p
        join item_sources s on s.id = p.source_id
        where s.item_id = $1
        order by p.fetched_at desc
        "#,
        item_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not load the price history: {e}")))?;

    let rows: Vec<HistoryRow> = snapshots
        .iter()
        .map(|r| HistoryRow {
            source_id: r.source_id.to_string(),
            label: r.label.clone(),
            fetched_at: r.fetched_at.timestamp_millis(),
            price_cents: r.price_cents,
            ok: r.ok,
            error: r.error.clone(),
        })
        .collect();

    // Chart series: successful points only, oldest first so the line reads left to right.
    let mut grouped: HashMap<String, Vec<PricePoint>> = HashMap::new();
    for row in snapshots.iter().rev() {
        if let (true, Some(price_cents)) = (row.ok, row.price_cents) {
            grouped
                .entry(row.source_id.to_string())
                .or_default()
                .push(PricePoint {
                    fetched_at: row.fetched_at.timestamp_millis(),
                    price_cents,
                });
        }
    }

    // Driven by `sources` rather than by the map, so series order matches the editor and
    // a source's colour stays put when it happens to have no data yet.
    let series: Vec<SourceSeries> = sources
        .iter()
        .filter_map(|source| {
            grouped.remove(&source.id).map(|points| SourceSeries {
                source_id: source.id.clone(),
                label: source.label.clone(),
                points,
            })
        })
        .collect();

    // Cheapest of the sources' latest successful prices.
    let best = sources
        .iter()
        .filter(|s| s.active)
        .filter_map(|s| {
            let latest = s.latest.as_ref()?;
            let price_cents = latest.price_cents.filter(|_| latest.ok)?;
            Some(BestPrice {
                source_id: s.id.clone(),
                label: s.label.clone(),
                price_cents,
                fetched_at: latest.fetched_at,
            })
        })
        .min_by_key(|b| (b.price_cents, -b.fetched_at));

    Ok(ItemHistory {
        item: WishlistItem {
            id: item.id.to_string(),
            name: item.name,
            currency: item.currency,
            target_price_cents: item.target_price_cents,
            notes: item.notes,
            active: item.active,
            source_count: sources.len() as i64,
            sources,
            best,
        },
        series,
        rows,
    })
}
