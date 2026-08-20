use leptos::prelude::*;

use crate::api::items::{list_items, CreateItem, DeleteItem};
use crate::api::refresh::RefreshAllNow;
use crate::components::action_error;
use crate::fmt::{format_cents, format_datetime, parse_price};
use crate::models::{ItemInput, WishlistItem};

#[component]
pub fn WishlistPage() -> impl IntoView {
    let create = ServerAction::<CreateItem>::new();
    let delete = ServerAction::<DeleteItem>::new();
    let refresh = ServerAction::<RefreshAllNow>::new();

    // Any mutation invalidates the list, so the resource tracks all three versions.
    let items = Resource::new(
        move || {
            (
                create.version().get(),
                delete.version().get(),
                refresh.version().get(),
            )
        },
        |_| async move { list_items().await },
    );

    let error = move || {
        action_error(create.value().get())
            .or_else(|| action_error(delete.value().get()))
            .or_else(|| action_error(refresh.value().get()))
    };

    let refresh_summary = move || {
        refresh.value().get().and_then(Result::ok).map(|report| {
            format!(
                "Checked {} source{}: {} succeeded, {} failed.",
                report.attempted,
                if report.attempted == 1 { "" } else { "s" },
                report.succeeded,
                report.failed
            )
        })
    };

    view! {
        <section class="page">
            <div class="page-head">
                <h1>"Wishlist"</h1>
                <button
                    class="secondary"
                    disabled=move || refresh.pending().get()
                    on:click=move |_| { refresh.dispatch(RefreshAllNow {}); }
                >
                    {move || if refresh.pending().get() { "Refreshing..." } else { "Refresh all" }}
                </button>
            </div>

            <Show when=move || error().is_some()>
                <p class="error" role="alert">{error}</p>
            </Show>
            <Show when=move || refresh_summary().is_some()>
                <p class="notice">{refresh_summary}</p>
            </Show>

            <AddItemForm action=create/>

            <Transition fallback=|| view! { <p class="loading">"Loading your wishlist..."</p> }>
                {move || {
                    items.get().map(|result| match result {
                        Err(e) => view! {
                            <p class="error" role="alert">{e.to_string()}</p>
                        }.into_any(),
                        Ok(items) if items.is_empty() => view! {
                            <p class="empty">
                                "Nothing tracked yet. Add an item above, then give it a retailer to watch."
                            </p>
                        }.into_any(),
                        Ok(items) => view! { <ItemTable items=items delete=delete/> }.into_any(),
                    })
                }}
            </Transition>
        </section>
    }
}

#[component]
fn ItemTable(items: Vec<WishlistItem>, delete: ServerAction<DeleteItem>) -> impl IntoView {
    view! {
        <table class="item-table">
            <thead>
                <tr>
                    <th>"Item"</th>
                    <th>"Best price"</th>
                    <th>"Where"</th>
                    <th>"Sources"</th>
                    <th>"Last checked"</th>
                    <th></th>
                </tr>
            </thead>
            <tbody>
                {items
                    .into_iter()
                    .map(|item| {
                        let id = item.id.clone();
                        let name = item.name.clone();
                        let currency = item.currency.clone();

                        let best_price = item
                            .best
                            .as_ref()
                            .map(|b| format_cents(b.price_cents, &currency))
                            .unwrap_or_else(|| "-".to_string());
                        let where_from = item
                            .best
                            .as_ref()
                            .map(|b| b.label.clone())
                            .unwrap_or_else(|| "-".to_string());
                        let checked = item
                            .best
                            .as_ref()
                            .map(|b| format_datetime(b.fetched_at))
                            .unwrap_or_else(|| "never".to_string());

                        // A target that has been met is the whole point of tracking, so
                        // it is called out rather than left for the reader to compare.
                        let met_target = matches!(
                            (item.target_price_cents, item.best.as_ref()),
                            (Some(target), Some(best)) if best.price_cents <= target
                        );

                        let confirm = format!("Delete \"{name}\" and its whole price history?");
                        let on_delete = move |_| {
                            if window_confirm(&confirm) {
                                delete.dispatch(DeleteItem { id: id.clone() });
                            }
                        };

                        view! {
                            <tr class:inactive=!item.active>
                                <td>
                                    <a href=format!("/items/{}", item.id)>{item.name}</a>
                                    <Show when=move || !item.active>
                                        <span class="badge">"paused"</span>
                                    </Show>
                                </td>
                                <td class="col-price">
                                    <span class:on-target=met_target>{best_price}</span>
                                </td>
                                <td>{where_from}</td>
                                <td class="col-count">{item.source_count}</td>
                                <td class="col-when">{checked}</td>
                                <td>
                                    <button class="danger linklike" on:click=on_delete>
                                        "Delete"
                                    </button>
                                </td>
                            </tr>
                        }
                    })
                    .collect::<Vec<_>>()}
            </tbody>
        </table>
    }
}

#[component]
fn AddItemForm(action: ServerAction<CreateItem>) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let currency = RwSignal::new("ZAR".to_string());
    let target = RwSignal::new(String::new());
    let local_error = RwSignal::new(Option::<String>::None);

    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        local_error.set(None);

        // A blank target is fine; a target that cannot be read is not, and silently
        // dropping it would be worse than saying so.
        let raw = target.get();
        let target_price_cents = if raw.trim().is_empty() {
            None
        } else {
            match parse_price(&raw) {
                Some(cents) => Some(cents),
                None => {
                    local_error.set(Some(format!("Could not read {raw:?} as a price.")));
                    return;
                }
            }
        };

        action.dispatch(CreateItem {
            input: ItemInput {
                name: name.get(),
                currency: currency.get(),
                target_price_cents,
                notes: None,
                active: true,
            },
        });

        name.set(String::new());
        target.set(String::new());
    };

    view! {
        <form class="add-item" on:submit=submit>
            <label>
                "Item"
                <input
                    type="text"
                    placeholder="Mechanical keyboard"
                    required
                    prop:value=name
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
            </label>
            <label>
                "Currency"
                <input
                    type="text"
                    size="5"
                    prop:value=currency
                    on:input=move |ev| currency.set(event_target_value(&ev))
                />
            </label>
            <label>
                "Target price"
                <input
                    type="text"
                    placeholder="optional"
                    prop:value=target
                    on:input=move |ev| target.set(event_target_value(&ev))
                />
            </label>
            <button type="submit" disabled=move || action.pending().get()>"Add item"</button>

            <Show when=move || local_error.get().is_some()>
                <p class="error" role="alert">{move || local_error.get()}</p>
            </Show>
        </form>
    }
}

/// Browser confirmation dialog, with a server-side no-op so SSR can render the handler.
fn window_confirm(message: &str) -> bool {
    #[cfg(feature = "hydrate")]
    {
        leptos::web_sys::window()
            .and_then(|w| w.confirm_with_message(message).ok())
            .unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = message;
        false
    }
}
