//! The Kanji tab of the Review page: one card per kanji from the words under
//! review, shown kanji → meanings + readings. The deck is built by
//! [`crate::kanji_deck`]; scheduling is the same FSRS path words use.

use std::collections::HashMap;

use dioxus::prelude::*;
use kanjidic_types::{KanjiEntry, KanjiPart, part_meanings};

use crate::dict::kanji_for;
use crate::drill_log;
use crate::icons::{Icon, IconName};
use crate::idb::{get_all_cards, get_due_kanji_cards, put_card_logged};
use crate::kanji_deck::{StudiedWord, ensure_kanji_cards};
use crate::settings::load as load_settings;
use crate::srs::{
    DRILL_BATCH, ReviewOutcome, TROUBLE_EXIT_INTERVAL_DAYS, TROUBLE_MIN_FAILS, TroubleWord,
    apply_review, now_ms, pick_drill_batch, rating_from_u8, trouble_kanji,
};
use crate::sync::{schedule_sync, use_reload_on_sync};
use crate::types::{ReviewKind, SrsCard};

use super::review::{next_due_message, shuffle};
use super::review_common::{RateButtons, ShowAnswerButton, rating_label, use_review_shortcuts};

/// One answered kanji card, kept for the end-of-session recap.
#[derive(Clone, PartialEq)]
struct KanjiResult {
    id: String,
    kanji: char,
    entry: Option<KanjiEntry>,
    rating: u8,
}

/// Dictionary entries for `kanji`, keyed by literal. Missing ones are absent.
async fn kanji_entries(kanji: impl Iterator<Item = char>) -> HashMap<char, KanjiEntry> {
    let text: String = kanji.collect();
    if text.is_empty() {
        return HashMap::new();
    }
    kanji_for(&text)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|k| (k.literal, k))
        .collect()
}

