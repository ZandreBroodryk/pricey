use leptos::prelude::*;

use crate::models::{ItemInput, WishlistItem};

/// The wishlist, each item with its current cheapest price and where it came from.
///
/// The list view does not carry per-source detail -- only a count -- so the query returns
/// one row per item rather than one per source.
#[server(name = ListItems, prefix = "/api", endpoint = "items/list")]
pub async fn list_items() -> Result<Vec<WishlistItem>, ServerFnError> {
    use crate::models::BestPrice;
    use crate::server::auth;
    use std::collections::HashMap;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;

    let rows = sqlx::query!(
        r#"
        select i.id, i.name, i.currency, i.target_price_cents, i.notes, i.active,
               count(s.id) as "source_count!"
        from wishlist_items i
        left join item_sources s on s.item_id = i.id
        where i.user_id = $1
        group by i.id
        order by i.active desc, lower(i.name)
        "#,
        user_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not load your wishlist: {e}")))?;

    let item_ids: Vec<uuid::Uuid> = rows.iter().map(|r| r.id).collect();

    // Cheapest current price per item, reduced in the database so only one row per item
    // crosses the wire. `latest` picks each source's most recent successful snapshot;
    // the outer `distinct on` then picks the cheapest of those per item.
    let best_rows = sqlx::query!(
        r#"
        with latest as (
            select distinct on (s.id)
                   s.id as source_id, s.item_id, s.label, p.price_cents, p.fetched_at
            from item_sources s
            join price_snapshots p on p.source_id = s.id and p.ok
            where s.item_id = any($1) and s.active
            order by s.id, p.fetched_at desc
        )
        select distinct on (item_id)
               item_id as "item_id!", source_id as "source_id!", label as "label!",
               price_cents as "price_cents!", fetched_at as "fetched_at!"
        from latest
        order by item_id, price_cents asc, fetched_at desc
        "#,
        &item_ids
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not load current prices: {e}")))?;

    let mut best: HashMap<uuid::Uuid, BestPrice> = HashMap::new();
    for row in best_rows {
        best.insert(
            row.item_id,
            BestPrice {
                source_id: row.source_id.to_string(),
                label: row.label,
                price_cents: row.price_cents,
                fetched_at: row.fetched_at.timestamp_millis(),
            },
        );
    }

    Ok(rows
        .into_iter()
        .map(|r| WishlistItem {
            best: best.remove(&r.id),
            id: r.id.to_string(),
            name: r.name,
            currency: r.currency,
            target_price_cents: r.target_price_cents,
            notes: r.notes,
            active: r.active,
            sources: Vec::new(),
            source_count: r.source_count,
        })
        .collect())
}

#[server(name = CreateItem, prefix = "/api", endpoint = "items/create")]
pub async fn create_item(input: ItemInput) -> Result<String, ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let input = validate(input)?;

    let id = sqlx::query_scalar!(
        r#"
        insert into wishlist_items (user_id, name, currency, target_price_cents, notes, active)
        values ($1, $2, $3, $4, $5, $6)
        returning id
        "#,
        user_id,
        input.name,
        input.currency,
        input.target_price_cents,
        input.notes,
        input.active
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not create the item: {e}")))?;

    Ok(id.to_string())
}

#[server(name = UpdateItem, prefix = "/api", endpoint = "items/update")]
pub async fn update_item(id: String, input: ItemInput) -> Result<(), ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let item_id = auth::parse_id(&id, "item")?;
    let input = validate(input)?;

    // `user_id` in the WHERE clause is the ownership check: a guessed id updates nothing.
    let result = sqlx::query!(
        r#"
        update wishlist_items
        set name = $3, currency = $4, target_price_cents = $5, notes = $6, active = $7,
            updated_at = now()
        where id = $1 and user_id = $2
        "#,
        item_id,
        user_id,
        input.name,
        input.currency,
        input.target_price_cents,
        input.notes,
        input.active
    )
    .execute(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not update the item: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(ServerFnError::new("That item does not exist."));
    }
    Ok(())
}

#[server(name = DeleteItem, prefix = "/api", endpoint = "items/delete")]
pub async fn delete_item(id: String) -> Result<(), ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let item_id = auth::parse_id(&id, "item")?;

    // Sources and snapshots go with it via `on delete cascade`.
    let result = sqlx::query!(
        "delete from wishlist_items where id = $1 and user_id = $2",
        item_id,
        user_id
    )
    .execute(&pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not delete the item: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(ServerFnError::new("That item does not exist."));
    }
    Ok(())
}

/// Trims and sanity-checks item fields. Shared by create and update so they cannot drift.
#[cfg(feature = "ssr")]
fn validate(mut input: ItemInput) -> Result<ItemInput, ServerFnError> {
    input.name = input.name.trim().to_string();
    input.currency = input.currency.trim().to_uppercase();
    input.notes = input
        .notes
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    if input.name.is_empty() {
        return Err(ServerFnError::new("Give the item a name."));
    }
    if input.currency.is_empty() {
        input.currency = "ZAR".to_string();
    }
    if input.currency.chars().count() > 8 {
        return Err(ServerFnError::new("That currency code looks wrong."));
    }
    if input.target_price_cents.is_some_and(|c| c < 0) {
        return Err(ServerFnError::new("A target price cannot be negative."));
    }
    Ok(input)
}
