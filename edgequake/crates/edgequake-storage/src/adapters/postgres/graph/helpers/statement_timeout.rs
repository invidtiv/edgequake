//! Transaction-scoped `statement_timeout` (SPEC-089 / LAW-H2).
//!
//! WHY: `tokio::time::timeout` abandons the Rust future while Postgres keeps
//! executing — zombie pool holders (GH-336). `SET LOCAL` inside a transaction
//! cancels the statement and cannot leak into the pool after COMMIT/ROLLBACK
//! ([PostgreSQL SET](https://www.postgresql.org/docs/16/sql-set.html)).

use sqlx::{Acquire, PgConnection, Postgres, Transaction};

use crate::error::{Result, StorageError};

/// Open transaction with `SET LOCAL statement_timeout` already applied.
pub(in crate::adapters::postgres::graph) struct LocalTimeoutTx<'c> {
    inner: Transaction<'c, Postgres>,
}

impl<'c> LocalTimeoutTx<'c> {
    pub async fn begin(conn: &'c mut PgConnection, timeout_ms: u32) -> Result<Self> {
        let mut inner = conn
            .begin()
            .await
            .map_err(|e| StorageError::Database(format!("statement_timeout begin failed: {e}")))?;
        sqlx::query(&format!("SET LOCAL statement_timeout = '{timeout_ms}ms'"))
            .execute(&mut *inner)
            .await
            .map_err(|e| {
                StorageError::Database(format!("SET LOCAL statement_timeout failed: {e}"))
            })?;
        Ok(Self { inner })
    }

    pub fn as_mut(&mut self) -> &mut Transaction<'c, Postgres> {
        &mut self.inner
    }

    pub async fn commit(self) -> Result<()> {
        self.inner
            .commit()
            .await
            .map_err(|e| StorageError::Database(format!("statement_timeout commit failed: {e}")))
    }

    pub async fn rollback(self) -> Result<()> {
        self.inner
            .rollback()
            .await
            .map_err(|e| StorageError::Database(format!("statement_timeout rollback failed: {e}")))
    }
}

/// Headroom so Postgres cancels before `tokio::time::timeout` abandons the
/// future (LAW-H2 / GH-336). Covers commit/rollback + error mapping.
const GRAPH_QUERY_PG_HEADROOM_MS: u32 = 250;

/// Resolve graph interactive query timeout in milliseconds (native SQL paths).
///
/// Mirrors app budget (`EDGEQUAKE_GRAPH_QUERY_TIMEOUT_SECS`, 15s) minus
/// [`GRAPH_QUERY_PG_HEADROOM_MS`] so the server wins the cancel race.
pub(in crate::adapters::postgres::graph) fn graph_query_statement_timeout_ms() -> u32 {
    let secs = std::env::var("EDGEQUAKE_GRAPH_QUERY_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(15)
        .max(1);
    secs.saturating_mul(1000)
        .saturating_sub(GRAPH_QUERY_PG_HEADROOM_MS)
        .max(1)
}

/// Interactive HTTP read-path PG kill (SPEC-089 Phase 4 / F-336-13).
///
/// Mirrors `EDGEQUAKE_DOCUMENTS_READ_TIMEOUT_MS` (default 2500) minus headroom.
/// Worker wall-clock (≤7200s) is LLM-bound — per-statement kills stay on
/// `LocalTimeoutTx` / AGE session GUCs, not this interactive budget.
pub fn interactive_statement_timeout_ms() -> u32 {
    let ms = std::env::var("EDGEQUAKE_DOCUMENTS_READ_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(2_500)
        .clamp(500, 30_000);
    ms.saturating_sub(GRAPH_QUERY_PG_HEADROOM_MS).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_query_timeout_ms_default_is_under_app_budget() {
        let prev = std::env::var("EDGEQUAKE_GRAPH_QUERY_TIMEOUT_SECS").ok();
        std::env::remove_var("EDGEQUAKE_GRAPH_QUERY_TIMEOUT_SECS");
        assert_eq!(graph_query_statement_timeout_ms(), 14_750);
        assert!(graph_query_statement_timeout_ms() < 15_000);
        if let Some(v) = prev {
            std::env::set_var("EDGEQUAKE_GRAPH_QUERY_TIMEOUT_SECS", v);
        }
    }

    #[test]
    fn interactive_timeout_ms_default_is_under_read_path_budget() {
        let prev = std::env::var("EDGEQUAKE_DOCUMENTS_READ_TIMEOUT_MS").ok();
        std::env::remove_var("EDGEQUAKE_DOCUMENTS_READ_TIMEOUT_MS");
        assert_eq!(interactive_statement_timeout_ms(), 2_250);
        assert!(interactive_statement_timeout_ms() < 2_500);
        if let Some(v) = prev {
            std::env::set_var("EDGEQUAKE_DOCUMENTS_READ_TIMEOUT_MS", v);
        }
    }
}
