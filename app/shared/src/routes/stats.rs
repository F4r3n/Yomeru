use dioxus::prelude::*;

use crate::idb::get_all_cards;
use crate::srs::now_ms;
use crate::sync::use_reload_on_sync;
use crate::types::{CardStatus, SrsCard};

/// Time window for the upcoming-reviews forecast chart.
#[derive(Clone, Copy, PartialEq)]
enum ForecastRange {
    Hour,
    Day,
    Month,
}

impl ForecastRange {
    /// Number of bars in the chart.
    fn buckets(self) -> usize {
        match self {
            Self::Hour => 12,  // 12 × 5 min
            Self::Day => 24,   // 24 × 1 hr
            Self::Month => 30, // 30 × 1 day
        }
    }

    /// Width of one bucket, in milliseconds.
    fn step_ms(self) -> f64 {
        match self {
            Self::Hour => 5.0 * 60_000.0,
            Self::Day => 60.0 * 60_000.0,
            Self::Month => 24.0 * 60.0 * 60_000.0,
        }
    }

    /// Show an axis tick label every Nth bar to avoid crowding.
    fn tick_stride(self) -> usize {
        match self {
            Self::Hour => 3,  // every 15 min
            Self::Day => 6,   // every 6 hr
            Self::Month => 5, // every 5 days
        }
    }

    /// Axis label for bucket `i` (offset from now).
    fn tick_label(self, i: usize) -> String {
        if i == 0 {
            return "now".into();
        }
        match self {
            Self::Hour => format!("{}m", i * 5),
            Self::Day => format!("{i}h"),
            Self::Month => format!("{i}d"),
        }
    }

    /// Human-readable span a single bucket covers, for the hover tooltip.
    fn bucket_span(self, i: usize) -> String {
        match self {
            Self::Hour => format!("{}–{} min", i * 5, (i + 1) * 5),
            Self::Day => format!("{i}–{} hr", i + 1),
            Self::Month => format!("day {}", i + 1),
        }
    }

    /// Noun for the summary line ("… due in the next month").
    fn window_label(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

/// Bucket active cards by how soon they fall due. Overdue cards (due in the
/// past) and anything beyond the window are clamped into / excluded from the
/// chart: overdue lands in the first bar, far-future cards are dropped.
fn forecast_counts(cards: &[SrsCard], now: f64, range: ForecastRange) -> Vec<u32> {
    let n = range.buckets();
    let step = range.step_ms();
    let mut counts = vec![0u32; n];
    for c in cards {
        if !matches!(c.status, CardStatus::Active) {
            continue;
        }
        let delta = c.due_ms - now;
        if delta > step * n as f64 {
            continue; // beyond the forecast window
        }
        let idx = if delta <= 0.0 {
            0
        } else {
            ((delta / step) as usize).min(n - 1)
        };
        counts[idx] += 1;
    }
    counts
}

#[component]
pub fn StatsTab() -> Element {
    let mut cards = use_signal(Vec::<SrsCard>::new);
    let mut loaded = use_signal(|| false);
    let mut range = use_signal(|| ForecastRange::Day);

    let load = move || {
        spawn(async move {
            match get_all_cards().await {
                Ok(all) => cards.set(all),
                Err(e) => warn!("get_all_cards in StatsTab failed: {e}"),
            }
            loaded.set(true);
        });
    };

    use_reload_on_sync(move || load());

    let all = cards.read();
    let active = all
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Active))
        .count();
    let staged = all
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Staging))
        .count();
    let now = now_ms();
    let due_now = all
        .iter()
        .filter(|c| matches!(c.status, CardStatus::Active) && c.due_ms <= now)
        .count();

    let r = *range.read();
    let counts = forecast_counts(&all, now, r);
    let max = counts.iter().copied().max().unwrap_or(0).max(1);
    let window_total: u32 = counts.iter().sum();
    let stride = r.tick_stride();

    rsx! {
        div {
            div { class: "page-header",
                div {
                    h2 { "Stats" }
                    div { class: "subtitle", "Your review queue at a glance." }
                }
            }

            div { class: "stat-grid",
                div { class: "stat-card danger",
                    div { class: "stat-value", "{due_now}" }
                    div { class: "stat-label", "Due now" }
                }
                div { class: "stat-card accent",
                    div { class: "stat-value", "{active}" }
                    div { class: "stat-label", "Active cards" }
                }
                div { class: "stat-card warn",
                    div { class: "stat-value", "{staged}" }
                    div { class: "stat-label", "Staged" }
                }
            }

            div { class: "forecast",
                div { class: "forecast-head",
                    div { class: "section-title", "Upcoming reviews" }
                    div { class: "forecast-tabs",
                        RangeButton { active: r == ForecastRange::Hour,  onclick: move |_| range.set(ForecastRange::Hour),  label: "Hour" }
                        RangeButton { active: r == ForecastRange::Day,   onclick: move |_| range.set(ForecastRange::Day),   label: "Day" }
                        RangeButton { active: r == ForecastRange::Month, onclick: move |_| range.set(ForecastRange::Month), label: "Month" }
                    }
                }

                if !*loaded.read() {
                    div { class: "loading", "Loading…" }
                } else if window_total == 0 {
                    div { class: "empty", "No reviews due in the next {r.window_label()}." }
                } else {
                    div { class: "forecast-chart",
                        for (i, c) in counts.iter().enumerate() {
                            div { class: "forecast-bar-wrap",
                                div { class: "forecast-tip",
                                    strong { "{c}" }
                                    " card(s)"
                                    span { class: "tip-span", "{r.bucket_span(i)}" }
                                }
                                div {
                                    class: "forecast-bar",
                                    style: "height: {(*c as f64 / max as f64 * 100.0):.1}%;",
                                }
                            }
                        }
                    }
                    div { class: "forecast-axis",
                        for i in 0..counts.len() {
                            div { class: "tick",
                                if i % stride == 0 { "{r.tick_label(i)}" }
                            }
                        }
                    }
                    div { class: "forecast-total",
                        strong { "{window_total}" }
                        " card(s) due in the next {r.window_label()}"
                    }
                }
            }
        }
    }
}

#[component]
fn RangeButton(active: bool, onclick: EventHandler<MouseEvent>, label: &'static str) -> Element {
    let class = if active { "active" } else { "" };
    rsx! {
        button { class: "{class}", onclick: move |e| onclick.call(e), "{label}" }
    }
}
