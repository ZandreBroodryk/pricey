//! A hand-rolled multi-series SVG line chart.
//!
//! Every source is drawn on **shared** axes -- that is the whole point of the view, since
//! comparing retailers only means something when the scales line up. There is no charting
//! dependency and no JavaScript: hover tooltips are native SVG `<title>` elements, so the
//! chart is fully formed in the server-rendered HTML.

use std::collections::HashSet;

use leptos::prelude::*;

use crate::fmt::{format_cents, format_date, format_datetime};
use crate::models::SourceSeries;

const VIEW_W: f64 = 720.0;
const VIEW_H: f64 = 280.0;
const PAD_LEFT: f64 = 78.0;
const PAD_RIGHT: f64 = 18.0;
const PAD_TOP: f64 = 16.0;
const PAD_BOTTOM: f64 = 36.0;
/// How many horizontal gridlines to draw, including both ends.
const Y_TICKS: usize = 5;
/// Number of distinct series colours defined in the stylesheet; assignment wraps.
pub const SERIES_COLOURS: usize = 6;

/// The data-space rectangle the plot covers, after padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub x_min: i64,
    pub x_max: i64,
    pub y_min: i64,
    pub y_max: i64,
}

impl Bounds {
    fn plot_width() -> f64 {
        VIEW_W - PAD_LEFT - PAD_RIGHT
    }

    fn plot_height() -> f64 {
        VIEW_H - PAD_TOP - PAD_BOTTOM
    }

    /// Maps a data point to SVG user-space coordinates.
    pub fn project(&self, x: i64, y: i64) -> (f64, f64) {
        // `expand` guarantees non-zero spans, so neither division can blow up.
        let x_frac = (x - self.x_min) as f64 / (self.x_max - self.x_min) as f64;
        let y_frac = (y - self.y_min) as f64 / (self.y_max - self.y_min) as f64;
        (
            PAD_LEFT + x_frac * Self::plot_width(),
            // SVG y grows downwards; prices should grow upwards.
            PAD_TOP + (1.0 - y_frac) * Self::plot_height(),
        )
    }

    /// The price values at each horizontal gridline, bottom to top.
    pub fn y_ticks(&self) -> Vec<i64> {
        let span = self.y_max - self.y_min;
        (0..Y_TICKS)
            .map(|i| self.y_min + span * i as i64 / (Y_TICKS as i64 - 1))
            .collect()
    }
}

/// Computes shared bounds across every visible series.
///
/// Returns `None` when there is nothing to draw. Degenerate inputs are widened rather than
/// special-cased downstream: a single point, or a price that never moved, would otherwise
/// give a zero-width span and a division by zero in [`Bounds::project`].
pub fn bounds(series: &[SourceSeries]) -> Option<Bounds> {
    let mut points = series.iter().flat_map(|s| s.points.iter()).peekable();
    points.peek()?;

    let (mut x_min, mut x_max) = (i64::MAX, i64::MIN);
    let (mut y_min, mut y_max) = (i64::MAX, i64::MIN);
    for point in points {
        x_min = x_min.min(point.fetched_at);
        x_max = x_max.max(point.fetched_at);
        y_min = y_min.min(point.price_cents);
        y_max = y_max.max(point.price_cents);
    }

    let all_prices_positive = y_min >= 0;

    // Widen a zero span so the single point (or flat line) sits in the middle.
    let (x_min, x_max) = expand(x_min, x_max, 60_000);
    let (mut y_min, y_max) = expand(y_min, y_max, 100);

    // The y axis is deliberately not anchored at zero -- price movements are small next to
    // absolute prices -- but it should not dip below zero either.
    if all_prices_positive {
        y_min = y_min.max(0);
    }

    Some(Bounds {
        x_min,
        x_max,
        y_min,
        y_max,
    })
}

/// Pads a range by 5%, or widens it by `flat_pad` when it has no width at all.
fn expand(min: i64, max: i64, flat_pad: i64) -> (i64, i64) {
    if min == max {
        let pad = (min.abs() / 20).max(flat_pad);
        (min - pad, max + pad)
    } else {
        let pad = ((max - min) / 20).max(1);
        (min - pad, max + pad)
    }
}

/// The stylesheet class carrying a series' colour. Assigned by position and wrapped, so a
/// source keeps its colour when other series are toggled off.
pub fn series_class(index: usize) -> String {
    format!("series series-{}", index % SERIES_COLOURS)
}

