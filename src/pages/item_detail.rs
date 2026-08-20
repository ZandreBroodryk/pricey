use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::api::history::item_history;
use crate::api::refresh::RefreshItemNow;
use crate::api::sources::{CreateSource, DeleteSource, TestSource, UpdateSource};
use crate::components::action_error;
use crate::components::chart::PriceChart;
use crate::components::price_table::PriceTable;
use crate::fmt::{format_cents, format_datetime};
use crate::models::{ItemSource, SourceInput};

#[component]
pub fn ItemDetailPage() -> impl IntoView {
    let params = use_params_map();
    let item_id = move || params.read().get("id").unwrap_or_default();

    let refresh = ServerAction::<RefreshItemNow>::new();
    let create = ServerAction::<CreateSource>::new();
    let update = ServerAction::<UpdateSource>::new();
    let remove = ServerAction::<DeleteSource>::new();

    let history = Resource::new(
        move || {
            (
                item_id(),
                refresh.version().get(),
                create.version().get(),
                update.version().get(),
                remove.version().get(),
            )
        },
        |(id, ..)| async move { item_history(id).await },
    );

    let error = move || {
        action_error(refresh.value().get())
            .or_else(|| action_error(create.value().get()))
            .or_else(|| action_error(update.value().get()))
            .or_else(|| action_error(remove.value().get()))
    };

    view! {
        <section class="page">
            <p class="crumb"><a href="/">"< Back to wishlist"</a></p>

            <Show when=move || error().is_some()>
                <p class="error" role="alert">{error}</p>
            </Show>

            <Transition fallback=|| view! { <p class="loading">"Loading..."</p> }>
                {move || {
                    history.get().map(|result| match result {
                        Err(e) => view! {
                            <p class="error" role="alert">{e.to_string()}</p>
                        }.into_any(),
                        Ok(history) => {
                            let item = history.item;
                            let currency = item.currency.clone();
                            let id = item.id.clone();

                            let best = item
                                .best
                                .as_ref()
                                .map(|b| {
                                    format!(
                                        "{} at {} ({})",
                                        format_cents(b.price_cents, &currency),
                                        b.label,
                                        format_datetime(b.fetched_at),
                                    )
                                })
                                .unwrap_or_else(|| "No price recorded yet".to_string());

                            let target = StoredValue::new(item.target_price_cents.map(|cents| {
                                format!("Target {}", format_cents(cents, &currency))
                            }));

                            let refresh_id = id.clone();

                            view! {
                                <div class="page-head">
                                    <div>
                                        <h1>{item.name}</h1>
                                        <p class="subtitle">{best}</p>
                                        <Show when=move || target.with_value(Option::is_some)>
                                            <p class="muted">{move || target.get_value()}</p>
                                        </Show>
                                    </div>
                                    <button
                                        class="secondary"
                                        disabled=move || refresh.pending().get()
                                        on:click=move |_| {
                                            refresh.dispatch(RefreshItemNow {
                                                item_id: refresh_id.clone(),
                                            });
                                        }
                                    >
                                        {move || if refresh.pending().get() {
                                            "Refreshing..."
                                        } else {
                                            "Refresh now"
                                        }}
                                    </button>
                                </div>

                                <h2>"Retailers"</h2>
                                <p class="muted">
                                    "Each retailer needs a CSS selector pointing at the price on \
                                     its page. Use Test to check one before saving."
                                </p>
                                <div class="sources">
                                    {item.sources
                                        .into_iter()
                                        .map(|source| view! {
                                            <SourceEditor
                                                item_id=id.clone()
                                                source=source
                                                create=create
                                                update=update
                                                remove=remove
                                            />
                                        })
                                        .collect::<Vec<_>>()}
                                    <SourceEditor
                                        item_id=id.clone()
                                        create=create
                                        update=update
                                        remove=remove
                                    />
                                </div>

                                <h2>"Price history"</h2>
                                <PriceChart series=history.series currency=currency.clone()/>
                                <PriceTable rows=history.rows currency=currency/>
                            }
                            .into_any()
                        }
                    })
                }}
            </Transition>
        </section>
    }
}

