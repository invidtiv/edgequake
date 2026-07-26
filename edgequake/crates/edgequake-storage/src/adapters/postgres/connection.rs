//! PostgreSQL connection pool management.
//!
//! Provides connection pooling with lazy initialization and extension setup.
//!
//! ## Implements
//!
//! - [`FEAT0246`]: Connection pool with lazy initialization
//! - [`FEAT0247`]: Extension auto-setup (pgvector, AGE, pgcrypto)
//!
//! ## Use Cases
//!
//! - [`UC0901`]: System establishes database connection
//!
//! ## Enforces
//!
//! - [`BR0246`]: Connection reuse via pooling
//! - [`BR0247`]: Extension availability validation

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;

use super::config::resolve_pool_max_connections;
use super::config::PostgresConfig;
use crate::error::{Result, StorageError};

/// SPEC-090 F-090-07 / LAW-P4: pin `search_path` on connect and reset session
/// state on release so DDL/reconcile GUCs cannot leak across pooled checkouts.
pub fn with_session_hygiene(options: PgPoolOptions) -> PgPoolOptions {
    options
        // WHY: Force search_path=public on every connection.
        // After migration 001 creates the 'edgequake' schema, PostgreSQL's
        // default search_path "$user",public resolves "$user"="edgequake" to
        // that schema first. Unqualified table references in storage queries
        // (e.g. INSERT INTO documents) could resolve to edgequake views instead
        // of the actual public tables. Pinning search_path to public ensures
        // consistent, correct table resolution on all pool connections.
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET search_path TO public")
                    .execute(conn)
                    .await
                    .map(|_| ())
            })
        })
        .after_release(|conn, _meta| {
            Box::pin(async move {
                // RESET ALL clears session GUCs leaked by DDL/reconcile (statement_timeout,
                // maintenance_work_mem, search_path) without discarding sqlx prepared statements.
                // Must not run inside an open transaction (sqlx releases after end).
                if let Err(e) = sqlx::query("RESET ALL").execute(&mut *conn).await {
                    tracing::warn!(
                        error = %e,
                        "SPEC-090: after_release RESET ALL failed; dropping connection"
                    );
                    return Ok(false);
                }
                if let Err(e) = sqlx::query("SET search_path TO public")
                    .execute(&mut *conn)
                    .await
                {
                    tracing::warn!(
                        error = %e,
                        "SPEC-090: after_release search_path pin failed; dropping connection"
                    );
                    return Ok(false);
                }
                Ok(true)
            })
        })
}

/// PostgreSQL connection pool wrapper.
#[derive(Clone)]
pub struct PostgresPool {
    pool: Arc<RwLock<Option<PgPool>>>,
    config: PostgresConfig,
    /// Set during extension setup — AGE available for graph storage.
    graph_extension_available: Arc<AtomicBool>,
}

