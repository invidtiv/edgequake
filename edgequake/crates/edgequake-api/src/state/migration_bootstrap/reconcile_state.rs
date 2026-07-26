//! Reconcile apply ledger (SPEC-090 F-090-20).
//!
//! Persists support-migration apply outcomes to `edgequake_reconcile_state`.

use sqlx::PgPool;
use tracing::debug;

/// Record a support reconcile apply in `edgequake_reconcile_state`.
pub async fn record_reconcile_state(
    pool: &PgPool,
    support_version: &str,
    apply_sha384: &str,
    duration_ms: Option<i64>,
    outcome: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO edgequake_reconcile_state
            (support_version, apply_sha384, duration_ms, outcome)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (support_version) DO UPDATE SET
            apply_sha384 = EXCLUDED.apply_sha384,
            applied_at = now(),
            duration_ms = EXCLUDED.duration_ms,
            outcome = EXCLUDED.outcome
        "#,
    )
    .bind(support_version)
    .bind(apply_sha384)
    .bind(duration_ms)
    .bind(outcome)
    .execute(pool)
    .await?;

    debug!(
        support_version,
        apply_sha384, outcome, "Recorded reconcile state"
    );
    Ok(())
}

/// SHA-384 hex digest of support apply SQL bytes (matches checksums.lock format).
pub fn sha384_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha384};
    let digest = Sha384::digest(bytes);
    format!("{digest:x}")
}
