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
        insert into item_sources (item_id, label, url, css_selector, price_regex, active, manual)
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id
        "#,
        item_id,
        input.label,
        input.url,
        input.css_selector,
        input.price_regex,
        input.active,
        input.manual
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
            manual = $8, updated_at = now()
        from wishlist_items i
        where s.id = $1 and i.id = s.item_id and i.user_id = $2
        "#,
        source_id,
        user_id,
        input.label,
        input.url,
        input.css_selector,
        input.price_regex,
        input.active,
        input.manual
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

    // A manual source exists precisely because this fetch does not work for it. Say so
    // rather than spending a request to reproduce the block; `record_from_html` is the
    // equivalent selector check for these.
    if input.manual {
        return Err(ServerFnError::new(
            "This retailer is set to manual entry, so there is nothing to fetch. \
             Paste its page source instead.",
        ));
    }

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

/// Records a price the user read off the page themselves.
///
/// The fallback for manual sources, and the only option on a phone where viewing a page's
/// source is impractical. `price` is free text -- whatever the retailer displayed.
///
/// Returns what was stored rather than a bare acknowledgement, in the same shape as the
/// paste path: [`crate::fmt::parse_price`] takes the first run of digits it finds, so
/// "Was R1 999, now R899" is read as 1999 and the only way to notice is to be shown the
/// number that went in.
#[server(name = RecordPrice, prefix = "/api", endpoint = "sources/record-price")]
pub async fn record_price(source_id: String, price: String) -> Result<SourceTest, ServerFnError> {
    use crate::server::auth;

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let source_id = auth::parse_id(&source_id, "source")?;

    // The same parser the scraper uses, so a price typed as it appears on the page --
    // "R 1 299,00" -- is read identically to one that was scraped.
    let price_cents = crate::fmt::parse_price(&price)
        .ok_or_else(|| ServerFnError::new("That does not look like a price."))?;
    if price_cents < 0 {
        return Err(ServerFnError::new("A price cannot be negative."));
    }

    insert_manual_snapshot(&pool, source_id, user_id, price_cents).await?;

    Ok(SourceTest {
        price_cents: Some(price_cents),
        // Nothing was matched out of a larger page here -- the field *is* the price, and
        // echoing it back adds nothing the parsed number above does not already say.
        matched_text: None,
        error: None,
    })
}

/// Extracts a price from HTML the user pasted and records it.
///
/// The primary path for a retailer that blocks this host: the page cannot be fetched from
/// here, but the user's own browser reaches it fine, so they supply the page and the
/// selector does the rest. Extraction runs through the very same
/// [`price::extract`](crate::server::price::extract) the scraper uses, so nothing about
/// how a price is read differs between the two.
///
/// The selector comes from the client, like [`test_source`]'s does, because the editor
/// shows it in an editable field: reading the stored one instead would extract with a
/// selector the user is not looking at. It is not trusted for anything -- the only thing
/// it can do is read the caller's own paste -- and ownership is still enforced in SQL by
/// [`insert_manual_snapshot`].
///
/// A returned `price_cents` of `Some` means the price was recorded; `None` means nothing
/// was recorded and `error`/`matched_text` say why.
#[server(name = RecordFromHtml, prefix = "/api", endpoint = "sources/record-html")]
pub async fn record_from_html(
    source_id: String,
    html: String,
    css_selector: String,
    price_regex: Option<String>,
) -> Result<SourceTest, ServerFnError> {
    use crate::server::{auth, price};

    let pool = crate::server::pool()?;
    let user = auth::require_user(&pool).await?;
    let user_id = auth::parse_id(&user.id, "user")?;
    let source_id = auth::parse_id(&source_id, "source")?;

    // Checked here as well as by the body limit on the route so an oversized paste gets a
    // sentence rather than a bare 413. The route's limit is derived from this one so the
    // sentence is actually reachable (see `RECORD_HTML_BODY_LIMIT`).
    if html.len() > MAX_PASTED_HTML {
        return Err(ServerFnError::new(
            "That page is too large to accept. Paste the page source, not a saved copy \
             with its images inlined.",
        ));
    }

    let css_selector = css_selector.trim();
    if css_selector.is_empty() {
        return Err(ServerFnError::new(
            "Set a CSS selector for this retailer first, or enter the price directly.",
        ));
    }

    let selector = scraper::Selector::parse(css_selector)
        .map_err(|_| ServerFnError::new("That CSS selector is not valid."))?;
    let regex = match price_regex.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => Some(
            regex::Regex::new(r)
                .map_err(|e| ServerFnError::new(format!("That regex is not valid: {e}")))?,
        ),
        None => None,
    };

    let outcome = price::extract(&html, &selector, regex.as_ref());

    // Only a successful read is worth recording. A miss is a selector problem, and the
    // caller shows the error and matched text so it can be fixed -- exactly like Test.
    if let Some(price_cents) = outcome.price_cents {
        insert_manual_snapshot(&pool, source_id, user_id, price_cents).await?;
    }

    Ok(SourceTest {
        price_cents: outcome.price_cents,
        matched_text: outcome.matched_text,
        error: outcome.error,
    })
}

/// Largest page this will accept, in bytes. Product pages run a few hundred KB; this
/// leaves generous room while refusing something that is clearly not a page.
#[cfg(feature = "ssr")]
const MAX_PASTED_HTML: usize = 4 * 1024 * 1024;

/// Body limit for the `sources/record-html` route, applied in `main.rs`.
///
/// It has to sit well *above* [`MAX_PASTED_HTML`] rather than alongside it. The page
/// arrives as one form-urlencoded field, and percent-encoding spends three bytes on every
/// character that is not URL-safe -- which in HTML is most of them: `<`, `>`, `"`, `=`,
/// spaces and newlines. A 4 MB page can therefore reach ~12 MB on the wire, so an equal
/// limit would always fire first and answer an oversized paste with a deserialization
/// error instead of the sentence in `record_from_html`.
#[cfg(feature = "ssr")]
pub const RECORD_HTML_BODY_LIMIT: usize = 4 * MAX_PASTED_HTML;

/// Writes a user-supplied price, with ownership enforced inside the statement.
///
/// Shared by both manual paths so they cannot disagree about what gets stored: `ok` is
/// true (there is a price), and `manual` marks it as not having been measured by us.
#[cfg(feature = "ssr")]
async fn insert_manual_snapshot(
    pool: &sqlx::PgPool,
    source_id: uuid::Uuid,
    user_id: uuid::Uuid,
    price_cents: i64,
) -> Result<(), ServerFnError> {
    let result = sqlx::query!(
        r#"
        insert into price_snapshots (source_id, price_cents, ok, manual)
        select s.id, $3, true, true
        from item_sources s
        join wishlist_items i on i.id = s.item_id
        where s.id = $1 and i.user_id = $2
        "#,
        source_id,
        user_id,
        price_cents
    )
    .execute(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("Could not record the price: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(ServerFnError::new("That source does not exist."));
    }
    Ok(())
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

    // A scraped source cannot work without a selector. A manual one may leave it blank and
    // rely on typed-in prices, but keeps it when there is one -- that is what reads a price
    // out of pasted HTML.
    if input.css_selector.is_empty() {
        if !input.manual {
            return Err(ServerFnError::new("A CSS selector is required."));
        }
    } else {
        // Reject a broken selector here rather than storing it and failing on every run.
        // `scraper`'s error Display asks the reader to report a bug, which is wrong here --
        // an invalid selector is user input, not a library fault.
        scraper::Selector::parse(&input.css_selector)
            .map_err(|_| ServerFnError::new("That CSS selector is not valid."))?;
    }

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
