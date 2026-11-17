//! The server database as it is in production today, through a server start.
//!
//! `testdata/backup/server-today.sql` holds today's schema with rows of every
//! card kind; these tests pin what a start does to it, the starting point of
//! the card key migration (`docs/phase-4-grammar-cards.md`, "Card keys").
//! `real_db_survives_a_start` runs the same on a copy of the real database.

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

#[tokio::test]
async fn fixture_is_todays_schema() {
    // A fresh start creates the schema the fixture claims to hold, so the
    // fixture can't drift from the server it stands for.
    let fresh = one_conn(SqliteConnectOptions::from_str(":memory:").unwrap()).await;
    init_schema(&fresh).await.unwrap();
    let fixture = fixture_db().await;
    let columns = "SELECT m.name || '.' || p.name || ' ' || p.type || ' ' || p.\"notnull\" || ' ' || p.pk
                   FROM sqlite_master m JOIN pragma_table_info(m.name) p
                   WHERE m.type = 'table' ORDER BY 1";
    let a: Vec<String> = sqlx::query_scalar(columns).fetch_all(&fresh).await.unwrap();
    let b: Vec<String> = sqlx::query_scalar(columns).fetch_all(&fixture).await.unwrap();
    assert_eq!(a, b);
}

#[tokio::test]
async fn a_start_leaves_todays_database_as_it_is() {
    let pool = fixture_db().await;
    let before = snapshot(&pool).await;
    assert_eq!(before["cards"].len(), 8);
    assert_eq!(before["reviews"].len(), 8);
    assert_eq!(before["deletions"].len(), 2);

    init_schema(&pool).await.unwrap();
    init_schema(&pool).await.unwrap();
    assert_eq!(snapshot(&pool).await, before);
}

#[tokio::test]
async fn todays_cards_read_back_through_the_api() {
    let pool = fixture_db().await;
    init_schema(&pool).await.unwrap();
    let cards = super::cards::get_all_cards(&pool, "alice@example.com").await.unwrap();
    let mut ids: Vec<&str> = cards.iter().map(|c| c.id.as_str()).collect();
    ids.sort();
    assert_eq!(
        ids,
        [
            "1000220::recall",
            "1000220::recognition",
            "1206730::recognition",
            "1358280::recall",
            "1358280::recognition",
            "29483::kanji",
            "3444285014::grammar",
        ]
    );
    let grammar = cards.iter().find(|c| c.direction == "grammar").unwrap();
    assert_eq!(grammar.sequence, 3_444_285_014);
}

/// A copy of the production database through a start:
/// `YOMERU_DB=path/to/copy.db cargo test -p server real_db -- --ignored --nocapture`.
/// Works on a temporary copy; the file itself is only read.
#[tokio::test]
#[ignore]
async fn real_db_survives_a_start() {
    let src = std::env::var("YOMERU_DB").expect("set YOMERU_DB");
    let tmp = std::env::temp_dir().join(format!("yomeru-real-db-{}.db", std::process::id()));
    std::fs::copy(&src, &tmp).expect("copy YOMERU_DB");
    let pool = one_conn(SqliteConnectOptions::new().filename(&tmp)).await;

    let ok: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&pool).await.unwrap();
    assert_eq!(ok, "ok");
    let before = snapshot(&pool).await;
    init_schema(&pool).await.unwrap();
    let after = snapshot(&pool).await;

    let per_user: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT u.email,
                (SELECT count(*) FROM cards c WHERE c.email = u.email),
                (SELECT count(*) FROM reviews r WHERE r.user_id = u.id)
         FROM users u ORDER BY u.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for (i, (_, cards, reviews)) in per_user.iter().enumerate() {
        println!("account {}: {cards} cards, {reviews} reviews", i + 1);
    }
    let directions: Vec<(String, i64)> =
        sqlx::query_as("SELECT direction, count(*) FROM cards GROUP BY direction ORDER BY 1")
            .fetch_all(&pool)
            .await
            .unwrap();
    println!("cards by direction: {directions:?}");

    pool.close().await;
    let _ = std::fs::remove_file(&tmp);
    assert_eq!(after, before, "a start changed the real database");
}
