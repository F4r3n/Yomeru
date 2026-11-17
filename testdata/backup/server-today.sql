-- A server database in today's schema (cards keyed by `sequence`, ids
-- "<sequence>::<direction>"), for the card key migration's tests. The DDL is
-- what `init_schema` creates today; the rows cover every card kind, both
-- review kinds, a review of a card deleted since, and two accounts.
--
-- Read by `server/src/db/backup_tests.rs`. Keep it in today's shape: it
-- stands for the production database the migration starts from.

CREATE TABLE users (
             id     INTEGER PRIMARY KEY,
             email  TEXT NOT NULL UNIQUE
         );
CREATE TABLE cards (
             email           TEXT NOT NULL,
             id              TEXT NOT NULL,
             sequence        INTEGER NOT NULL,
             direction       TEXT NOT NULL,
             due_ms          REAL NOT NULL,
             stability       REAL NOT NULL,
             difficulty      REAL NOT NULL,
             reps            INTEGER NOT NULL,
             lapses          INTEGER NOT NULL,
             state           TEXT NOT NULL,
             last_review_ms  REAL,
             added_ms        REAL NOT NULL,
             status          TEXT NOT NULL,
             priority        INTEGER NOT NULL DEFAULT 0,
             again_count     INTEGER NOT NULL DEFAULT 0,
             updated_ms      REAL NOT NULL DEFAULT 0,
             rev             INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (email, id)
         );
CREATE TABLE settings (
             email                     TEXT PRIMARY KEY,
             graduation_interval_days  INTEGER NOT NULL,
             interval_scale            REAL NOT NULL,
             max_session_cards         INTEGER NOT NULL,
             request_retention         REAL NOT NULL,
             updated_ms                REAL NOT NULL,
             fsrs_params               TEXT
         );
CREATE TABLE otps (
             email           TEXT PRIMARY KEY,
             code            TEXT NOT NULL,
             expires_at      INTEGER NOT NULL,
             last_requested  INTEGER NOT NULL,
             attempts        INTEGER NOT NULL DEFAULT 0
         );
CREATE TABLE sessions (
             token_hash  TEXT PRIMARY KEY,
             user_id     INTEGER NOT NULL REFERENCES users (id),
             expires_at  INTEGER NOT NULL
         );
CREATE TABLE deletions (
             email       TEXT NOT NULL,
             id          TEXT NOT NULL,
             deleted_at  INTEGER NOT NULL,
             rev         INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (email, id)
         );
CREATE TABLE reviews (
             user_id      INTEGER NOT NULL REFERENCES users (id),
             kind         INTEGER NOT NULL,
             card_id      TEXT NOT NULL,
             reviewed_ms  INTEGER NOT NULL,
             rating       INTEGER NOT NULL,
             rev          INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (user_id, kind, card_id, reviewed_ms)
         ) WITHOUT ROWID;
CREATE TABLE sync_meta (
             id          INTEGER PRIMARY KEY CHECK (id = 1),
             epoch       TEXT NOT NULL,
             rev         INTEGER NOT NULL,
             pruned_rev  INTEGER NOT NULL
         );
CREATE INDEX cards_by_rev ON cards (email, rev);
CREATE INDEX deletions_by_rev ON deletions (email, rev);

INSERT INTO users (id, email) VALUES
    (1, 'alice@example.com'),
    (2, 'bob@example.com');

INSERT INTO sessions (token_hash, user_id, expires_at) VALUES
    ('5f1d0c8a3b', 1, 1900000000000);

INSERT INTO settings VALUES
    ('alice@example.com', 120, 1.0, 50, 0.9, 1763000000000.0, '[0.4,1.2,3.1,15.7]'),
    ('bob@example.com', 90, 1.0, 30, 0.85, 1762000000000.0, NULL);

INSERT INTO cards (email, id, sequence, direction, due_ms, stability, difficulty,
                   reps, lapses, state, last_review_ms, added_ms, status, priority,
                   again_count, updated_ms, rev) VALUES
    ('alice@example.com', '1358280::recognition', 1358280, 'recognition',
        1763640000000.0, 12.5, 4.75, 6, 1, 'review', 1762560000000.0, 1760000000000.0,
        'active', 0, 1, 1762560000000.0, 3),
    ('alice@example.com', '1358280::recall', 1358280, 'recall',
        1763381400000.0, 0.5, 7.25, 3, 0, 'learning', 1763380200000.0, 1760000000000.0,
        'active', 0, 2, 1763380200000.0, 4),
    ('alice@example.com', '1000220::recognition', 1000220, 'recognition',
        1763300000000.0, 0.0, 0.0, 0, 0, 'new', NULL, 1763300000000.0,
        'staging', 2, 0, 1763310000000.0, 5),
    ('alice@example.com', '1000220::recall', 1000220, 'recall',
        1763300000000.0, 0.0, 0.0, 0, 0, 'new', NULL, 1763300000000.0,
        'staging', 2, 0, 1763310000000.0, 5),
    ('alice@example.com', '1206730::recognition', 1206730, 'recognition',
        1780000000000.0, 220.0, 3.5, 11, 0, 'review', 1761000000000.0, 1750000000000.0,
        'graduated', 0, 0, 1761000000000.0, 2),
    ('alice@example.com', '29483::kanji', 29483, 'kanji',
        1763500000000.0, 5.0, 5.5, 4, 0, 'review', 1763000000000.0, 1762000000000.0,
        'active', 0, 0, 1763000000000.0, 6),
    ('alice@example.com', '3444285014::grammar', 3444285014, 'grammar',
        1763370000000.0, 0.0, 0.0, 0, 0, 'new', NULL, 1763370000000.0,
        'staging', 0, 0, 1763370000000.0, 7),
    ('bob@example.com', '1358280::recognition', 1358280, 'recognition',
        1763700000000.0, 3.0, 6.0, 2, 0, 'review', 1763100000000.0, 1763000000000.0,
        'active', 0, 0, 1763100000000.0, 8);

-- Alice deleted 1500000 (both directions); its reviews stay in the log.
INSERT INTO deletions (email, id, deleted_at, rev) VALUES
    ('alice@example.com', '1500000::recognition', 1762900000000, 9),
    ('alice@example.com', '1500000::recall', 1762900000000, 9);

-- kind 0 = review, 1 = drill.
INSERT INTO reviews (user_id, kind, card_id, reviewed_ms, rating, rev) VALUES
    (1, 0, '1358280::recognition', 1761000000000, 3, 1),
    (1, 0, '1358280::recognition', 1762560000000, 1, 3),
    (1, 0, '1358280::recall', 1763380200000, 3, 4),
    (1, 1, '1358280::recall', 1763380300000, 1, 4),
    (1, 0, '1206730::recognition', 1761000000000, 4, 2),
    (1, 0, '29483::kanji', 1763000000000, 3, 6),
    (1, 0, '1500000::recognition', 1762000000000, 3, 1),
    (2, 0, '1358280::recognition', 1763100000000, 3, 8);

INSERT INTO sync_meta (id, epoch, rev, pruned_rev) VALUES (1, '9a3c55e01f2b7d64', 9, 0);