/// One retailer row. Doubles as the "add a retailer" form when `source` is absent, so the
/// two paths cannot drift apart in validation or layout.
#[component]
fn SourceEditor(
    item_id: String,
    #[prop(optional)] source: Option<ItemSource>,
    create: ServerAction<CreateSource>,
    update: ServerAction<UpdateSource>,
    remove: ServerAction<DeleteSource>,
) -> impl IntoView {
    let existing_id = source.as_ref().map(|s| s.id.clone());
    let is_new = existing_id.is_none();

    let label = RwSignal::new(source.as_ref().map(|s| s.label.clone()).unwrap_or_default());
    let url = RwSignal::new(source.as_ref().map(|s| s.url.clone()).unwrap_or_default());
    let selector = RwSignal::new(
        source
            .as_ref()
            .map(|s| s.css_selector.clone())
            .unwrap_or_default(),
    );
    let regex = RwSignal::new(
        source
            .as_ref()
            .and_then(|s| s.price_regex.clone())
            .unwrap_or_default(),
    );
    let active = RwSignal::new(source.as_ref().map(|s| s.active).unwrap_or(true));

    // Testing is per row: a shared action would splash one row's result across all of them.
    let test = ServerAction::<TestSource>::new();

    let latest = source.as_ref().and_then(|s| s.latest.clone());

    let gather = move || SourceInput {
        label: label.get(),
        url: url.get(),
        css_selector: selector.get(),
        price_regex: Some(regex.get()).filter(|r| !r.trim().is_empty()),
        active: active.get(),
    };

    let save = {
        let item_id = item_id.clone();
        let existing_id = existing_id.clone();
        move |ev: leptos::ev::SubmitEvent| {
            ev.prevent_default();
            match &existing_id {
                Some(id) => {
                    update.dispatch(UpdateSource {
                        id: id.clone(),
                        input: gather(),
                    });
                }
                None => {
                    create.dispatch(CreateSource {
                        item_id: item_id.clone(),
                        input: gather(),
                    });
                    // Clear the row so it is ready for the next retailer.
                    label.set(String::new());
                    url.set(String::new());
                    selector.set(String::new());
                    regex.set(String::new());
                }
            }
        }
    };

    let on_delete = {
        let existing_id = existing_id.clone();
        move |_| {
            if let Some(id) = &existing_id {
                remove.dispatch(DeleteSource { id: id.clone() });
            }
        }
    };

    let test_result = move || {
        test.value().get().map(|result| match result {
            Err(e) => view! { <p class="error">{e.to_string()}</p> }.into_any(),
            Ok(outcome) => {
                let detail = outcome
                    .matched_text
                    .clone()
                    .map(|t| format!("Matched: {t:?}"))
                    .unwrap_or_default();

                match outcome.price_cents {
                    // Cents are shown unformatted here on purpose: this is a diagnostic
                    // view, and the item's currency is not what is being tested.
                    Some(cents) => view! {
                        <p class="notice">
                            {format!("Read {}.{:02}. ", cents / 100, cents % 100)}
                            <span class="muted">{detail}</span>
                        </p>
                    }
                    .into_any(),
                    None => view! {
                        <p class="error">
                            {outcome.error.clone().unwrap_or_else(|| "No price found".into())}
                            " " <span class="muted">{detail}</span>
                        </p>
                    }
                    .into_any(),
                }
            }
        })
    };

    view! {
        <form class="source-editor" class:is-new=is_new on:submit=save>
            <div class="source-grid">
                <label>
                    "Label"
                    <input
                        type="text"
                        placeholder="defaults to the site"
                        prop:value=label
                        on:input=move |ev| label.set(event_target_value(&ev))
                    />
                </label>
                <label class="wide">
                    "URL"
                    <input
                        type="url"
                        placeholder="https://shop.example/product/123"
                        required
                        prop:value=url
                        on:input=move |ev| url.set(event_target_value(&ev))
                    />
                </label>
                <label class="wide">
                    "CSS selector"
                    <input
                        type="text"
                        placeholder="span.price, or meta[itemprop=price]"
                        required
                        prop:value=selector
                        on:input=move |ev| selector.set(event_target_value(&ev))
                    />
                </label>
                <label class="wide">
                    "Price regex (optional)"
                    <input
                        type="text"
                        placeholder="now R([\\d\\s,.]+)"
                        prop:value=regex
                        on:input=move |ev| regex.set(event_target_value(&ev))
                    />
                </label>
                <label class="checkbox">
                    <input
                        type="checkbox"
                        prop:checked=active
                        on:change=move |ev| active.set(event_target_checked(&ev))
                    />
                    "Check this retailer on refreshes"
                </label>
            </div>

            <div class="source-actions">
                <button type="submit">{if is_new { "Add retailer" } else { "Save" }}</button>
                <button
                    type="button"
                    class="secondary"
                    disabled=move || test.pending().get()
                    on:click=move |_| { test.dispatch(TestSource { input: gather() }); }
                >
                    {move || if test.pending().get() { "Testing..." } else { "Test" }}
                </button>
                <Show when=move || !is_new>
                    <button type="button" class="danger linklike" on:click=on_delete.clone()>
                        "Delete"
                    </button>
                </Show>
            </div>

            {move || latest.clone().map(|status| {
                if status.ok {
                    let price = status
                        .price_cents
                        .map(|c| format!("{}.{:02}", c / 100, c % 100))
                        .unwrap_or_default();
                    view! {
                        <p class="source-status">
                            "Last checked " {format_datetime(status.fetched_at)}
                            " - " {price}
                        </p>
                    }
                    .into_any()
                } else {
                    view! {
                        <p class="source-status is-failed">
                            "Last check failed (" {format_datetime(status.fetched_at)} "): "
                            {status.error.clone().unwrap_or_else(|| "unknown error".into())}
                        </p>
                    }
                    .into_any()
                }
            })}

            {test_result}
        </form>
    }
}