impl PostgresPool {
    /// Create a new pool with the given configuration.
    pub fn new(config: PostgresConfig) -> Self {
        Self {
            pool: Arc::new(RwLock::new(None)),
            config,
            graph_extension_available: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Wrap an already-initialized `PgPool` (SPEC-011: shared pool across adapters).
    ///
    /// Skips lazy pool creation and extension setup — caller must have initialized
    /// extensions on the underlying pool already.
    pub fn from_existing(pool: PgPool, config: PostgresConfig) -> Self {
        Self {
            pool: Arc::new(RwLock::new(Some(pool))),
            config,
            graph_extension_available: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether Apache AGE extension was successfully enabled at pool init.
    pub fn graph_extension_available(&self) -> bool {
        self.graph_extension_available.load(Ordering::Relaxed)
    }

    /// Probe `pg_extension` for AGE (used when pool was from_existing without setup).
    pub async fn probe_graph_extension_available(&self) -> Result<bool> {
        let pool = self.get().await?;
        let available: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'age')")
                .fetch_one(&pool)
                .await
                .map_err(|e| StorageError::Database(format!("AGE extension probe failed: {e}")))?;
        self.graph_extension_available
            .store(available, Ordering::Relaxed);
        Ok(available)
    }

    /// Get the configuration.
    pub fn config(&self) -> &PostgresConfig {
        &self.config
    }

    /// Initialize the connection pool.
    pub async fn initialize(&self) -> Result<()> {
        let mut pool_guard = self.pool.write().await;

        if pool_guard.is_some() {
            return Ok(());
        }

        let max_connections = resolve_pool_max_connections(self.config.max_connections);

        let pool = with_session_hygiene(
            PgPoolOptions::new()
                .max_connections(max_connections)
                .min_connections(self.config.min_connections)
                .acquire_timeout(self.config.connect_timeout)
                .idle_timeout(Some(self.config.idle_timeout)),
        )
        .connect(&self.config.connection_url())
        .await
        .map_err(|e| StorageError::Connection(format!("Failed to connect: {}", e)))?;

        // Enable required extensions
        self.setup_extensions(&pool).await?;

        *pool_guard = Some(pool);
        Ok(())
    }

    /// Get a reference to the connection pool.
    pub async fn get(&self) -> Result<PgPool> {
        let pool_guard = self.pool.read().await;
        pool_guard
            .clone()
            .ok_or_else(|| StorageError::Connection("Pool not initialized".to_string()))
    }

    /// Close the connection pool.
    pub async fn close(&self) -> Result<()> {
        let mut pool_guard = self.pool.write().await;
        if let Some(pool) = pool_guard.take() {
            pool.close().await;
        }
        Ok(())
    }

    /// Check if the pool is connected.
    pub async fn is_connected(&self) -> bool {
        let pool_guard = self.pool.read().await;
        pool_guard.is_some()
    }

    /// Set up required PostgreSQL extensions.
    async fn setup_extensions(&self, pool: &PgPool) -> Result<()> {
        // Enable pgvector extension
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(pool)
            .await
            .map_err(|e| {
                StorageError::Database(format!(
                    "Failed to create vector extension: {}. Make sure pgvector is installed.",
                    e
                ))
            })?;

        // Enable Apache AGE extension (fail-closed unless opt-out — SPEC-090 F-090-19)
        let age_ok = match sqlx::query("CREATE EXTENSION IF NOT EXISTS age CASCADE")
            .execute(pool)
            .await
        {
            Ok(_) => {
                // Verify AGE catalog is reachable, then reset search_path on this connection
                // so it is not returned to the pool with ag_catalog-first resolution.
                let mut conn = pool.acquire().await.map_err(|e| {
                    StorageError::Database(format!("Failed to acquire connection for AGE: {}", e))
                })?;
                sqlx::query("SET search_path = ag_catalog, \"$user\", public")
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| {
                        StorageError::Database(format!("Failed to set AGE search_path: {}", e))
                    })?;
                sqlx::query("SET search_path TO public")
                    .execute(&mut *conn)
                    .await
                    .map_err(|e| {
                        StorageError::Database(format!("Failed to reset search_path: {}", e))
                    })?;
                true
            }
            Err(e) => {
                if Self::allow_no_graph_extension() {
                    tracing::warn!(
                        "Apache AGE extension not available: {}. Graph operations will use fallback (EDGEQUAKE_ALLOW_NO_GRAPH=1).",
                        e
                    );
                    false
                } else {
                    return Err(StorageError::Database(format!(
                        "Apache AGE extension required but unavailable: {e}. \
                         Set EDGEQUAKE_ALLOW_NO_GRAPH=1 to start without graph storage."
                    )));
                }
            }
        };
        self.graph_extension_available
            .store(age_ok, Ordering::Relaxed);

        Ok(())
    }

    fn allow_no_graph_extension() -> bool {
        matches!(
            std::env::var("EDGEQUAKE_ALLOW_NO_GRAPH")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on"
        )
    }

    /// Execute a raw query (for testing).
    #[allow(dead_code)]
    pub async fn execute(&self, query: &str) -> Result<()> {
        let pool = self.get().await?;
        sqlx::query(query)
            .execute(&pool)
            .await
            .map_err(|e| StorageError::Database(format!("Query failed: {}", e)))?;
        Ok(())
    }

    /// Check database connectivity.
    pub async fn health_check(&self) -> Result<bool> {
        let pool = self.get().await?;
        let row = sqlx::query("SELECT 1 as health")
            .fetch_one(&pool)
            .await
            .map_err(|e| StorageError::Connection(format!("Health check failed: {}", e)))?;

        let health: i32 = row.get("health");
        Ok(health == 1)
    }
}

impl std::fmt::Debug for PostgresPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresPool")
            .field("config", &self.config)
            .field(
                "connected",
                &self.pool.try_read().map(|g| g.is_some()).unwrap_or(false),
            )
            .finish()
    }
}
