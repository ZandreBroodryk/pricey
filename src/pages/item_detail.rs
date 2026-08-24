//! The `/items/:id` page.
//!
//! # Components here return [`AnyView`]
//!
//! A component returning `impl IntoView` returns a *concrete* type spelling out its whole
//! markup tree, so a parent's type contains every child's type in full. Laying that out
//! recurses once per level -- through `RenderHtml::resolve`'s future on the server, and
//! `IntoAny::hydrate_async`'s in the wasm build -- and this page ran roughly ten levels past
//! rustc's default recursion limit in release builds, which is the "queries overflow the
//! depth limit" error. Debug builds do not reach that codegen, so it only shows up on a
//! release build.
//!
//! Splitting the page into smaller components does not fix it on its own: the same tree is
//! still nested, only spelled across more functions. Returning `AnyView` is what bounds it,
//! because the tree is erased at the boundary and cannot reach the parent's type. The cost
//! is one boxed view per instance -- a handful per page, one per retailer row.
//!
//! Keep new components in this file to the same rule, and reach for it in shared components
//! that render something substantial ([`PriceChart`] and [`PriceTable`] both do), rather
//! than raising `recursion_limit`: that only buys room until the next field is added.

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
use crate::models::{ItemHistory, ItemSource, SourceInput, SourceStatus};

/// What a [`SourceEditor`] needs from the page it sits on, bundled into one prop.
///
/// These actions are shared across every row because each one rewrites the source list,
/// so the page's `Resource` has to watch their versions to know when to refetch.
///
/// Recording a price is deliberately *not* here. Those actions live in [`ManualEntry`],
/// one set per row, because their result is displayed in the row that ran it and a shared
/// action would splash it across every other row. `recorded` is how a row tells the page
/// something landed: rows bump it, the `Resource` watches it.
#[derive(Clone, Copy)]
struct SourceActions {
    create: ServerAction<CreateSource>,
    update: ServerAction<UpdateSource>,
    remove: ServerAction<DeleteSource>,
    recorded: RwSignal<usize>,
}

/// The editable state of one retailer row.
///
/// Bundled so the pieces of a row can each be handed the whole set rather than a prop per
/// field, and so the two places that read every field at once -- submitting and testing --
/// cannot disagree about what a row currently says.
#[derive(Clone, Copy)]
struct SourceFields {
    label: RwSignal<String>,
    url: RwSignal<String>,
    selector: RwSignal<String>,
    regex: RwSignal<String>,
    active: RwSignal<bool>,
    manual: RwSignal<bool>,
}

impl SourceFields {
    /// Seeds the fields from the stored row, after which they are left alone: re-seeding
    /// them when the page refetches would overwrite whatever is being typed.
    fn seed(from: Option<&ItemSource>) -> Self {
        Self {
            label: RwSignal::new(from.map(|s| s.label.clone()).unwrap_or_default()),
            url: RwSignal::new(from.map(|s| s.url.clone()).unwrap_or_default()),
            selector: RwSignal::new(from.map(|s| s.css_selector.clone()).unwrap_or_default()),
            regex: RwSignal::new(from.and_then(|s| s.price_regex.clone()).unwrap_or_default()),
            active: RwSignal::new(from.map(|s| s.active).unwrap_or(true)),
            manual: RwSignal::new(from.map(|s| s.manual).unwrap_or(false)),
        }
    }

    fn gather(self) -> SourceInput {
        SourceInput {
            label: self.label.get(),
            url: self.url.get(),
            css_selector: self.selector.get(),
            price_regex: Some(self.regex.get()).filter(|r| !r.trim().is_empty()),
            active: self.active.get(),
            manual: self.manual.get(),
        }
    }

    /// Empties the row so it is ready for the next retailer. "Include this retailer" keeps
    /// its default rather than being cleared, since off is not a sensible starting point.
    fn clear(self) {
        self.label.set(String::new());
        self.url.set(String::new());
        self.selector.set(String::new());
        self.regex.set(String::new());
        self.manual.set(false);
    }
}

#[component]
pub fn ItemDetailPage() -> AnyView {
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
                <ItemBody
                    item_id=Signal::derive(item_id)
                    history=history
                    refresh=refresh
                    actions=actions
                />
            </Transition>
        </section>
    }
    .into_any()
}

