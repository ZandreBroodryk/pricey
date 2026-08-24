//! The tabular view of recorded prices, including failed fetches.

use leptos::prelude::*;

use crate::fmt::{format_cents, format_datetime, format_delta};
use crate::models::HistoryRow;

/// Price change for each row against the previous **successful** price from the *same*
/// source. Input is newest-first; the returned vector is aligned with it.
///
/// Comparing per source matters: with several retailers interleaved in one table, a naive
/// "difference from the row above" would compare one shop's price against another's.
pub fn deltas(rows: &[HistoryRow]) -> Vec<Option<i64>> {
    use std::collections::HashMap;

    let mut previous: HashMap<&str, i64> = HashMap::new();
    let mut out = vec![None; rows.len()];

    // Walk oldest-first so "previous" means what it says, then write back into place.
    for (i, row) in rows.iter().enumerate().rev() {
        let Some(price) = row.price_cents.filter(|_| row.ok) else {
            continue;
        };
        if let Some(before) = previous.insert(row.source_id.as_str(), price) {
            out[i] = Some(price - before);
        }
    }

    out
}

#[component]
pub fn PriceTable(rows: Vec<HistoryRow>, currency: String) -> AnyView {
    // "" means every source.
    let filter = RwSignal::new(String::new());

    // Distinct sources present in the history, for the filter dropdown.
    let mut options: Vec<(String, String)> = Vec::new();
    for row in &rows {
        if !options.iter().any(|(id, _)| id == &row.source_id) {
            options.push((row.source_id.clone(), row.label.clone()));
        }
    }

    let rows = StoredValue::new(rows);
    let currency = StoredValue::new(currency);
    let has_rows = !rows.with_value(|r| r.is_empty());

    let body = move || {
        let all = rows.get_value();
        let currency = currency.get_value();
        // Deltas are computed over the full history, then filtered, so hiding a source
        // never changes the change-column of the rows that remain.
        let all_deltas = deltas(&all);
        let selected = filter.get();

        all.into_iter()
            .zip(all_deltas)
            .filter(|(row, _)| selected.is_empty() || row.source_id == selected)
            .map(|(row, delta)| {
                let price = match row.price_cents.filter(|_| row.ok) {
                    Some(cents) => format_cents(cents, &currency),
                    None => "-".to_string(),
                };
                let change = delta
                    .map(|d| format_delta(d, &currency))
                    .unwrap_or_else(|| "-".to_string());
                let direction = match delta {
                    Some(d) if d > 0 => "up",
                    Some(d) if d < 0 => "down",
                    _ => "flat",
                };

                view! {
                    <tr class:row-failed=!row.ok>
                        <td class="col-when">{format_datetime(row.fetched_at)}</td>
                        <td class="col-source">{row.label}</td>
                        <td class="col-price">{price}</td>
                        <td class=format!("col-change change-{direction}")>{change}</td>
                        <td class="col-status">
                            {if row.manual {
                                // Worth distinguishing: this number was supplied by hand,
                                // not measured, so it is only as good as what was typed.
                                view! { <span class="badge badge-manual">"manual"</span> }
                                    .into_any()
                            } else if row.ok {
                                view! { <span class="badge badge-ok">"ok"</span> }.into_any()
                            } else {
                                let error = row.error.unwrap_or_else(|| "failed".to_string());
                                let tooltip = error.clone();
                                view! {
                                    <span class="badge badge-failed" title=tooltip>{error}</span>
                                }
                                .into_any()
                            }}
                        </td>
                    </tr>
                }
            })
            .collect::<Vec<_>>()
    };

    view! {
        <div class="price-table">
            <Show when=move || has_rows fallback=|| view! {
                <p class="empty">"No prices recorded yet."</p>
            }>
                <label class="table-filter">
                    "Source "
                    <select on:change=move |ev| filter.set(event_target_value(&ev))>
                        <option value="">"All sources"</option>
                        {options
                            .clone()
                            .into_iter()
                            .map(|(id, label)| view! { <option value=id>{label}</option> })
                            .collect::<Vec<_>>()}
                    </select>
                </label>

                <table>
                    <thead>
                        <tr>
                            <th>"When"</th>
                            <th>"Source"</th>
                            <th>"Price"</th>
                            <th>"Change"</th>
                            <th>"Status"</th>
                        </tr>
                    </thead>
                    <tbody>{body}</tbody>
                </table>
            </Show>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(source: &str, at: i64, price: Option<i64>) -> HistoryRow {
        HistoryRow {
            source_id: source.to_string(),
            label: source.to_string(),
            fetched_at: at,
            price_cents: price,
            ok: price.is_some(),
            error: price.is_none().then(|| "boom".to_string()),
            manual: false,
        }
    }

    #[test]
    fn oldest_row_of_a_source_has_no_change() {
        let rows = vec![row("a", 2, Some(200)), row("a", 1, Some(100))];
        assert_eq!(deltas(&rows), vec![Some(100), None]);
    }

    #[test]
    fn changes_are_computed_per_source_not_per_adjacent_row() {
        // Interleaved sources, newest first.
        let rows = vec![
            row("a", 4, Some(150)),
            row("b", 3, Some(900)),
            row("a", 2, Some(100)),
            row("b", 1, Some(800)),
        ];
        assert_eq!(
            deltas(&rows),
            vec![Some(50), Some(100), None, None],
            "each row must compare against the same source's previous price"
        );
    }

    #[test]
    fn failed_fetches_neither_get_nor_break_a_change() {
        let rows = vec![
            row("a", 3, Some(120)),
            row("a", 2, None),
            row("a", 1, Some(100)),
        ];
        assert_eq!(
            deltas(&rows),
            vec![Some(20), None, None],
            "a failure is skipped, so the change bridges across it"
        );
    }

    #[test]
    fn manual_prices_take_part_in_changes_like_any_other() {
        // A manual snapshot is only distinguished in the status badge; for the purpose of
        // "what changed since last time" it is an ordinary recorded price.
        let mut rows = vec![row("a", 2, Some(90)), row("a", 1, Some(100))];
        rows[0].manual = true;
        assert_eq!(deltas(&rows), vec![Some(-10), None]);
    }

    #[test]
    fn handles_an_empty_history() {
        assert!(deltas(&[]).is_empty());
    }
}