#[component]
pub(crate) fn KanjiReview() -> Element {
    let mut due = use_signal(Vec::<SrsCard>::new);
    let mut idx = use_signal(|| 0usize);
    let mut show_back = use_signal(|| false);
    let mut started = use_signal(|| false);
    let mut loaded = use_signal(|| false);
    let mut load_error = use_signal(|| Option::<String>::None);
    // Kanji → the studied words using it, for the back of the card.
    let mut words = use_signal(HashMap::<char, Vec<StudiedWord>>::new);
    let mut next_due = use_signal(|| Option::<String>::None);
    // Kanji cards outlive their words, so the deck can hold cards even when no
    // current word has kanji.
    let mut has_deck = use_signal(|| false);
    let mut trouble = use_signal(Vec::<(TroubleWord, Option<KanjiEntry>)>::new);
    // Drill sessions replay trouble kanji without touching their schedule:
    // ratings feed the recap and nothing else.
    let mut drill = use_signal(|| false);
    // Drill counts from the answer log, which rotate the batch a drill picks.
    let mut drilled = use_signal(HashMap::<u32, u32>::new);
    let mut show_all_trouble = use_signal(|| false);
    let mut results = use_signal(Vec::<KanjiResult>::new);
    // Same guard as word review: the card only advances once its write lands,
    // so a second press in that window must not rate it again.
    let mut rating_in_flight = use_signal(|| false);

    let load_session = move || {
        spawn(async move {
            match ensure_kanji_cards().await {
                Ok(map) => {
                    words.set(map);
                    load_error.set(None);
                }
                Err(e) => {
                    warn!("ensure_kanji_cards failed: {e}");
                    load_error.set(Some(e));
                }
            }
            let settings = load_settings();
            let cards: Vec<SrsCard> = get_due_kanji_cards(now_ms())
                .await
                .unwrap_or_default()
                .into_iter()
                .take(settings.max_session_cards as usize)
                .collect();
            due.set(cards);
            idx.set(0);
            show_back.set(false);
            started.set(false);
            drill.set(false);
            results.set(vec![]);
            let mut all = get_all_cards().await.unwrap_or_default();
            all.retain(SrsCard::is_kanji);
            has_deck.set(!all.is_empty());
            next_due.set(next_due_message(&all, now_ms()));
            let list = trouble_kanji(&all, TROUBLE_MIN_FAILS, TROUBLE_EXIT_INTERVAL_DAYS);
            let mut entries =
                kanji_entries(list.iter().filter_map(|t| t.cards.first()?.kanji_char())).await;
            trouble.set(
                list.into_iter()
                    .map(|t| {
                        let e = t.cards.first().and_then(SrsCard::kanji_char);
                        let e = e.and_then(|k| entries.remove(&k));
                        (t, e)
                    })
                    .collect(),
            );
            drilled.set(drill_log::load().await);
            loaded.set(true);
        });
    };

    // Reload on mount and whenever a sync lands, but not mid-session.
    use_reload_on_sync(move || {
        if !*started.peek() {
            load_session();
        }
    });

    let start_drill = move |_| {
        let list: Vec<TroubleWord> = trouble.read().iter().map(|(t, _)| t.clone()).collect();
        let cards: Vec<SrsCard> = pick_drill_batch(&list, &drilled.read(), DRILL_BATCH)
            .into_iter()
            .flat_map(|t| t.cards.iter().cloned())
            .collect();
        due.set(shuffle(cards));
        idx.set(0);
        show_back.set(false);
        results.set(vec![]);
        drill.set(true);
        started.set(true);
    };

    // Dictionary details for the card on screen, fetched as soon as it shows
    // so the back is ready when revealed.
    let info = use_resource(move || async move {
        let kanji = due.read().get(*idx.read()).and_then(SrsCard::kanji_char)?;
        kanji_for(&kanji.to_string())
            .await
            .ok()?
            .into_iter()
            .find(|k| k.literal == kanji)
    });

    let mut reveal = move || show_back.set(true);

    let rate = move |r: u8| {
        if *rating_in_flight.peek() || !*show_back.peek() {
            return;
        }
        let Some(card) = due.peek().get(*idx.peek()).cloned() else {
            return;
        };
        let Some(kanji) = card.kanji_char() else {
            return;
        };
        let entry = info.peek().clone().flatten().filter(|k| k.literal == kanji);
        results.with_mut(|v| {
            v.push(KanjiResult {
                id: card.id.clone(),
                kanji,
                entry,
                rating: r,
            })
        });
        if *drill.peek() {
            // One card per kanji, so each drill answer is that kanji's one drill.
            drill_log::record_drill(&card, r);
            idx += 1;
            show_back.set(false);
            return;
        }
        rating_in_flight.set(true);
        spawn(async move {
            let settings = load_settings();
            let (ReviewOutcome::Rescheduled(c) | ReviewOutcome::Graduated(c)) =
                apply_review(&card, rating_from_u8(r), now_ms(), &settings);
            let row = drill_log::answer(&card, r, ReviewKind::Review);
            match put_card_logged(&c, &row).await {
                Ok(()) => schedule_sync(),
                Err(e) => warn!("put_card({}) after kanji review failed: {e}", card.id),
            }
            idx += 1;
            show_back.set(false);
            rating_in_flight.set(false);
            if *idx.peek() >= due.peek().len() {
                let mut all = get_all_cards().await.unwrap_or_default();
                all.retain(SrsCard::is_kanji);
                next_due.set(next_due_message(&all, now_ms()));
            }
        });
    };

    use_review_shortcuts(
        move || *started.peek() && *idx.peek() < due.peek().len(),
        show_back,
        reveal,
        rate,
    );

    if !*loaded.read() {
        return rsx! { div { class: "loading", "Building kanji deck…" } };
    }

    let cards = due.read();
    let i = *idx.read();
    let total = cards.len();

    if !*started.read() {
        let label = if total == 1 {
            "1 kanji ready".to_string()
        } else {
            format!("{total} kanji ready")
        };
        let deck_empty = !*has_deck.read();
        return rsx! {
            if let Some(e) = load_error.read().clone() {
                div { class: "card error", "Couldn't update the kanji deck: {e}" }
            }
            if total > 0 {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Play, size: 44 } }
                    div { class: "headline", "{label}" }
                    div { class: "helper", "Kanji from the words you are reviewing." }
                    button { class: "primary", onclick: move |_| started.set(true), "Start Review" }
                }
            } else if deck_empty {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::Sparkles, size: 44 } }
                    div { class: "headline", "No kanji yet" }
                    div { class: "helper",
                        "Kanji cards appear here once you review words written with kanji."
                    }
                }
            } else {
                div { class: "empty-state",
                    div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                    div { class: "headline", "All caught up" }
                    div { class: "helper",
                        if let Some(m) = next_due.read().clone() { "{m}" } else { "No kanji due right now." }
                    }
                }
            }
            if !trouble.read().is_empty() {
                TroubleKanji {
                    rows: trouble
                        .read()
                        .iter()
                        .filter_map(|(t, e)| {
                            let kanji = t.cards.first()?.kanji_char()?;
                            let n = drilled.read().get(&t.sequence).copied().unwrap_or(0);
                            Some(TroubleRow { kanji, entry: e.clone(), fails: t.fails, drilled: n })
                        })
                        .collect::<Vec<_>>(),
                    show_all: *show_all_trouble.read(),
                    on_toggle_all: move |_| show_all_trouble.toggle(),
                    on_drill: start_drill,
                }
            }
        };
    }

    let Some(kanji) = cards.get(i).and_then(SrsCard::kanji_char) else {
        let (forgot, remembered): (Vec<KanjiResult>, Vec<KanjiResult>) =
            results.read().iter().cloned().partition(|r| r.rating == 1);
        let headline = if *drill.read() {
            "Drill complete!"
        } else {
            "Review complete!"
        };
        return rsx! {
            div { class: "empty-state",
                div { class: "glyph", Icon { name: IconName::CircleCheck, size: 44 } }
                div { class: "headline", "{headline}" }
                div { class: "helper",
                    "{remembered.len()} remembered · {forgot.len()} forgot"
                    if let Some(m) = next_due.read().clone() { " · {m}" }
                }
            }
            if !forgot.is_empty() || !remembered.is_empty() {
                div { class: "recap-grid",
                    KanjiRecapList {
                        title: format!("Forgot · {}", forgot.len()),
                        title_class: "section-title error",
                        empty: "Nothing forgotten — clean session.",
                        results: forgot,
                    }
                    KanjiRecapList {
                        title: format!("Remembered · {}", remembered.len()),
                        title_class: "section-title ok",
                        empty: "Nothing remembered this time.",
                        results: remembered,
                    }
                }
            }
            div { style: "text-align: center; margin-top: 16px;",
                button { class: "primary", onclick: move |_| load_session(), "Done" }
            }
        };
    };

    let show_back_v = *show_back.read();
    let in_flight = *rating_in_flight.read();
    let is_drill = *drill.read();
    let used_in = words.read().get(&kanji).cloned().unwrap_or_default();
    // The resource keeps its last value until the new lookup lands, so right
    // after a rating it can still hold the previous kanji's entry.
    let entry = info.read().clone().flatten().filter(|k| k.literal == kanji);

    rsx! {
        div { class: "review-card",
            div { class: "row", style: "justify-content: space-between;",
                span { class: "muted", "Card {i + 1} of {total}" }
                span { class: "row",
                    if show_back_v {
                        if let Some(k) = &entry {
                            span { class: "muted kanji-facts", "{kanji_facts(k)}" }
                        }
                    }
                    if is_drill {
                        span { class: "badge fail", "Drill" }
                    }
                    span { class: "badge", "Kanji" }
                }
            }
            div { class: "face",
                div { class: "word kanji-face", "{kanji}" }
            }
            if show_back_v {
                KanjiBack { info: entry, used_in }
            }
            div { style: "margin-top: 16px;",
                if !show_back_v {
                    ShowAnswerButton { on_reveal: move |_| reveal() }
                } else {
                    RateButtons { disabled: in_flight, on_rate: rate }
                }
            }
        }
    }
}

