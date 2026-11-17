//! The server database as it was before the card key change, through a start.
//!
//! `testdata/backup/server-today.sql` holds the schema from before the card
//! key change, with rows of every card kind; these tests pin what the
//! migration (`schema::migrate_card_keys`) does to it. `real_db_migrates_whole`
//! runs the same on a copy of the real database.

use super::schema::init_schema;
use super::Db;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::collections::BTreeMap;
use std::str::FromStr;

const SERVER_TODAY: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../testdata/backup/server-today.sql"));

/// Tables a start must leave row for row as they were.
const TABLES: [&str; 8] = [
    "users",
    "sessions",
    "settings",
    "otps",
    "cards",
    "deletions",
    "reviews",
    "sync_meta",
];

async fn one_conn(opts: SqliteConnectOptions) -> Db {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap()
}

async fn fixture_db() -> Db {
    let pool = one_conn(SqliteConnectOptions::from_str(":memory:").unwrap()).await;
    sqlx::raw_sql(SERVER_TODAY).execute(&pool).await.unwrap();
    pool
}

/// Every row of `table`, each as its columns `quote()`d and joined, sorted:
/// compares whole tables without a struct per table.
async fn rows(pool: &Db, table: &str) -> Vec<String> {
    let cols: Vec<String> =
        sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"))
            .fetch_all(pool)
            .await
            .unwrap();
    let row = cols
        .iter()
        .map(|c| format!("{c} || '=' || quote({c})"))
        .collect::<Vec<_>>()
        .join(" || ' ' || ");
    let mut out: Vec<String> = sqlx::query_scalar(&format!("SELECT {row} FROM {table}"))
        .fetch_all(pool)
        .await
        .unwrap();
    out.sort();
    out
}

async fn snapshot(pool: &Db) -> BTreeMap<&'static str, Vec<String>> {
    let mut out = BTreeMap::new();
    for t in TABLES {
        out.insert(t, rows(pool, t).await);
    }
    out
}

/// The schema as `sqlite_master` and `pragma_table_info` describe it.
async fn schema(pool: &Db) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT m.name || '.' || p.name || ' ' || p.type || ' ' || p.\"notnull\" || ' ' || p.pk
         FROM sqlite_master m JOIN pragma_table_info(m.name) p
         WHERE m.type = 'table' ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// The id an old `<sequence>::<direction>` card id has after the migration;
/// `None` for a grammar card, which it drops. Written out here rather than
/// shared with the migration's SQL, so the two check each other.
fn new_id(old: &str) -> Option<String> {
    let (seq, dir) = old.split_once("::")?;
    let seq: u32 = seq.parse().ok()?;
    match dir {
        "recognition" | "recall" => Some(format!("word:{seq}:{dir}")),
        "kanji" => Some(format!("kanji:{}:meaning", char::from_u32(seq)?)),
        _ => None,
    }
}

/// Every non-key field of each card, by id: what the migration must carry over.
async fn card_fields(pool: &Db) -> BTreeMap<(String, String), String> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT email, id, quote(due_ms) || ' ' || quote(stability) || ' ' || quote(difficulty)
                || ' ' || reps || ' ' || lapses || ' ' || state || ' ' || quote(last_review_ms)
                || ' ' || quote(added_ms) || ' ' || status || ' ' || priority || ' '
                || again_count || ' ' || quote(updated_ms) || ' ' || rev
         FROM cards",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter().map(|(e, id, f)| ((e, id), f)).collect()
}

/// Answers per (user, card id), with their times and ratings.
async fn answers(pool: &Db) -> BTreeMap<(i64, String), Vec<String>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT user_id, card_id, kind || ' ' || reviewed_ms || ' ' || rating || ' ' || rev
         FROM reviews ORDER BY reviewed_ms, kind",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut out: BTreeMap<(i64, String), Vec<String>> = BTreeMap::new();
    for (u, id, a) in rows {
        out.entry((u, id)).or_default().push(a);
    }
    out
}

/// `before`'s rows under their new ids, the ones without one left out.
fn rekeyed<K: Ord + Clone, V: Clone>(before: &BTreeMap<(K, String), V>) -> BTreeMap<(K, String), V> {
    before
        .iter()
        .filter_map(|((k, id), v)| Some(((k.clone(), new_id(id)?), v.clone())))
        .collect()
}

#[tokio::test]
async fn the_migrated_schema_is_a_fresh_one() {
    let fresh = one_conn(SqliteConnectOptions::from_str(":memory:").unwrap()).await;
    init_schema(&fresh).await.unwrap();
    let migrated = fixture_db().await;
    init_schema(&migrated).await.unwrap();
    assert_eq!(schema(&migrated).await, schema(&fresh).await);
}

