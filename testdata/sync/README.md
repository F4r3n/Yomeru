# Sync wire fixtures

The exact JSON that crosses `/api/sync`, shared by three test suites so the
implementations can't drift apart:

| suite | asserts |
|---|---|
| `server/src/api/integration.rs` (`mod wire`) | the server accepts each request over HTTP and stores the right thing |
| `app/shared/src/sync.rs` | the Rust client serializes `request-app-client.json` exactly, and parses `response.json` |
| `extension/src/background/sync.test.ts` | the extension sends `request-extension-client.json`, and applies `response.json` |

## Why there are two dialects of the same payload

Rust serializes an `f64` as `1757000000123.0`. JavaScript has a single number
type, so `JSON.stringify` writes the same value as `1757000000123`. The server
must accept both, and getting that wrong is not hypothetical: `deleted_at` was
once declared `i64`, which would have rejected every request the Rust client
sent — and because `DeletionEntry` is `#[serde(untagged)]`, the failure would
have surfaced as "did not match any variant" over the whole body rather than as
a field error.

So each fixture is written in its own client's dialect, and the comparisons
differ accordingly: the Rust test compares `serde_json::Value`s exactly (which
distinguishes `0.0` from `0`), while the vitest test compares numerically after
coercing both sides, plus an exact key-set check.

## Files

- `request-app-client.json` — the Dioxus app: cards carrying `updated_ms`,
  `deletions` as `{id, deleted_at}` objects, and a `settings` block.
- `request-extension-client.json` — the browser extension: same payload in
  JavaScript's dialect, and no `settings` (it doesn't sync them).
- `request-legacy-client.json` — a client from before `updated_ms` existed:
  no version field, `deletions` as bare id strings. It must keep syncing.
- `response.json` — what the server sends back. `deletions` are bare ids here
  on purpose: a tombstone the server still lists is a delete that stands, since
  a genuine re-add would already have cleared it.

Changing a fixture means changing a wire contract. Both clients and the server
have to agree, and the server must be deployed before clients that depend on it.
