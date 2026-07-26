//! Workspace vector table naming and resolution (SPEC-090 F-090-17).

use super::connection::PostgresPool;
use crate::error::{Result, StorageError};
use crate::traits::WorkspaceVectorConfig;

/// Resolve PostgresConfig namespace: prefer existing full-slug table, else legacy, else new full.
pub async fn resolve_workspace_namespace(
    pool: &PostgresPool,
    config: &WorkspaceVectorConfig,
) -> Result<String> {
    let full_ns = config.namespace_prefix();
    let legacy_ns = config.legacy_namespace_prefix();

    if table_exists(pool, &config.table_name()).await? {
        return Ok(full_ns);
    }
    if table_exists(pool, &config.legacy_table_name()).await? {
        return Ok(legacy_ns);
    }

    // New table — fail-closed if relname already occupied in pg_class.
    ensure_table_name_available(pool, &config.table_name()).await?;
    Ok(full_ns)
}

/// Drop workspace vector table (full slug, then legacy fallback).
pub async fn drop_workspace_vector_tables(
    pool: &PostgresPool,
    config: &WorkspaceVectorConfig,
) -> Result<()> {
    let conn = pool.get().await?;
    for rel in [config.table_name(), config.legacy_table_name()] {
        let qualified = format!("public.{rel}");
        sqlx::query(&format!("DROP TABLE IF EXISTS {qualified}"))
            .execute(&conn)
            .await
            .map_err(|e| {
                StorageError::Database(format!(
                    "Failed to drop workspace vector table {qualified}: {e}"
                ))
            })?;
    }
    Ok(())
}

async fn table_exists(pool: &PostgresPool, relname: &str) -> Result<bool> {
    let conn = pool.get().await?;
    pg_class_table_exists(&conn, relname).await
}

async fn ensure_table_name_available(pool: &PostgresPool, relname: &str) -> Result<()> {
    if pg_class_table_exists(&pool.get().await?, relname).await? {
        return Err(StorageError::Database(format!(
            "Workspace vector table name collision: public.{relname} already exists in pg_class"
        )));
    }
    Ok(())
}

async fn pg_class_table_exists(conn: &sqlx::PgPool, relname: &str) -> Result<bool> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname = 'public'
              AND c.relname = $1
              AND c.relkind IN ('r', 'p')
        )",
    )
    .bind(relname)
    .fetch_one(conn)
    .await
    .map_err(|e| StorageError::Database(format!("pg_class probe for {relname} failed: {e}")))?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn full_slug_replaces_hyphens() {
        let id = Uuid::parse_str("4e32a055-9722-40f9-b03e-ade870b07604").unwrap();
        let cfg = WorkspaceVectorConfig::new(id, 1536);
        assert_eq!(
            cfg.namespace_prefix(),
            "default_ws_4e32a055_9722_40f9_b03e_ade870b07604"
        );
        assert_eq!(cfg.legacy_namespace_prefix(), "default_ws_4e32a055");
    }

    #[test]
    fn table_names_full_and_legacy() {
        let id = Uuid::parse_str("4e32a055-9722-40f9-b03e-ade870b07604").unwrap();
        let cfg = WorkspaceVectorConfig::new(id, 1536);
        assert_eq!(
            cfg.table_name(),
            "eq_default_ws_4e32a055_9722_40f9_b03e_ade870b07604_vectors"
        );
        assert_eq!(cfg.legacy_table_name(), "eq_default_ws_4e32a055_vectors");
    }
}
