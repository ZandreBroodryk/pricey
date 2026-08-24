//! Types that cross the server-function boundary.
//!
//! These are compiled for **both** the server and the wasm bundle, so they deliberately
//! avoid `uuid`, `chrono` and `sqlx` types: ids travel as `String` and timestamps as
//! `i64` epoch milliseconds. That keeps the hydrate build free of those crates (and of
//! their `js`/wasm feature gymnastics). Conversion happens at the server boundary.

use serde::{Deserialize, Serialize};

/// The signed-in user, as far as the browser needs to know.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
}

/// One retailer page tracked for an item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSource {
    pub id: String,
    pub item_id: String,
    pub label: String,
    pub url: String,
    pub css_selector: String,
    pub price_regex: Option<String>,
    pub active: bool,
    /// The tracker cannot reach this retailer, so its prices are supplied by hand.
    /// Refreshes skip it entirely; the selector, when set, is used on pasted HTML.
    pub manual: bool,
    /// Most recent snapshot for this source, successful or not.
    pub latest: Option<SourceStatus>,
}

/// The outcome of the most recent fetch for a source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub ok: bool,
    pub price_cents: Option<i64>,
    pub error: Option<String>,
    pub fetched_at: i64,
    /// Whether this particular snapshot was supplied by the user. Read from the snapshot
    /// rather than the source so it stays right for one that was scraped before the
    /// retailer started blocking us.
    pub manual: bool,
}

/// The cheapest current price across an item's sources.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BestPrice {
    pub source_id: String,
    pub label: String,
    pub price_cents: i64,
    pub fetched_at: i64,
}

/// A product on the wishlist, with its sources and current best price.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WishlistItem {
    pub id: String,
    pub name: String,
    pub currency: String,
    pub target_price_cents: Option<i64>,
    pub notes: Option<String>,
    pub active: bool,
    /// Populated on the detail view only; the list view carries `source_count` instead so
    /// it does not ship a row per source it will never render.
    pub sources: Vec<ItemSource>,
    pub source_count: i64,
    pub best: Option<BestPrice>,
}

/// Field payload for creating or updating an item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemInput {
    pub name: String,
    pub currency: String,
    pub target_price_cents: Option<i64>,
    pub notes: Option<String>,
    pub active: bool,
}

/// Field payload for creating or updating a source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInput {
    /// Blank means "derive it from the URL's host".
    pub label: String,
    pub url: String,
    /// Required unless `manual`, where it is optional and only used on pasted HTML.
    pub css_selector: String,
    pub price_regex: Option<String>,
    pub active: bool,
    pub manual: bool,
}

/// A single recorded price. Failed fetches are not points -- see [`HistoryRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricePoint {
    pub fetched_at: i64,
    pub price_cents: i64,
}

/// One line on the chart: every successful price recorded for a single source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSeries {
    pub source_id: String,
    pub label: String,
    pub points: Vec<PricePoint>,
}

/// One row of the history table. Unlike [`PricePoint`] this includes failures.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRow {
    pub source_id: String,
    pub label: String,
    pub fetched_at: i64,
    pub price_cents: Option<i64>,
    pub ok: bool,
    pub error: Option<String>,
    /// Supplied by the user rather than fetched by the tracker.
    pub manual: bool,
}

/// Everything `/items/:id` needs to render.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemHistory {
    pub item: WishlistItem,
    pub series: Vec<SourceSeries>,
    pub rows: Vec<HistoryRow>,
}

/// Result of a refresh run, whether triggered by cron or by a button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshReport {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// What a selector extracted from a page.
///
/// Shared by two callers with the same shape: the "test this selector" button, which
/// never records, and recording from pasted HTML, which records only when `price_cents`
/// is `Some`. Either way a `None` carries `error`/`matched_text` to diagnose the selector.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTest {
    pub price_cents: Option<i64>,
    /// The raw text the selector matched, useful when parsing fails.
    pub matched_text: Option<String>,
    pub error: Option<String>,
}
