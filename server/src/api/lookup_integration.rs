//! HTTP-level tests for the dictionary endpoints, through the real router.
//!
//! The lookups themselves are tested in their crates. What's covered here is
//! what the clients depend on at the HTTP edge: the response shape (results
//! aligned with the request, a miss as `[]` or `null`), the input limits that
//! keep an unauthenticated body from being a free denial of service, the
//! defaults for an omitted `max`, and the one rate limit all these routes share.
//!
//! The dictionaries live in process-wide cells, so every test loads the same
//! small fixture set once ([`ensure_dicts`]).

use std::num::NonZeroU32;
use std::sync::Once;

use axum::http::StatusCode;
use governor::Quota;
use jmdict_types::PartOfSpeech;
use serde_json::{Value, json};

use super::integration::Harness;

static DICTS: Once = Once::new();

/// Headword the examples fixture holds more sentences for than the default `max`.
const EXAMPLE_WORD: &str = "食べる";
const EXAMPLE_COUNT: usize = 7;

/// Entries sharing the `かな` prefix, more than the prefix search's default `max`.
const PREFIX_FILLER: u32 = 40;

fn ensure_dicts() {
    DICTS.call_once(|| {
        use jmdict_core::test_utils::{build_binary, word};
        let mut words = vec![
            word(1, "食べる", "たべる", vec![PartOfSpeech::VerbIchidan], "to eat"),
            word(2, "飲む", "のむ", vec![PartOfSpeech::VerbGodanMu], "to drink"),
            word(3, "橋", "はし", vec![PartOfSpeech::Noun], "bridge"),
            word(4, "箸", "はし", vec![PartOfSpeech::Noun], "chopsticks"),
            word(5, "食べ物", "たべもの", vec![PartOfSpeech::Noun], "food"),
        ];
        for i in 0..PREFIX_FILLER {
            words.push(word(100 + i, "", &format!("かな{i}"), vec![], "filler"));
        }
        jmdict_core::init(&build_binary(&words)).expect("init jmdict");

        use kanjidic_core::test_utils::{build_binary as kanji_bin, entry, parts};
        let with_parts = |mut e: kanjidic_types::KanjiEntry, p: &[char]| {
            e.parts = parts(p);
            e
        };
        let kanji = [
            with_parts(entry('食', 9, &["ショク"], &["た.べる"], &["eat"]), &['人', '良']),
            with_parts(entry('飲', 12, &["イン"], &["の.む"], &["drink"]), &['食', '欠']),
            entry('人', 2, &["ジン"], &["ひと"], &["person"]),
            entry('良', 7, &["リョウ"], &["よ.い"], &["good"]),
            entry('欠', 4, &["ケツ"], &["か.ける"], &["lack"]),
        ];
        kanjidic_core::init_from_bytes(&kanji_bin(&kanji)).expect("init kanjidic");

        use examples_core::test_utils::{build_binary as examples_bin, ex};
        let sentences: Vec<_> = (0..EXAMPLE_COUNT)
            .map(|i| ex(&format!("例文{i}を食べる。"), &format!("Example {i}.")))
            .collect();
        let index = [(EXAMPLE_WORD, (0..EXAMPLE_COUNT).collect())];
        examples_core::init_from_bytes(&examples_bin(&sentences, &index))
            .expect("init examples");
    });
}

async fn harness() -> Harness {
    ensure_dicts();
    Harness::new().await
}

fn sequences(entries: &Value) -> Vec<u64> {
    let mut seqs: Vec<u64> = entries
        .as_array()
        .expect("a list of entries")
        .iter()
        .map(|e| e["sequence"].as_u64().expect("an entry with a sequence"))
        .collect();
    seqs.sort_unstable();
    seqs
}

fn literals(entries: &Value) -> Vec<String> {
    let mut lits: Vec<String> = entries
        .as_array()
        .expect("a list of kanji")
        .iter()
        .map(|e| e["literal"].as_str().expect("a literal").to_string())
        .collect();
    lits.sort_unstable();
    lits
}