/// The loaded page.
///
/// Split out from [`ItemDetailPage`] so the signals below are created *inside* the
/// `<Transition/>`. A resource read only registers with a suspense boundary when the owner
/// it runs under is that boundary's, and a `Memo` runs under the owner it was created in --
/// so memos over `history` have to be born in here, not in the parent.
#[component]
fn ItemBody(
    item_id: Signal<String>,
    history: Resource<Result<ItemHistory, ServerFnError>>,
    refresh: ServerAction<RefreshItemNow>,
    actions: SourceActions,
) -> AnyView {
    // The resource is read once, here, and everything below is derived from it. That is
    // what lets the retailer rows be driven by signals instead of rebuilt on every refetch:
    // recording a price refetches the page, and a rebuild would throw away the notice the
    // row had just put up along with any edits in progress in the other rows.
    let loaded = Memo::new(move |_| match history.get() {
        Some(Ok(history)) => Some(history),
        _ => None,
    });
    let load_error = move || match history.get() {
        Some(Err(e)) => Some(e.to_string()),
        _ => None,
    };
    let sources = Memo::new(move |_| {
        loaded
            .get()
            .map(|history| history.item.sources)
            .unwrap_or_default()
    });

    view! {
        <Show when=move || load_error().is_some()>
            <p class="error" role="alert">{load_error}</p>
        </Show>

        <Show when=move || loaded.with(Option::is_some)>
            <ItemHead loaded=loaded item_id=item_id refresh=refresh/>
            <RetailerList sources=sources item_id=item_id actions=actions/>
            <PriceHistory loaded=loaded/>
        </Show>
    }
    .into_any()
}

/// Name, best price, target, and the button that refreshes this item on demand.
#[component]
fn ItemHead(
    loaded: Memo<Option<ItemHistory>>,
    item_id: Signal<String>,
    refresh: ServerAction<RefreshItemNow>,
) -> AnyView {
    let name = move || loaded.get().map(|h| h.item.name).unwrap_or_default();
    let best = move || {
        loaded
            .get()
            .map(|h| {
                h.item
                    .best
                    .map(|b| {
                        format!(
                            "{} at {} ({})",
                            format_cents(b.price_cents, &h.item.currency),
                            b.label,
                            format_datetime(b.fetched_at),
                        )
                    })
                    .unwrap_or_else(|| "No price recorded yet".to_string())
            })
            .unwrap_or_default()
    };
    let target = move || {
        loaded.get().and_then(|h| {
            h.item
                .target_price_cents
                .map(|cents| format!("Target {}", format_cents(cents, &h.item.currency)))
        })
    };

    view! {
        <div class="page-head">
            <div>
                <h1>{name}</h1>
                <p class="subtitle">{best}</p>
                <Show when=move || target().is_some()>
                    <p class="muted">{target}</p>
                </Show>
            </div>
            <button
                class="secondary"
                disabled=move || refresh.pending().get()
                on:click=move |_| {
                    refresh.dispatch(RefreshItemNow { item_id: item_id.get() });
                }
            >
                {move || if refresh.pending().get() { "Refreshing..." } else { "Refresh now" }}
            </button>
        </div>
    }
    .into_any()
}

/// Every retailer tracked for the item, plus the empty row that adds another.
#[component]
fn RetailerList(
    sources: Memo<Vec<ItemSource>>,
    item_id: Signal<String>,
    actions: SourceActions,
) -> AnyView {
    view! {
        <h2>"Retailers"</h2>
        <p class="muted">
            "Each retailer needs a CSS selector pointing at the price on its page. Use Test \
             to check one before saving. If a retailer blocks this tracker, tick \"Enter \
             prices by hand\", save, and supply its prices yourself."
        </p>
        <div class="sources">
            <For
                each=move || sources.get()
                key=|source: &ItemSource| source.id.clone()
                children=move |source| {
                    // The row reads itself back out of the list rather than closing over
                    // this value. It is deliberately not rebuilt when the list refetches --
                    // that is what preserves what the user has typed -- so a save or a newly
                    // recorded price has to reach it as a signal.
                    let id = source.id.clone();
                    let stored = Signal::derive(move || {
                        sources.with(|list| list.iter().find(|s| s.id == id).cloned())
                    });
                    view! { <SourceEditor item_id=item_id source=stored actions=actions/> }
                }
            />
            <SourceEditor item_id=item_id actions=actions/>
        </div>
    }
    .into_any()
}

