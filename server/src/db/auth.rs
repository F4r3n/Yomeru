//! One-time codes and session tokens.

use anyhow::Context;

use super::Db;

// Minimum gap between OTP emails for the same address — anti-spam only. A new
// code is always allowed once the previous one has expired, so a user who
// misses the TTL is never stranded waiting out a long cooldown.
const OTP_RESEND_FLOOR_MS: i64 = 60_000;

// 10-minute TTL for a generated OTP.
const OTP_TTL_MS: i64 = 600_000;

// Wrong guesses allowed before the code is burned. A 6-digit code is only ~20
// bits, so the TTL alone is not a meaningful brute-force barrier: an attacker
// spread across enough source addresses to defeat the per-IP rate limit could
// otherwise keep guessing for the full 10 minutes. Five attempts caps the odds
// of hitting a given code at 5-in-a-million per issued code.
const OTP_MAX_ATTEMPTS: i64 = 5;

/// Hex-encoded SHA-256, used to keep session tokens out of the database in
/// recoverable form. The token itself is 256 bits of CSPRNG output, so a plain
/// digest is enough — there is no low-entropy input here to make brute-forcing
/// a stolen hash worthwhile, and no salt/KDF is needed.
fn token_hash(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// `Ok(true)` = stored, `Ok(false)` = blocked by the anti-spam floor while a
/// still-valid code exists, `Err` = db error.
///
/// SELECT + INSERT are wrapped in a transaction so concurrent OTP requests
/// for the same email can't both pass the resend check.
pub async fn store_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> anyhow::Result<bool> {
    let mut tx = db.begin().await.context("begin store_otp tx")?;
    let prev: Option<(i64, i64)> =
        sqlx::query_as("SELECT last_requested, expires_at FROM otps WHERE email = ?1")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await
            .context("read otp resend state")?;
    if let Some((last, exp)) = prev {
        // Only throttle while the existing code is still usable. An expired
        // code never blocks a resend, so the TTL can't strand the user.
        if now_ms < exp && now_ms - last < OTP_RESEND_FLOOR_MS {
            return Ok(false);
        }
    }
    // A freshly issued code starts with a full attempt budget.
    sqlx::query(
        "INSERT OR REPLACE INTO otps (email, code, expires_at, last_requested, attempts)
         VALUES (?1, ?2, ?3, ?4, 0)",
    )
    .bind(email)
    .bind(code)
    .bind(now_ms + OTP_TTL_MS)
    .bind(now_ms)
    .execute(&mut *tx)
    .await
    .context("insert otp")?;
    tx.commit().await.context("commit store_otp tx")?;
    Ok(true)
}

/// Validates code and expiry, deleting the OTP row on success.
///
/// A wrong guess increments `attempts` and burns the code once it reaches
/// [`OTP_MAX_ATTEMPTS`], so a code cannot be ground down over its full TTL.
/// The comparison is constant-time: a byte-by-byte early exit would leak the
/// correct prefix through response timing, turning 10^6 guesses into ~60.
pub async fn verify_otp(db: &Db, email: &str, code: &str, now_ms: i64) -> anyhow::Result<bool> {
    use subtle::ConstantTimeEq;

    let mut tx = db.begin().await.context("begin verify_otp tx")?;
    let row: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT code, expires_at, attempts FROM otps WHERE email = ?1")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await
            .context("read otp")?;

    let ok = match row {
        Some((stored, exp, attempts)) => {
            let live = now_ms < exp && attempts < OTP_MAX_ATTEMPTS;
            let matches: bool = stored.as_bytes().ct_eq(code.as_bytes()).into();
            if live && matches {
                sqlx::query("DELETE FROM otps WHERE email = ?1")
                    .bind(email)
                    .execute(&mut *tx)
                    .await
                    .context("delete otp on verify")?;
                true
            } else {
                // Burn the code outright once the budget is spent, so a stale
                // row can't be retried after the counter maxes out.
                if attempts + 1 >= OTP_MAX_ATTEMPTS {
                    sqlx::query("DELETE FROM otps WHERE email = ?1")
                        .bind(email)
                        .execute(&mut *tx)
                        .await
                        .context("delete exhausted otp")?;
                } else {
                    sqlx::query("UPDATE otps SET attempts = attempts + 1 WHERE email = ?1")
                        .bind(email)
                        .execute(&mut *tx)
                        .await
                        .context("increment otp attempts")?;
                }
                false
            }
        }
        None => false,
    };

    tx.commit().await.context("commit verify_otp tx")?;
    Ok(ok)
}

/// Stores a session keyed on the token's hash. The raw token is returned to the
/// client and never written down, so a leaked database copy yields no usable
/// bearer tokens.
pub async fn create_session(
    db: &Db,
    token: &str,
    email: &str,
    expires_at: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO sessions (token_hash, email, expires_at) VALUES (?1, ?2, ?3)",
    )
    .bind(token_hash(token))
    .bind(email)
    .bind(expires_at)
    .execute(db)
    .await
    .context("insert session")?;
    Ok(())
}

/// Returns the email associated with a valid (non-expired) session token, or
/// `Ok(None)` if there is no matching live session.
pub async fn validate_session(db: &Db, token: &str, now_ms: i64) -> anyhow::Result<Option<String>> {
    let email: Option<String> =
        sqlx::query_scalar("SELECT email FROM sessions WHERE token_hash = ?1 AND expires_at > ?2")
            .bind(token_hash(token))
            .bind(now_ms)
            .fetch_optional(db)
            .await
            .context("validate session")?;
    Ok(email)
}