#[tokio::test]
async fn a_start_rekeys_todays_cards_and_their_answers() {
    let pool = fixture_db().await;
    let before = snapshot(&pool).await;
    let fields = card_fields(&pool).await;
    let history = answers(&pool).await;
    let epoch: String = sqlx::query_scalar("SELECT epoch FROM sync_meta").fetch_one(&pool).await.unwrap();

    init_schema(&pool).await.unwrap();

    // Every card but the grammar one, under its new id, every field kept.
    let after_fields = card_fields(&pool).await;
    assert_eq!(after_fields, rekeyed(&fields));
    assert_eq!(after_fields.len(), 7);
    let keys: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT id, kind, item, direction FROM cards WHERE email = 'alice@example.com' ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let row = |id: &str, kind: &str, item: &str, dir: &str| {
        (id.to_owned(), kind.to_owned(), item.to_owned(), dir.to_owned())
    };
    assert_eq!(
        keys,
        [
            row("kanji:猫:meaning", "kanji", "猫", "meaning"),
            row("word:1000220:recall", "word", "1000220", "recall"),
            row("word:1000220:recognition", "word", "1000220", "recognition"),
            row("word:1206730:recognition", "word", "1206730", "recognition"),
            row("word:1358280:recall", "word", "1358280", "recall"),
            row("word:1358280:recognition", "word", "1358280", "recognition"),
        ]
    );

    // Each card keeps its whole answer history, a deleted card's too.
    let after_history = answers(&pool).await;
    assert_eq!(after_history, rekeyed(&history));
    assert_eq!(after_history.values().map(Vec::len).sum::<usize>(), 8);
    assert!(after_history.contains_key(&(1, "word:1500000:recognition".to_owned())));

    // Tombstones name old ids: gone. A new epoch refuses every old cursor.
    let after = snapshot(&pool).await;
    assert!(after["deletions"].is_empty());
    let new_epoch: String = sqlx::query_scalar("SELECT epoch FROM sync_meta").fetch_one(&pool).await.unwrap();
    assert_ne!(new_epoch, epoch);
    for t in ["users", "sessions", "settings", "otps"] {
        assert_eq!(after[t], before[t], "{t}");
    }

    // Once migrated, a start changes nothing.
    init_schema(&pool).await.unwrap();
    assert_eq!(snapshot(&pool).await, after);
}

#[tokio::test]
async fn answers_without_a_numeric_sequence_are_dropped_not_collided() {
    // From the era cards keyed on the word itself: none in production, but
    // two at one instant would both become `word:0:recall` and abort the
    // migration on the primary key.
    let pool = fixture_db().await;
    sqlx::raw_sql(
        "INSERT INTO reviews (user_id, kind, card_id, reviewed_ms, rating, rev) VALUES
             (1, 0, '猫::recall', 1760000000000, 3, 1),
             (1, 0, '犬::recall', 1760000000000, 3, 1),
             (1, 0, '::recall', 1760000000001, 3, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    init_schema(&pool).await.unwrap();

    assert_eq!(answers(&pool).await.values().map(Vec::len).sum::<usize>(), 8);
}

#[tokio::test]
async fn migrated_cards_read_back_through_the_api() {
    let pool = fixture_db().await;
    init_schema(&pool).await.unwrap();
    let bob = super::cards::get_all_cards(&pool, "bob@example.com").await.unwrap();
    assert_eq!(bob.len(), 1);
    assert_eq!(bob[0].id, "word:1358280:recognition");
    assert_eq!((bob[0].kind.as_str(), bob[0].item.as_str()), ("word", "1358280"));
    assert_eq!(bob[0].reps, 2);
}

/// The migration on a copy of the production database:
/// `YOMERU_DB=path/to/copy.db cargo test -p server real_db -- --ignored --nocapture`.
/// Works on a temporary copy; the file itself is only read.
#[tokio::test]
#[ignore]
async fn real_db_migrates_whole() {
    let src = std::env::var("YOMERU_DB").expect("set YOMERU_DB");
    let tmp = std::env::temp_dir().join(format!("yomeru-real-db-{}.db", std::process::id()));
    std::fs::copy(&src, &tmp).expect("copy YOMERU_DB");
    let pool = one_conn(SqliteConnectOptions::new().filename(&tmp)).await;

    let ok: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&pool).await.unwrap();
    assert_eq!(ok, "ok");
    let before = snapshot(&pool).await;
    let fields = card_fields(&pool).await;
    let history = answers(&pool).await;

    init_schema(&pool).await.unwrap();

    let after_fields = card_fields(&pool).await;
    let after_history = answers(&pool).await;
    let after = snapshot(&pool).await;
    let ok: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&pool).await.unwrap();
    let kinds: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT kind, direction, count(*) FROM cards GROUP BY 1, 2 ORDER BY 1, 2")
            .fetch_all(&pool)
            .await
            .unwrap();
    pool.close().await;
    let _ = std::fs::remove_file(&tmp);

    assert_eq!(ok, "ok");
    let dropped_cards = fields.len() - rekeyed(&fields).len();
    let answers_before: usize = history.values().map(Vec::len).sum();
    let answers_after: usize = after_history.values().map(Vec::len).sum();
    println!(
        "cards: {} → {} ({dropped_cards} grammar dropped); answers: {answers_before} → {answers_after}",
        fields.len(),
        after_fields.len()
    );
    println!("cards by kind: {kinds:?}");
    assert_eq!(after_fields, rekeyed(&fields), "every card, every field, under its new id");
    assert_eq!(after_history, rekeyed(&history), "every answer, under its card's new id");
    assert!(after["deletions"].is_empty());
    for t in ["users", "sessions", "settings", "otps"] {
        assert_eq!(after[t], before[t], "{t}");
    }
}