/// `n` copies of a kana: `n` chars, three times as many bytes.
fn kana(n: usize) -> String {
    "あ".repeat(n)
}

// ---- response shape --------------------------------------------------------

#[tokio::test]
async fn lookup_answers_each_word_in_request_order() {
    let h = harness().await;
    let (status, body) = h
        .post(
            "/api/lookup",
            None,
            &json!({ "words": ["飲む", "ない", "はし", "食べる"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 4, "one result per word, misses included");
    assert_eq!(sequences(&results[0]), [2]);
    assert_eq!(sequences(&results[1]), Vec::<u64>::new(), "a miss is []");
    assert_eq!(sequences(&results[2]), [3, 4], "every entry for a shared reading");
    assert_eq!(sequences(&results[3]), [1]);
    assert_eq!(results[3][0]["senses"][0]["glosses"][0]["text"], "to eat");
}

#[tokio::test]
async fn lookup_by_sequence_answers_each_number_in_request_order() {
    let h = harness().await;
    let (status, body) = h
        .post(
            "/api/lookup-by-sequence",
            None,
            &json!({ "sequences": [2, 999_999, 1] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["sequence"], 2);
    assert_eq!(results[1], Value::Null, "an unknown sequence is null");
    assert_eq!(results[2]["sequence"], 1);
}

#[tokio::test]
async fn lookup_prefix_finds_the_words_starting_with_the_text() {
    let h = harness().await;
    let (status, body) = h
        .post("/api/lookup-prefix", None, &json!({ "text": "たべ" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(sequences(&body["results"]), [1, 5]);
}

#[tokio::test]
async fn lookup_prefix_puts_the_exact_match_first() {
    let h = harness().await;
    let (_, body) = h
        .post("/api/lookup-prefix", None, &json!({ "text": "食べ物" }))
        .await;
    assert_eq!(body["results"][0]["sequence"], 5);
}

#[tokio::test]
async fn kanji_lists_the_kanji_of_a_word_and_skips_the_kana() {
    let h = harness().await;
    let (status, body) = h
        .post("/api/kanji", None, &json!({ "word": "食べ飲む" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entries = body["entries"].as_array().expect("entries");
    let in_order: Vec<_> = entries.iter().map(|e| e["literal"].clone()).collect();
    assert_eq!(in_order, [json!("食"), json!("飲")], "in the word's order");
    assert_eq!(entries[0]["meanings"], json!(["eat"]));
}

#[tokio::test]
async fn kanji_parts_lists_every_part_used_inside_a_kanji() {
    let h = harness().await;
    let (status, body) = h.post("/api/kanji-parts", None, &json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut elements: Vec<&str> = body["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .map(|p| p["element"].as_str().expect("an element"))
        .collect();
    elements.sort_unstable();
    assert_eq!(elements, ["人", "欠", "良", "食"]);
}

#[tokio::test]
async fn kanji_by_parts_finds_kanji_containing_the_part_at_any_depth() {
    let h = harness().await;
    let (status, body) = h
        .post("/api/kanji-by-parts", None, &json!({ "parts": "良" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // 飲 holds 良 only through 食.
    assert_eq!(literals(&body["kanji"]), ["良", "食", "飲"]);
    assert_eq!(body["total"], 3);
}

#[tokio::test]
async fn kanji_by_parts_caps_the_list_but_still_counts_every_match() {
    let h = harness().await;
    let (_, body) = h
        .post("/api/kanji-by-parts", None, &json!({ "parts": "良", "max": 1 }))
        .await;
    assert_eq!(body["kanji"].as_array().expect("kanji").len(), 1);
    assert_eq!(body["total"], 3);
}

#[tokio::test]
async fn examples_returns_sentences_for_the_word() {
    let h = harness().await;
    let (status, body) = h
        .post("/api/examples", None, &json!({ "word": EXAMPLE_WORD, "max": 2 }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let entries = body["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["english"], "Example 0.");
    assert_eq!(entries[0]["japanese"], "例文0を食べる。");

    let (_, body) = h
        .post("/api/examples", None, &json!({ "word": "ない" }))
        .await;
    assert_eq!(body["entries"], json!([]), "no sentences is []");
}

// ---- defaults for an omitted `max` -------------------------------------------

#[tokio::test]
async fn an_omitted_max_uses_the_endpoint_default() {
    let h = harness().await;

    let (_, body) = h
        .post("/api/lookup-prefix", None, &json!({ "text": "かな" }))
        .await;
    assert_eq!(body["results"].as_array().expect("results").len(), 30);

    let (_, body) = h
        .post("/api/examples", None, &json!({ "word": EXAMPLE_WORD }))
        .await;
    assert_eq!(body["entries"].as_array().expect("entries").len(), 5);
}

// ---- input limits ------------------------------------------------------------

/// Posts `at_limit` then `over_limit` to `path`: the first must succeed, the
/// second must be a 400 whose error names the field and the limit.
async fn assert_limit(h: &Harness, path: &str, at_limit: Value, over_limit: Value, error: &str) {
    let (status, body) = h.post(path, None, &at_limit).await;
    assert_eq!(status, StatusCode::OK, "{path} at the limit: {body}");
    let (status, body) = h.post(path, None, &over_limit).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{path} over the limit");
    assert_eq!(body["error"], error, "{path}");
}

#[tokio::test]
async fn lookup_takes_at_most_1000_words() {
    let h = harness().await;
    assert_limit(
        &h,
        "/api/lookup",
        json!({ "words": vec!["あ"; 1_000] }),
        json!({ "words": vec!["あ"; 1_001] }),
        "words too large: 1001 (max 1000)",
    )
    .await;
}

#[tokio::test]
async fn lookup_by_sequence_takes_at_most_20000_sequences() {
    let h = harness().await;
    assert_limit(
        &h,
        "/api/lookup-by-sequence",
        json!({ "sequences": vec![1; 20_000] }),
        json!({ "sequences": vec![1; 20_001] }),
        "sequences too large: 20001 (max 20000)",
    )
    .await;
}

/// The text limits count chars, not bytes: 256 kana is 768 bytes and passes.
#[tokio::test]
async fn text_inputs_take_at_most_256_chars() {
    let h = harness().await;
    for (path, field) in [
        ("/api/lookup-prefix", "text"),
        ("/api/kanji", "word"),
        ("/api/examples", "word"),
    ] {
        assert_limit(
            &h,
            path,
            json!({ field: kana(256) }),
            json!({ field: kana(257) }),
            &format!("{field} too large: 257 (max 256)"),
        )
        .await;
    }
}

#[tokio::test]
async fn kanji_by_parts_takes_at_most_16_parts() {
    let h = harness().await;
    assert_limit(
        &h,
        "/api/kanji-by-parts",
        json!({ "parts": "良".repeat(16) }),
        json!({ "parts": "良".repeat(17) }),
        "parts too large: 17 (max 16)",
    )
    .await;
}

// ---- rate limit --------------------------------------------------------------

#[tokio::test]
async fn the_lookup_routes_share_one_rate_limit() {
    ensure_dicts();
    let one = Quota::per_hour(const { NonZeroU32::new(1).unwrap() });
    let roomy = Quota::per_second(const { NonZeroU32::new(10_000).unwrap() });
    let h = Harness::with_quotas(roomy, roomy, one).await;

    let (status, _) = h
        .post("/api/lookup", None, &json!({ "words": ["食べる"] }))
        .await;
    assert_eq!(status, StatusCode::OK);

    // Every route the quota covers, each after the one request it allows.
    for (path, body) in [
        ("/api/lookup", json!({ "words": ["食べる"] })),
        ("/api/lookup-by-sequence", json!({ "sequences": [1] })),
        ("/api/lookup-prefix", json!({ "text": "たべ" })),
        ("/api/kanji", json!({ "word": "食" })),
        ("/api/kanji-parts", json!({})),
        ("/api/kanji-by-parts", json!({ "parts": "良" })),
        ("/api/examples", json!({ "word": EXAMPLE_WORD })),
    ] {
        let (status, _) = h.post(path, None, &body).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{path}");
    }
}
