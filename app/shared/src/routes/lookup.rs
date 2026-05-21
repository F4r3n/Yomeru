use dioxus::prelude::*;
use gloo_storage::{LocalStorage, Storage};
use jmdict_types::WordEntry;

use crate::components::EntryCard;
use crate::dict;
use crate::idb::{get_cards_by_word, put_cards};
use crate::srs::now_ms;
use crate::types::{CardDirection, SrsCard};

const HISTORY_KEY: &str = "lookup_history";
const HISTORY_MAX: usize = 10;

fn load_history() -> Vec<String> {
    LocalStorage::get::<Vec<String>>(HISTORY_KEY).unwrap_or_default()
}

fn push_history(mut h: Vec<String>, term: &str) -> Vec<String> {
    h.retain(|x| x != term);
    h.insert(0, term.to_owned());
    h.truncate(HISTORY_MAX);
    let _ = LocalStorage::set(HISTORY_KEY, &h);
    h
}

fn is_romaji(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
}

/// Hepburn romaji → hiragana. Mirrors `extension/src/options/romaji.ts`.
fn romaji_to_hiragana(input: &str) -> String {
    let s = input.to_lowercase();
    let bytes: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // Double consonant (except 'n') → small っ
        if c != 'n'
            && c.is_ascii_alphabetic()
            && !matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
            && i + 1 < bytes.len()
            && bytes[i + 1] == c
        {
            out.push('っ');
            i += 1;
            continue;
        }
        // 'n' before consonant / end → ん
        if c == 'n'
            && (i + 1 >= bytes.len()
                || !matches!(bytes[i + 1], 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
        {
            out.push('ん');
            i += 1;
            continue;
        }
        let mut matched = false;
        for len in [3usize, 2, 1] {
            if i + len > bytes.len() {
                continue;
            }
            let chunk: String = bytes[i..i + len].iter().collect();
            if let Some(rep) = lookup_romaji(&chunk) {
                out.push_str(rep);
                i += len;
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn lookup_romaji(c: &str) -> Option<&'static str> {
    Some(match c {
        "kya" => "きゃ", "kyu" => "きゅ", "kyo" => "きょ",
        "sha" => "しゃ", "shu" => "しゅ", "sho" => "しょ", "shi" => "し",
        "cha" => "ちゃ", "chu" => "ちゅ", "cho" => "ちょ", "chi" => "ち", "tsu" => "つ",
        "nya" => "にゃ", "nyu" => "にゅ", "nyo" => "にょ",
        "hya" => "ひゃ", "hyu" => "ひゅ", "hyo" => "ひょ",
        "mya" => "みゃ", "myu" => "みゅ", "myo" => "みょ",
        "rya" => "りゃ", "ryu" => "りゅ", "ryo" => "りょ",
        "gya" => "ぎゃ", "gyu" => "ぎゅ", "gyo" => "ぎょ",
        "ja" => "じゃ", "ju" => "じゅ", "jo" => "じょ", "ji" => "じ",
        "jya" => "じゃ", "jyu" => "じゅ", "jyo" => "じょ",
        "bya" => "びゃ", "byu" => "びゅ", "byo" => "びょ",
        "pya" => "ぴゃ", "pyu" => "ぴゅ", "pyo" => "ぴょ",
        "ka" => "か", "ki" => "き", "ku" => "く", "ke" => "け", "ko" => "こ",
        "ga" => "が", "gi" => "ぎ", "gu" => "ぐ", "ge" => "げ", "go" => "ご",
        "sa" => "さ", "su" => "す", "se" => "せ", "so" => "そ",
        "za" => "ざ", "zu" => "ず", "ze" => "ぜ", "zo" => "ぞ",
        "ta" => "た", "te" => "て", "to" => "と",
        "da" => "だ", "de" => "で", "do" => "ど",
        "na" => "な", "ni" => "に", "nu" => "ぬ", "ne" => "ね", "no" => "の",
        "ha" => "は", "hi" => "ひ", "fu" => "ふ", "he" => "へ", "ho" => "ほ",
        "ba" => "ば", "bi" => "び", "bu" => "ぶ", "be" => "べ", "bo" => "ぼ",
        "pa" => "ぱ", "pi" => "ぴ", "pu" => "ぷ", "pe" => "ぺ", "po" => "ぽ",
        "ma" => "ま", "mi" => "み", "mu" => "む", "me" => "め", "mo" => "も",
        "ya" => "や", "yu" => "ゆ", "yo" => "よ",
        "ra" => "ら", "ri" => "り", "ru" => "る", "re" => "れ", "ro" => "ろ",
        "wa" => "わ", "wo" => "を",
        "a" => "あ", "i" => "い", "u" => "う", "e" => "え", "o" => "お",
        "n" => "ん",
        _ => return None,
    })
}

#[component]
pub fn LookupTab() -> Element {
    let query = use_signal(String::new);
    let mut last_target = use_signal(String::new);
    let mut entries = use_signal(Vec::<WordEntry>::new);
    let mut searching = use_signal(|| false);
    let mut searched = use_signal(|| false);
    let mut load_err = use_signal(|| Option::<String>::None);
    let mut history = use_signal(load_history);

    let run_lookup = move |q: String| {
        let q = q.trim().to_string();
        if q.is_empty() {
            entries.set(Vec::new());
            searched.set(false);
            return;
        }
        let target = if is_romaji(&q) { romaji_to_hiragana(&q) } else { q.clone() };
        last_target.set(target.clone());
        searching.set(true);
        let mut entries = entries.clone();
        let mut searching = searching.clone();
        let mut searched = searched.clone();
        let mut history = history.clone();
        spawn(async move {
            // Exact lookup first, then prefix as a fallback.
            let mut results = dict::lookup(&target).await.unwrap_or_else(|e| {
                load_err.set(Some(e));
                Vec::new()
            });
            if results.is_empty() {
                results = dict::lookup_prefix(&target, 30)
                    .await
                    .unwrap_or_default();
            }
            if !results.is_empty() {
                let next = push_history(history.read().clone(), &target);
                history.set(next);
            }
            entries.set(results);
            searching.set(false);
            searched.set(true);
        });
    };

    let on_input = {
        let mut query = query.clone();
        let mut run_lookup = run_lookup.clone();
        move |evt: Event<FormData>| {
            let v = evt.value();
            query.set(v.clone());
            run_lookup(v);
        }
    };

    let on_history = {
        let mut query = query.clone();
        let mut run_lookup = run_lookup.clone();
        move |term: String| {
            query.set(term.clone());
            run_lookup(term);
        }
    };

    let on_clear_history = move |_| {
        let _ = LocalStorage::delete(HISTORY_KEY);
        history.set(Vec::new());
    };

    let on_add = move |word: String| {
        spawn(async move {
            // staging-only sibling pair
            let existing = get_cards_by_word(&word).await.unwrap_or_default();
            if !existing.is_empty() {
                return;
            }
            let now = now_ms();
            let cards = vec![
                SrsCard::new(&word, CardDirection::Recognition, now),
                SrsCard::new(&word, CardDirection::Recall, now),
            ];
            let _ = put_cards(&cards).await;
        });
    };

    let q_trim = query.read().trim().to_string();
    let target = last_target.read().clone();
    let show_converted = is_romaji(&q_trim) && !target.is_empty() && target != q_trim;

    rsx! {
        div { class: "col",
            if let Some(err) = load_err.read().clone() {
                div { class: "card error", "Lookup error: {err}" }
            }
            input {
                r#type: "search",
                placeholder: "Type a Japanese word…",
                value: "{query}",
                autofocus: true,
                oninput: on_input,
            }
            if show_converted {
                div { class: "ok", style: "font-size: 13px;", "→ {target}" }
            }

            if *searching.read() {
                div { class: "loading", "Searching…" }
            } else if !entries.read().is_empty() {
                for entry in entries.read().iter().cloned() {
                    EntryCard {
                        entry,
                        on_add: Some(EventHandler::new(on_add.clone())),
                    }
                }
            } else if *searched.read() {
                {
                    let label = if target.is_empty() { q_trim.clone() } else { target.clone() };
                    rsx! { div { class: "empty", "No entry found for 「{label}」." } }
                }
            } else if !history.read().is_empty() {
                div { class: "card",
                    div { class: "row", style: "justify-content: space-between;",
                        span { class: "muted", style: "font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em;", "Recent" }
                        button { onclick: on_clear_history, "Clear" }
                    }
                    for term in history.read().iter().cloned() {
                        div { class: "row", style: "margin-top: 6px;",
                            button {
                                style: "background: none; padding: 2px 0; color: var(--text);",
                                onclick: move |_| (on_history.clone())(term.clone()),
                                "{term}"
                            }
                        }
                    }
                }
            }
        }
    }
}