/// One line of the trouble-kanji list.
#[derive(Clone, PartialEq)]
struct TroubleRow {
    kanji: char,
    entry: Option<KanjiEntry>,
    fails: u32,
    /// Drill sessions this kanji was in, on this device.
    drilled: u32,
}

/// The trouble-kanji card under the start screen: the kanji the user keeps
/// failing, and the button that drills the next batch of them.
#[component]
fn TroubleKanji(
    rows: Vec<TroubleRow>,
    show_all: bool,
    on_toggle_all: EventHandler<()>,
    on_drill: EventHandler<()>,
) -> Element {
    let total = rows.len();
    let drill_label = if total > DRILL_BATCH {
        format!("Drill next {DRILL_BATCH}")
    } else {
        "Drill these".to_string()
    };
    let shown = if show_all {
        total
    } else {
        total.min(DRILL_BATCH)
    };
    rsx! {
        div { class: "card", style: "margin-top: 16px;",
            div { class: "row", style: "justify-content: space-between; margin-bottom: 8px;",
                div { class: "section-title", style: "margin-bottom: 0;", "Trouble kanji · {total}" }
                button { class: "outline", onclick: move |_| on_drill.call(()),
                    Icon { name: IconName::Play, size: 14 }
                    "{drill_label}"
                }
            }
            details { class: "info",
                summary { Icon { name: IconName::Info, size: 13 } "How this works" }
                ul {
                    li { "Kanji you keep failing." }
                    li { "A kanji leaves once a real review schedules it a month or more out." }
                    li { "Drills take {DRILL_BATCH} kanji at a time, least-drilled first." }
                    li { "Drilling never changes a kanji's schedule." }
                }
            }
            div { class: "word-rows",
                for r in rows.into_iter().take(shown) {
                    KanjiRow {
                        key: "{r.kanji}",
                        kanji: r.kanji,
                        entry: r.entry,
                        note: (r.drilled > 0).then(|| format!("drilled {}×", r.drilled)),
                        badge: format!("failed {}×", r.fails),
                        badge_class: "badge fail",
                    }
                }
            }
            if total > DRILL_BATCH {
                div { style: "text-align: center; margin-top: 8px;",
                    button { class: "link",
                        onclick: move |_| on_toggle_all.call(()),
                        if show_all { "Show less" } else { "Show all ({total})" }
                    }
                }
            }
        }
    }
}