/// The chart and the table under it, rebuilt whenever new prices arrive.
#[component]
fn PriceHistory(loaded: Memo<Option<ItemHistory>>) -> AnyView {
    view! {
        {move || loaded.get().map(|h| {
            let currency = h.item.currency;
            view! {
                <h2>"Price history"</h2>
                <PriceChart series=h.series currency=currency.clone()/>
                <PriceTable rows=h.rows currency=currency/>
            }
        })}
    }
    .into_any()
}

/// One retailer row. Doubles as the "add a retailer" form when `source` is absent, so the
/// two paths cannot drift apart in validation or layout.
#[component]
fn SourceEditor(
    item_id: Signal<String>,
    /// The stored row this editor edits; `None` is the "add a retailer" row.
    ///
    /// It arrives as a signal rather than a value because the row outlives the page's
    /// fetches: not rebuilding it is what keeps a half-typed selector and a just-shown
    /// "recorded" notice alive across a refetch, so saved values and the latest snapshot
    /// have to reach it this way instead.
    #[prop(optional)]
    source: Option<Signal<Option<ItemSource>>>,
    actions: SourceActions,
) -> AnyView {
    let SourceActions {
        create,
        update,
        remove,
        recorded,
    } = actions;

    let initial = source.and_then(|s| s.get_untracked());
    let existing_id = StoredValue::new(initial.as_ref().map(|s| s.id.clone()));
    let is_new = existing_id.with_value(Option::is_none);
    let fields = SourceFields::seed(initial.as_ref());

    // Reactive views of what is *saved*, as opposed to what is currently in the fields.
    let stored = move || source.and_then(|s| s.get());
    let latest = move || stored().and_then(|s| s.latest);
    // The manual panel follows the saved flag rather than the checkbox: until the source is
    // saved as manual the runner still refreshes it every hour, so offering to record its
    // prices by hand would be describing a state that does not exist yet.
    let is_manual = move || stored().is_some_and(|s| s.manual);

    // Per row, like recording: a shared action would splash one row's result across all of
    // them.
    let test = ServerAction::<TestSource>::new();

    let save = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        match existing_id.get_value() {
            Some(id) => {
                update.dispatch(UpdateSource {
                    id,
                    input: fields.gather(),
                });
            }
            None => {
                create.dispatch(CreateSource {
                    item_id: item_id.get(),
                    input: fields.gather(),
                });
                fields.clear();
            }
        }
    };

    let on_delete = move |_| {
        if let Some(id) = existing_id.get_value() {
            remove.dispatch(DeleteSource { id });
        }
    };

    // The add row has nothing to record a price against, so the panel does not exist for it
    // at all. `Show` memoizes its condition, which is what keeps the panel -- and the notice
    // it is showing -- alive across the refetch that recording triggers.
    let manual_panel = existing_id.get_value().map(|source_id| {
        view! {
            <Show when=move || is_manual()>
                <ManualEntry source_id=source_id.clone() fields=fields recorded=recorded/>
            </Show>
        }
    });

    view! {
        <form class="source-editor" class:is-new=is_new on:submit=save>
            <SourceGrid fields=fields/>

            <div class="source-actions">
                <button type="submit">{if is_new { "Add retailer" } else { "Save" }}</button>
                // Test fetches the page, which is exactly what does not work for a manual
                // source. Pasting its HTML is the equivalent check there.
                <Show when=move || !fields.manual.get()>
                    <button
                        type="button"
                        class="secondary"
                        disabled=move || test.pending().get()
                        on:click=move |_| { test.dispatch(TestSource { input: fields.gather() }); }
                    >
                        {move || if test.pending().get() { "Testing..." } else { "Test" }}
                    </button>
                </Show>
                <Show when=move || !is_new>
                    <button type="button" class="danger linklike" on:click=on_delete>
                        "Delete"
                    </button>
                </Show>
            </div>

            {manual_panel}

            <LastSnapshot latest=Signal::derive(latest)/>

            {move || extraction_result(test.value().get(), "Read")}
        </form>
    }
    .into_any()
}

/// The fields every retailer has, scraped or not.
#[component]
fn SourceGrid(fields: SourceFields) -> AnyView {
    let SourceFields {
        label,
        url,
        selector,
        regex,
        active,
        manual,
    } = fields;

    view! {
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
                // Optional in manual mode: prices can be typed in instead. It is still what
                // reads a price out of a pasted page, so the field stays.
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
    }
    .into_any()
}