#[component]
pub fn PriceChart(series: Vec<SourceSeries>, currency: String) -> impl IntoView {
    let hidden = RwSignal::new(HashSet::<String>::new());
    let series = StoredValue::new(series);
    let currency = StoredValue::new(currency);

    let legend = move || {
        series
            .get_value()
            .into_iter()
            .enumerate()
            .map(|(index, s)| {
                let id = s.source_id.clone();
                let is_hidden = {
                    let id = id.clone();
                    move || hidden.with(|h| h.contains(&id))
                };
                let toggle = {
                    let id = id.clone();
                    move |_| {
                        hidden.update(|h| {
                            if !h.remove(&id) {
                                h.insert(id.clone());
                            }
                        })
                    }
                };
                view! {
                    <button
                        type="button"
                        class="legend-entry"
                        class:is-hidden=is_hidden
                        on:click=toggle
                    >
                        <span class=format!("legend-swatch series-{}", index % SERIES_COLOURS)></span>
                        <span class="legend-label">{s.label}</span>
                    </button>
                }
            })
            .collect::<Vec<_>>()
    };

    let plot = move || {
        let all = series.get_value();
        let currency = currency.get_value();

        // Keep the original index so colours stay attached to a source across toggles.
        let visible: Vec<(usize, SourceSeries)> = all
            .into_iter()
            .enumerate()
            .filter(|(_, s)| !hidden.with(|h| h.contains(&s.source_id)))
            .filter(|(_, s)| !s.points.is_empty())
            .collect();

        let Some(bounds) = bounds(&visible.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>())
        else {
            return view! {
                <p class="chart-empty">
                    "Nothing to plot yet. Prices appear here once a refresh records one."
                </p>
            }
            .into_any();
        };

        let gridlines = bounds
            .y_ticks()
            .into_iter()
            .map(|value| {
                let (_, y) = bounds.project(bounds.x_min, value);
                view! {
                    <g class="gridline">
                        <line x1=PAD_LEFT y1=y x2=VIEW_W - PAD_RIGHT y2=y />
                        <text x=PAD_LEFT - 8.0 y=y + 4.0 class="axis-label axis-label-y">
                            {format_cents(value, &currency)}
                        </text>
                    </g>
                }
            })
            .collect::<Vec<_>>();

        // First, middle and last instants, which is as much as fits without collisions.
        let x_labels = [
            bounds.x_min,
            (bounds.x_min + bounds.x_max) / 2,
            bounds.x_max,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, at)| {
            let (x, _) = bounds.project(at, bounds.y_min);
            let anchor = match i {
                0 => "start",
                1 => "middle",
                _ => "end",
            };
            view! {
                <text
                    x=x
                    y=VIEW_H - PAD_BOTTOM + 20.0
                    text-anchor=anchor
                    class="axis-label axis-label-x"
                >
                    {format_date(at)}
                </text>
            }
        })
        .collect::<Vec<_>>();

        let lines = visible
            .into_iter()
            .map(|(index, s)| {
                let coords: Vec<(f64, f64)> = s
                    .points
                    .iter()
                    .map(|p| bounds.project(p.fetched_at, p.price_cents))
                    .collect();

                let path = coords
                    .iter()
                    .map(|(x, y)| format!("{x:.2},{y:.2}"))
                    .collect::<Vec<_>>()
                    .join(" ");

                let dots = s
                    .points
                    .iter()
                    .zip(&coords)
                    .map(|(point, (x, y))| {
                        // A native SVG <title> gives a hover tooltip with no JavaScript,
                        // and survives server-side rendering intact.
                        let tip = format!(
                            "{} - {} at {}",
                            format_cents(point.price_cents, &currency),
                            s.label,
                            format_datetime(point.fetched_at),
                        );
                        view! {
                            <circle cx=*x cy=*y r="3.5" class="point">
                                <title>{tip}</title>
                            </circle>
                        }
                    })
                    .collect::<Vec<_>>();

                view! {
                    <g class=series_class(index)>
                        <polyline points=path class="line" />
                        {dots}
                    </g>
                }
            })
            .collect::<Vec<_>>();

        view! {
            <svg
                viewBox=format!("0 0 {VIEW_W} {VIEW_H}")
                class="price-chart"
                role="img"
                preserveAspectRatio="xMidYMid meet"
            >
                {gridlines}
                {x_labels}
                {lines}
            </svg>
        }
        .into_any()
    };

    view! {
        <figure class="chart">
            <div class="chart-legend">{legend}</div>
            {plot}
        </figure>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PricePoint;

    fn series(label: &str, points: &[(i64, i64)]) -> SourceSeries {
        SourceSeries {
            source_id: label.to_string(),
            label: label.to_string(),
            points: points
                .iter()
                .map(|(fetched_at, price_cents)| PricePoint {
                    fetched_at: *fetched_at,
                    price_cents: *price_cents,
                })
                .collect(),
        }
    }

    #[test]
    fn no_data_has_no_bounds() {
        assert_eq!(bounds(&[]), None);
        assert_eq!(bounds(&[series("empty", &[])]), None);
    }

    #[test]
    fn bounds_span_every_series_not_just_the_first() {
        let b = bounds(&[
            series("a", &[(1_000, 10_000), (2_000, 12_000)]),
            series("b", &[(500, 9_000), (3_000, 20_000)]),
        ])
        .unwrap();

        assert!(b.x_min < 500 && b.x_max > 3_000, "x must cover both series");
        assert!(
            b.y_min < 9_000 && b.y_max > 20_000,
            "y must cover both series"
        );
    }

    #[test]
    fn a_single_point_gets_a_usable_span_and_sits_centred() {
        let b = bounds(&[series("solo", &[(5_000, 50_000)])]).unwrap();
        assert!(b.x_max > b.x_min, "x span must not be zero");
        assert!(b.y_max > b.y_min, "y span must not be zero");

        let (x, y) = b.project(5_000, 50_000);
        assert!((x - (PAD_LEFT + Bounds::plot_width() / 2.0)).abs() < 0.5);
        assert!((y - (PAD_TOP + Bounds::plot_height() / 2.0)).abs() < 0.5);
    }

    #[test]
    fn an_unchanging_price_draws_a_centred_flat_line() {
        let b = bounds(&[series("flat", &[(1_000, 30_000), (2_000, 30_000)])]).unwrap();
        let (_, y1) = b.project(1_000, 30_000);
        let (_, y2) = b.project(2_000, 30_000);

        assert!(
            (y1 - y2).abs() < f64::EPSILON,
            "a flat price must be a flat line"
        );
        assert!((y1 - (PAD_TOP + Bounds::plot_height() / 2.0)).abs() < 0.5);
    }

    #[test]
    fn projection_stays_inside_the_plot_area() {
        let b = bounds(&[series("a", &[(0, 1_000), (10_000, 90_000)])]).unwrap();
        for (x, y) in [(0, 1_000), (10_000, 90_000), (5_000, 45_000)] {
            let (px, py) = b.project(x, y);
            assert!(
                (PAD_LEFT..=VIEW_W - PAD_RIGHT).contains(&px),
                "x {px} escaped the plot"
            );
            assert!(
                (PAD_TOP..=VIEW_H - PAD_BOTTOM).contains(&py),
                "y {py} escaped the plot"
            );
        }
    }

    #[test]
    fn cheaper_prices_render_lower_down() {
        let b = bounds(&[series("a", &[(0, 1_000), (1, 9_000)])]).unwrap();
        let (_, cheap) = b.project(0, 1_000);
        let (_, dear) = b.project(1, 9_000);
        assert!(
            cheap > dear,
            "SVG y grows downwards, so a lower price sits lower"
        );
    }

    #[test]
    fn positive_prices_never_produce_a_negative_axis() {
        let b = bounds(&[series("a", &[(0, 50), (1, 60)])]).unwrap();
        assert!(b.y_min >= 0, "padding must not push the axis below zero");
    }

    #[test]
    fn y_ticks_span_the_axis_in_order() {
        let b = bounds(&[series("a", &[(0, 10_000), (1, 20_000)])]).unwrap();
        let ticks = b.y_ticks();

        assert_eq!(ticks.len(), Y_TICKS);
        assert_eq!(ticks[0], b.y_min);
        assert_eq!(*ticks.last().unwrap(), b.y_max);
        assert!(ticks.windows(2).all(|w| w[0] < w[1]), "ticks must ascend");
    }

    #[test]
    fn series_colours_wrap_so_extra_sources_still_get_one() {
        assert_eq!(series_class(0), "series series-0");
        assert_eq!(series_class(SERIES_COLOURS), "series series-0");
        assert_eq!(series_class(SERIES_COLOURS + 2), "series series-2");
    }
}