#[component]
fn KanjiRecapList(
    title: String,
    title_class: &'static str,
    empty: &'static str,
    results: Vec<KanjiResult>,
) -> Element {
    rsx! {
        div { class: "card",
            div { class: "{title_class}", "{title}" }
            if results.is_empty() {
                div { class: "muted", style: "font-size: 13px;", "{empty}" }
            } else {
                div { class: "word-rows",
                    for r in results {
                        {
                            let badge_class = if r.rating == 1 { "badge fail" } else { "badge" };
                            rsx! {
                                KanjiRow {
                                    key: "{r.id}",
                                    kanji: r.kanji,
                                    entry: r.entry,
                                    badge: rating_label(r.rating).to_string(),
                                    badge_class,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One-line kanji summary in the same shape as a word row: the kanji, its
/// first readings, and its first meanings.
#[component]
fn KanjiRow(
    kanji: char,
    entry: Option<KanjiEntry>,
    /// Small muted text before the badge, e.g. "drilled 2×".
    note: Option<String>,
    badge: String,
    badge_class: &'static str,
) -> Element {
    let (reading, gloss) = match &entry {
        Some(k) => (
            k.on_readings
                .iter()
                .chain(&k.kun_readings)
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join("、"),
            k.meanings
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        ),
        None => (String::new(), String::new()),
    };
    rsx! {
        div { class: "word-row",
            span { class: "w", "{kanji}" }
            if !reading.is_empty() {
                span { class: "r", "{reading}" }
            }
            span { class: "g", title: "{gloss}", "{gloss}" }
            if let Some(n) = note {
                span { class: "n", "{n}" }
            }
            span { class: "{badge_class}", "{badge}" }
        }
    }
}

/// Strokes, grade and JLPT level: secondary, so shown small in the card's
/// corner rather than on the back itself.
fn kanji_facts(k: &KanjiEntry) -> String {
    let mut facts = vec![format!("{} strokes", k.stroke_count)];
    if let Some(g) = k.grade {
        facts.push(format!("grade {g}"));
    }
    if let Some(j) = k.jlpt {
        facts.push(format!("JLPT {j}"));
    }
    facts.join(" · ")
}

/// Back of a kanji card: meanings, readings, and the words
/// under review that use it. Each word folds open to its reading and meaning,
/// closed at first so the card stays about the kanji.
#[component]
fn KanjiBack(info: Option<KanjiEntry>, used_in: Vec<StudiedWord>) -> Element {
    let Some(k) = info else {
        return rsx! { div { class: "muted", style: "text-align: center;", "No dictionary entry for this kanji." } };
    };
    rsx! {
        div { class: "kanji-back",
            div { class: "meanings", "{k.meanings.join(\", \")}" }
            if !k.on_readings.is_empty() {
                div { span { class: "muted", "On: " } "{k.on_readings.join(\"、\")}" }
            }
            if !k.kun_readings.is_empty() {
                div { span { class: "muted", "Kun: " } "{k.kun_readings.join(\"、\")}" }
            }
            if !k.parts.is_empty() {
                KanjiParts { key: "{k.literal}", parts: k.parts.clone() }
            }
            if let Some(m) = &k.mnemonic {
                div { class: "mnemonic", "{m}" }
            }
            if !used_in.is_empty() {
                div { class: "used-in",
                    div { class: "muted", "Your words" }
                    for w in used_in {
                        details { key: "{w.sequence}", class: "used-word",
                            summary { "{w.headword}" }
                            div { class: "used-word-info",
                                if !w.reading.is_empty() {
                                    span { class: "reading", "{w.reading}" }
                                }
                                if !w.gloss.is_empty() {
                                    span { "{w.gloss}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The parts a kanji is built from, one level deep. A part that splits
/// further opens its own parts underneath when tapped.
#[component]
fn KanjiParts(parts: Vec<KanjiPart>) -> Element {
    // Each part's own entry, plus its original's (水 for 氵) as a fallback
    // for the meaning. Keyed by the kanji above, so fetched once per mount.
    let wanted: Vec<char> = parts
        .iter()
        .flat_map(|p| std::iter::once(p.element).chain(p.original))
        .collect();
    let entries = use_resource(move || {
        let wanted = wanted.clone();
        async move { kanji_entries(wanted.into_iter()).await }
    });
    let map = entries.read().clone().unwrap_or_default();
    rsx! {
        div { class: "kanji-parts",
            for (i, p) in parts.into_iter().enumerate() {
                PartChip {
                    // The same part can appear twice (木 in 林).
                    key: "{i}",
                    entry: map.get(&p.element).cloned(),
                    original: p.original.and_then(|o| map.get(&o).cloned()),
                    part: p,
                }
            }
        }
    }
}

#[component]
fn PartChip(part: KanjiPart, entry: Option<KanjiEntry>, original: Option<KanjiEntry>) -> Element {
    let mut open = use_signal(|| false);
    // A variant's own entry can be empty or only say "radical number 9"
    // (亻); its original (人) has the real meaning. A few meanings, not just
    // the first: the link is often in a later one (或 is "some", but also
    // "or", which is what makes 惑 click).
    let meaning = [&entry, &original]
        .into_iter()
        .flatten()
        .map(|e| part_meanings(&e.meanings))
        .find(|m| !m.is_empty())
        .map(|m| m.into_iter().take(3).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let sub = entry.map(|e| e.parts).unwrap_or_default();
    // Radical and sound are marked on the chip's edges (see theme.rs), not
    // spelled out; the glyph's tooltip names them.
    let roles = [(part.radical, "radical"), (part.phonetic, "sound")];
    let role_title = roles
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(" · ");
    let mut class = String::from("part");
    for (on, name) in roles {
        if on {
            class.push(' ');
            class.push_str(name);
        }
    }
    let body = rsx! {
        span { class: "el", title: "{role_title}", "{part.element}" }
        if !meaning.is_empty() {
            span { class: "m", title: "{meaning}", "{meaning}" }
        }
    };
    rsx! {
        div { class: "kanji-part",
            if sub.is_empty() {
                div { class: "{class}", {body} }
            } else {
                button {
                    class: if open() { "{class} open" } else { "{class}" },
                    title: "Show its parts",
                    onclick: move |_| open.toggle(),
                    {body}
                }
                if open() {
                    KanjiParts { parts: sub }
                }
            }
        }
    }
}
