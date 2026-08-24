use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::api::history::item_history;
use crate::api::refresh::RefreshItemNow;
use crate::api::sources::{
    CreateSource, DeleteSource, RecordFromHtml, RecordPrice, TestSource, UpdateSource,
};
use crate::components::action_error;
use crate::components::chart::PriceChart;
use crate::components::price_table::PriceTable;
use crate::fmt::{format_cents, format_datetime};
use crate::models::{ItemSource, SourceInput};

/// What a [`SourceEditor`] needs from the page it sits on, bundled into one prop.
///
/// These actions are shared across every row because each one rewrites the source list,
/// so the page's `Resource` has to watch their versions to know when to refetch.
///
/// Recording a price is deliberately *not* here. Those actions are per-row, like
/// `TestSource`, because their result is displayed in the row that ran it and a shared
/// action would splash it across every other row. `recorded` is how a row tells the page
/// something landed: rows bump it, the `Resource` watches it.
#[derive(Clone, Copy)]
struct SourceActions {
    create: ServerAction<CreateSource>,
    update: ServerAction<UpdateSource>,
    remove: ServerAction<DeleteSource>,
    recorded: RwSignal<usize>,
}

#[component]
pub fn ItemDetailPage() -> impl IntoView {
    let params = use_params_map();
    let item_id = move || params.read().get("id").unwrap_or_default();

    let refresh = ServerAction::<RefreshItemNow>::new();
    let actions = SourceActions {
        create: ServerAction::new(),
        update: ServerAction::new(),
        remove: ServerAction::new(),
        recorded: RwSignal::new(0),
    };

    let history = Resource::new(
        move || {
            (
                item_id(),
                refresh.version().get(),
                actions.create.version().get(),
                actions.update.version().get(),
                actions.remove.version().get(),
                actions.recorded.get(),
            )
        },
        |(id, ..)| async move { item_history(id).await },
    );

    let error = move || {
        action_error(refresh.value().get())
            .or_else(|| action_error(actions.create.value().get()))
            .or_else(|| action_error(actions.update.value().get()))
            .or_else(|| action_error(actions.remove.value().get()))
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
                                     its page. Use Test to check one before saving. If a retailer \
                                     blocks this tracker, tick \"Enter prices by hand\" and supply \
                                     its prices yourself."
                                </p>
                                <div class="sources">
                                    {item.sources
                                        .into_iter()
                                        .map(|source| view! {
                                            <SourceEditor
                                                item_id=id.clone()
                                                source=source
                                                actions=actions
                                            />
                                        })
                                        .collect::<Vec<_>>()}
                                    <SourceEditor item_id=id.clone() actions=actions/>
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
    actions: SourceActions,
) -> impl IntoView {
    let SourceActions {
        create,
        update,
        remove,
        recorded,
    } = actions;

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
    let manual = RwSignal::new(source.as_ref().map(|s| s.manual).unwrap_or(false));

    // Testing is per row: a shared action would splash one row's result across all of them.
    // Recording is per row for the same reason -- with two manual retailers on one item, a
    // shared action would show what one of them extracted underneath both.
    let test = ServerAction::<TestSource>::new();
    let record_price = ServerAction::<RecordPrice>::new();
    let record_html = ServerAction::<RecordFromHtml>::new();

    // What the two manual inputs hold.
    let pasted_html = RwSignal::new(String::new());
    let typed_price = RwSignal::new(String::new());

    // Clear an input only once its price is actually in, and tell the page to refetch so
    // the chart, table and best price pick it up. Holding onto a *failed* paste matters:
    // re-copying a whole page because the selector was wrong is a real cost, and the error
    // shown alongside it is what tells the user to go fix the selector.
    Effect::new(move |_| {
        if matches!(record_html.value().get(), Some(Ok(o)) if o.price_cents.is_some()) {
            pasted_html.set(String::new());
            recorded.update(|n| *n += 1);
        }
    });
    Effect::new(move |_| {
        if matches!(record_price.value().get(), Some(Ok(()))) {
            typed_price.set(String::new());
            recorded.update(|n| *n += 1);
        }
    });

    let latest = source.as_ref().and_then(|s| s.latest.clone());

    let gather = move || SourceInput {
        label: label.get(),
        url: url.get(),
        css_selector: selector.get(),
        price_regex: Some(regex.get()).filter(|r| !r.trim().is_empty()),
        active: active.get(),
        manual: manual.get(),
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
                    manual.set(false);
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

    let test_result = move || extraction_result(test.value().get(), "Read");

    // Shown in the row rather than the page banner, since the action is per row.
    let typed_result = move || {
        record_price.value().get().map(|result| match result {
            Err(e) => {
                view! { <p class="error">{action_error(Some(Err::<(), _>(e)))}</p> }.into_any()
            }
            Ok(()) => view! { <p class="notice">"Price recorded."</p> }.into_any(),
        })
    };
    // Same shape, same diagnostics -- only the verb differs, because this one recorded it.
    let paste_result = move || extraction_result(record_html.value().get(), "Recorded");

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
                    {move || if manual.get() { "CSS selector (for pasted HTML)" }
                             else { "CSS selector" }}
                    // Optional in manual mode: prices can be typed in instead. It is still
                    // what reads a price out of a pasted page, so the field stays.
                    <input
                        type="text"
                        placeholder="span.price, or meta[itemprop=price]"
                        required=move || !manual.get()
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
                    "Include this retailer"
                </label>
                <label class="checkbox">
                    <input
                        type="checkbox"
                        prop:checked=manual
                        on:change=move |ev| manual.set(event_target_checked(&ev))
                    />
                    "Enter prices by hand (this retailer blocks the tracker)"
                </label>
            </div>

            <div class="source-actions">
                <button type="submit">{if is_new { "Add retailer" } else { "Save" }}</button>
                // Test fetches the page, which is exactly what does not work for a manual
                // source. Pasting its HTML is the equivalent check there.
                <Show when=move || !manual.get()>
                    <button
                        type="button"
                        class="secondary"
                        disabled=move || test.pending().get()
                        on:click=move |_| { test.dispatch(TestSource { input: gather() }); }
                    >
                        {move || if test.pending().get() { "Testing..." } else { "Test" }}
                    </button>
                </Show>
                <Show when=move || !is_new>
                    <button type="button" class="danger linklike" on:click=on_delete.clone()>
                        "Delete"
                    </button>
                </Show>
            </div>

            // Only offered once the source exists, since recording needs its id and its
            // stored selector.
            <Show when=move || manual.get() && !is_new>
                <div class="source-manual">
                    <label class="wide">
                        "Page source"
                        <span class="muted">
                            "Open the product page, press Ctrl+U, then Ctrl+A and Ctrl+C, \
                             and paste it here. The selector above reads the price out of it."
                        </span>
                        <textarea
                            rows="4"
                            placeholder="<!DOCTYPE html>..."
                            prop:value=pasted_html
                            on:input=move |ev| pasted_html.set(event_target_value(&ev))
                        ></textarea>
                    </label>

                    <div class="source-actions">
                        <button
                            type="button"
                            disabled=move || {
                                record_html.pending().get()
                                    || pasted_html.with(|h| h.trim().is_empty())
                            }
                            on:click={
                                let existing_id = existing_id.clone();
                                move |_| {
                                    let Some(id) = existing_id.clone() else { return };
                                    record_html.dispatch(RecordFromHtml {
                                        source_id: id,
                                        html: pasted_html.get(),
                                    });
                                }
                            }
                        >
                            {move || if record_html.pending().get() {
                                "Extracting..."
                            } else {
                                "Extract & record"
                            }}
                        </button>
                    </div>

                    <label class="wide">
                        "Or enter it directly"
                        <input
                            type="text"
                            placeholder="R 1 299,00"
                            prop:value=typed_price
                            on:input=move |ev| typed_price.set(event_target_value(&ev))
                        />
                    </label>

                    <div class="source-actions">
                        <button
                            type="button"
                            class="secondary"
                            disabled=move || {
                                record_price.pending().get()
                                    || typed_price.with(|p| p.trim().is_empty())
                            }
                            on:click={
                                let existing_id = existing_id.clone();
                                move |_| {
                                    let Some(id) = existing_id.clone() else { return };
                                    record_price.dispatch(RecordPrice {
                                        source_id: id,
                                        price: typed_price.get(),
                                    });
                                }
                            }
                        >
                            {move || if record_price.pending().get() {
                                "Recording..."
                            } else {
                                "Record"
                            }}
                        </button>
                    </div>
                </div>
            </Show>

            {move || latest.clone().map(|status| {
                if status.ok {
                    let price = status
                        .price_cents
                        .map(|c| format!("{}.{:02}", c / 100, c % 100))
                        .unwrap_or_default();
                    // Driven by the snapshot rather than the source's current mode, so it
                    // stays right for one that was scraped before the retailer blocked us.
                    let verb = if status.manual { "Last recorded " } else { "Last checked " };
                    view! {
                        <p class="source-status">
                            {verb} {format_datetime(status.fetched_at)}
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
            {paste_result}
            {typed_result}
        </form>
    }
}

/// Renders the outcome of a selector run, whether it came from Test or from pasted HTML.
///
/// `verb` is the only difference between the two: Test only "Read" a price, while pasting
/// "Recorded" it. Cents are shown unformatted on purpose -- this is a diagnostic view, and
/// the item's currency is not what is being checked.
fn extraction_result(
    value: Option<Result<crate::models::SourceTest, ServerFnError>>,
    verb: &'static str,
) -> Option<AnyView> {
    value.map(|result| match result {
        Err(e) => view! { <p class="error">{e.to_string()}</p> }.into_any(),
        Ok(outcome) => {
            let detail = outcome
                .matched_text
                .map(|t| format!("Matched: {t:?}"))
                .unwrap_or_default();

            match outcome.price_cents {
                Some(cents) => view! {
                    <p class="notice">
                        {format!("{verb} {}.{:02}. ", cents / 100, cents % 100)}
                        <span class="muted">{detail}</span>
                    </p>
                }
                .into_any(),
                None => view! {
                    <p class="error">
                        {outcome.error.unwrap_or_else(|| "No price found".into())}
                        " " <span class="muted">{detail}</span>
                    </p>
                }
                .into_any(),
            }
        }
    })
}