/// Deletes expired sessions and OTPs. Both tables are append-mostly — nothing
/// removed them before, so they grew without bound for the life of the server.
pub async fn prune_expired_auth(db: &Db, now_ms: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?1")
        .bind(now_ms)
        .execute(db)
        .await
        .context("prune expired sessions")?;
    sqlx::query("DELETE FROM otps WHERE expires_at <= ?1")
        .bind(now_ms)
        .execute(db)
        .await
        .context("prune expired otps")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::*;

    #[tokio::test]
    async fn otp_store_then_verify_roundtrip() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        // Verify a few seconds later with the same code (handler trims input).
        let ok = verify_otp(&db, ALICE, "012345", now + 5_000).await.unwrap();
        assert!(ok, "fresh code should verify");
    }

    #[tokio::test]
    async fn otp_resend_blocked_while_valid_then_allowed_after_expiry() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "111111", now).await.unwrap());
        // Within the anti-spam floor while the code is still valid: blocked.
        assert!(!store_otp(&db, ALICE, "222222", now + 5_000).await.unwrap());
        // After the floor but code still valid: allowed (rotates the code).
        assert!(
            store_otp(&db, ALICE, "333333", now + OTP_RESEND_FLOOR_MS)
                .await
                .unwrap()
        );
        // Once the latest code has expired, a resend is always allowed even
        // immediately — the user is never stranded by the TTL.
        let expired_at = now + OTP_RESEND_FLOOR_MS + OTP_TTL_MS + 1;
        assert!(store_otp(&db, ALICE, "444444", expired_at).await.unwrap());
        // And the freshly issued code verifies.
        assert!(
            verify_otp(&db, ALICE, "444444", expired_at + 1_000)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn otp_wrong_code_fails() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "999999", now + 5_000).await.unwrap());
    }

    #[tokio::test]
    async fn otp_burns_after_max_failed_attempts() {
        // Without a budget a 6-digit code could be ground down for its whole
        // 10-minute TTL by anyone able to spread requests across enough IPs.
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        for i in 0..OTP_MAX_ATTEMPTS {
            assert!(
                !verify_otp(&db, ALICE, "999999", now + 1_000).await.unwrap(),
                "wrong guess {i} must fail"
            );
        }
        // The code is gone, so even the *right* value no longer works.
        assert!(
            !verify_otp(&db, ALICE, "012345", now + 1_000).await.unwrap(),
            "exhausted code must be burned, not merely rejected"
        );
    }

    #[tokio::test]
    async fn otp_attempts_reset_on_resend() {
        // Burning a code must not leave the user locked out — the next code
        // starts with a fresh budget.
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        for _ in 0..OTP_MAX_ATTEMPTS {
            let _ = verify_otp(&db, ALICE, "999999", now + 1_000).await.unwrap();
        }
        assert!(store_otp(&db, ALICE, "543210", now + 2_000).await.unwrap());
        assert!(verify_otp(&db, ALICE, "543210", now + 3_000).await.unwrap());
    }

    #[tokio::test]
    async fn otp_succeeds_within_attempt_budget() {
        let db = fresh_db().await;
        let now = 1_700_000_000_000_i64;
        assert!(store_otp(&db, ALICE, "012345", now).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "111111", now + 1_000).await.unwrap());
        assert!(!verify_otp(&db, ALICE, "222222", now + 1_000).await.unwrap());
        assert!(verify_otp(&db, ALICE, "012345", now + 1_000).await.unwrap());
    }

    #[tokio::test]
    async fn session_token_is_not_stored_in_the_clear() {
        // A database copy must not yield usable bearer tokens.
        let db = fresh_db().await;
        create_session(&db, "supersecrettoken", ALICE, 2_000)
            .await
            .unwrap();
        let stored: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_ne!(stored[0], "supersecrettoken");
        assert_eq!(stored[0], token_hash("supersecrettoken"));
        // ...and the raw token still validates.
        assert_eq!(
            validate_session(&db, "supersecrettoken", 1_000)
                .await
                .unwrap()
                .as_deref(),
            Some(ALICE)
        );
    }

    #[tokio::test]
    async fn session_rejects_wrong_or_expired_token() {
        let db = fresh_db().await;
        create_session(&db, "good", ALICE, 2_000).await.unwrap();
        assert!(validate_session(&db, "bad", 1_000).await.unwrap().is_none());
        assert!(
            validate_session(&db, "good", 3_000)
                .await
                .unwrap()
                .is_none(),
            "expired session must not validate"
        );
    }

    #[tokio::test]
    async fn prune_removes_expired_sessions_and_otps() {
        let db = fresh_db().await;
        create_session(&db, "live", ALICE, 10_000).await.unwrap();
        create_session(&db, "dead", BOB, 1_000).await.unwrap();
        // Issued at t=0, so this code expires at OTP_TTL_MS.
        store_otp(&db, ALICE, "012345", 0).await.unwrap();

        // Cutoff sits after the dead session but before the OTP's TTL.
        prune_expired_auth(&db, 5_000).await.unwrap();

        let count = |table: &'static str| {
            let db = db.clone();
            async move {
                sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                    .fetch_one(&db)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(count("sessions").await, 1, "expired session should be gone");
        assert!(
            validate_session(&db, "live", 4_000)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(count("otps").await, 1, "still-valid otp must survive");

        // Past the OTP TTL it goes too.
        prune_expired_auth(&db, OTP_TTL_MS + 1).await.unwrap();
        assert_eq!(count("otps").await, 0, "expired otp should be gone");
        assert_eq!(count("sessions").await, 0, "all sessions now expired");
    }
}