/// Getting a price in by hand, for a retailer that blocks the tracker.
///
/// Owns its own actions and inputs: with two manual retailers on one item, sharing them
/// would show what one of them extracted underneath both.
#[component]
fn ManualEntry(source_id: String, fields: SourceFields, recorded: RwSignal<usize>) -> AnyView {
    let source_id = StoredValue::new(source_id);
    let SourceFields {
        selector, regex, ..
    } = fields;

    let record_price = ServerAction::<RecordPrice>::new();
    let record_html = ServerAction::<RecordFromHtml>::new();

    // What the two inputs hold.
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
        if matches!(record_price.value().get(), Some(Ok(_))) {
            typed_price.set(String::new());
            recorded.update(|n| *n += 1);
        }
    });

    // Both ways of triggering the typed path go through one closure, so the button and the
    // keyboard cannot end up doing different things.
    let typing_blocked =
        move || record_price.pending().get() || typed_price.with(|p| p.trim().is_empty());
    let record_typed = move || {
        if record_price.pending().get_untracked()
            || typed_price.with_untracked(|p| p.trim().is_empty())
        {
            return;
        }
        record_price.dispatch(RecordPrice {
            source_id: source_id.get_value(),
            price: typed_price.get_untracked(),
        });
    };
    let record_pasted = move || {
        record_html.dispatch(RecordFromHtml {
            source_id: source_id.get_value(),
            html: pasted_html.get_untracked(),
            // The selector as it is shown, the same one Test would use. The field is
            // editable and sits right above the paste box, so reading the stored selector
            // instead would extract with something the user is not looking at.
            css_selector: selector.get_untracked(),
            price_regex: Some(regex.get_untracked()).filter(|r| !r.trim().is_empty()),
        });
    };

    view! {
        <div class="source-manual">
            <label class="wide">
                "Page source"
                <span class="muted">
                    "Open the product page, press Ctrl+U, then Ctrl+A and Ctrl+C, and paste \
                     it here. The selector above reads the price out of it."
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
                        record_html.pending().get() || pasted_html.with(|h| h.trim().is_empty())
                    }
                    on:click=move |_| record_pasted()
                >
                    {move || if record_html.pending().get() {
                        "Extracting..."
                    } else {
                        "Extract & record"
                    }}
                </button>
            </div>

            // Both paths report the price that was stored, which for a typed one is the only
            // chance to notice that "Was R1 999, now R899" parsed as 1999.
            {move || extraction_result(record_html.value().get(), "Recorded")}

            <label class="wide">
                "Or enter it directly"
                <input
                    type="text"
                    placeholder="R 1 299,00"
                    prop:value=typed_price
                    on:input=move |ev| typed_price.set(event_target_value(&ev))
                    // This input sits inside the row's form, where Enter means Save -- which
                    // would record nothing and clear the field, so the price would silently
                    // never be written. On a phone, the path this field exists for, Enter is
                    // the natural key here.
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            record_typed();
                        }
                    }
                />
            </label>

            <div class="source-actions">
                <button
                    type="button"
                    class="secondary"
                    disabled=typing_blocked
                    on:click=move |_| record_typed()
                >
                    {move || if record_price.pending().get() { "Recording..." } else { "Record" }}
                </button>
            </div>

            {move || extraction_result(record_price.value().get(), "Recorded")}
        </div>
    }
    .into_any()
}

/// When this retailer was last read, and what came back.
#[component]
fn LastSnapshot(latest: Signal<Option<SourceStatus>>) -> AnyView {
    (move || {
        latest.get().map(|status| {
            if status.ok {
                let price = status
                    .price_cents
                    .map(|c| format!("{}.{:02}", c / 100, c % 100))
                    .unwrap_or_default();
                // Driven by the snapshot rather than the source's current mode, so it stays
                // right for one that was scraped before the retailer blocked us.
                let verb = if status.manual {
                    "Last recorded "
                } else {
                    "Last checked "
                };
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
                        {status.error.unwrap_or_else(|| "unknown error".into())}
                    </p>
                }
                .into_any()
            }
        })
    })
    .into_any()
}

/// Renders the outcome of a recorded or tested price, whoever produced it.
///
/// `verb` is the only difference between the callers: Test only "Read" a price, while the
/// two manual paths "Recorded" it. Cents are shown unformatted on purpose -- this is a
/// diagnostic view, and the item's currency is not what is being checked.
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
