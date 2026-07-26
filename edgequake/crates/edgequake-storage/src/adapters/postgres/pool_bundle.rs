//! SPEC-090 F-090-28 / F-090-31 — role-split PostgreSQL pools.
//!
//! Four in-process pools share one primary `DATABASE_URL`. The **query** pool
//! uses `DATABASE_READ_URL` when set (read replica), otherwise the primary URL.
//!
//! Workload mapping (LAW-P1):
//! - **query** — latency-bound interactive reads
//! - **ingest** — throughput-bound writes (vector/KV/graph/PDF)
//! - **queue** — task claim / lease / list
//! - **admin** — migrate, reconcile, DDL, ANN warmup, inspector

use super::connection::with_session_hygiene;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Role label for metrics and logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolRole {
    Query,
    Ingest,
    Queue,
    Admin,
}

impl PoolRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Ingest => "ingest",
            Self::Queue => "queue",
            Self::Admin => "admin",
        }
    }

    fn size_env(self) -> &'static str {
        match self {
            Self::Query => "EDGEQUAKE_DB_POOL_SIZE_QUERY",
            Self::Ingest => "EDGEQUAKE_DB_POOL_SIZE_INGEST",
            Self::Queue => "EDGEQUAKE_DB_POOL_SIZE_QUEUE",
            Self::Admin => "EDGEQUAKE_DB_POOL_SIZE_ADMIN",
        }
    }

    fn default_size(self) -> u32 {
        match self {
            Self::Query => 16,
            Self::Ingest => 12,
            Self::Queue => 4,
            Self::Admin => 2,
        }
    }

    fn acquire_timeout(self) -> Duration {
        match self {
            Self::Query => Duration::from_secs(5),
            Self::Ingest => Duration::from_secs(10),
            Self::Queue => Duration::from_secs(5),
            Self::Admin => Duration::from_secs(30),
        }
    }
}

/// Four role-isolated connection pools (SPEC-090 F-090-28).
#[derive(Clone)]
pub struct PgPoolBundle {
    pub query: PgPool,
    pub ingest: PgPool,
    pub queue: PgPool,
    pub admin: PgPool,
    pub query_max: u32,
    pub ingest_max: u32,
    pub queue_max: u32,
    pub admin_max: u32,
    /// True when query pool connected via `DATABASE_READ_URL`.
    pub query_uses_read_url: bool,
}

impl PgPoolBundle {
    /// Build all four pools. `primary_url` is `DATABASE_URL`; query may use read URL.
    pub async fn connect(primary_url: &str) -> Result<Self, sqlx::Error> {
        let query_url = std::env::var("DATABASE_READ_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| primary_url.to_string());
        let query_uses_read_url = query_url != primary_url;

        let query_max = role_max_connections(PoolRole::Query);
        let ingest_max = role_max_connections(PoolRole::Ingest);
        let queue_max = role_max_connections(PoolRole::Queue);
        let admin_max = role_max_connections(PoolRole::Admin);

        let query = connect_role(PoolRole::Query, &query_url, query_max).await?;
        let ingest = connect_role(PoolRole::Ingest, primary_url, ingest_max).await?;
        let queue = connect_role(PoolRole::Queue, primary_url, queue_max).await?;
        let admin = connect_role(PoolRole::Admin, primary_url, admin_max).await?;

        tracing::info!(
            query_max,
            ingest_max,
            queue_max,
            admin_max,
            query_uses_read_url,
            "SPEC-090: PgPoolBundle ready (query/ingest/queue/admin)"
        );

        Ok(Self {
            query,
            ingest,
            queue,
            admin,
            query_max,
            ingest_max,
            queue_max,
            admin_max,
            query_uses_read_url,
        })
    }

    /// Backward-compat primary pool (ingest) for callers not yet role-aware.
    pub fn primary(&self) -> &PgPool {
        &self.ingest
    }

    pub fn for_role(&self, role: PoolRole) -> &PgPool {
        match role {
            PoolRole::Query => &self.query,
            PoolRole::Ingest => &self.ingest,
            PoolRole::Queue => &self.queue,
            PoolRole::Admin => &self.admin,
        }
    }

    /// Sum of configured max connections (for PG `max_connections` headroom checks).
    pub fn total_max_connections(&self) -> u32 {
        self.query_max + self.ingest_max + self.queue_max + self.admin_max
    }
}

fn role_max_connections(role: PoolRole) -> u32 {
    std::env::var(role.size_env())
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| role.default_size())
        .clamp(1, 128)
}

async fn connect_role(role: PoolRole, url: &str, max: u32) -> Result<PgPool, sqlx::Error> {
    with_session_hygiene(
        PgPoolOptions::new()
            .max_connections(max)
            .min_connections(1)
            .acquire_timeout(role.acquire_timeout()),
    )
    .connect(url)
    .await
}

/// Env size for a role (tests / sizing helpers).
pub fn pool_role_max_connections(role: PoolRole) -> u32 {
    role_max_connections(role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_sum_under_typical_pg_limit() {
        let sum = PoolRole::Query.default_size()
            + PoolRole::Ingest.default_size()
            + PoolRole::Queue.default_size()
            + PoolRole::Admin.default_size();
        assert_eq!(sum, 34);
        assert!(sum < 100, "leave headroom under common max_connections=100");
    }
}
