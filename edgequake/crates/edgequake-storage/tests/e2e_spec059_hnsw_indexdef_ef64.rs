//! SPEC-059 Wave 5 — new HNSW indexes use ef_construction=128 (or env).

#![cfg(feature = "postgres")]

#[path = "support/postgres_test_config.rs"]
mod postgres_test_config;

use edgequake_storage::traits::VectorStorage;
use edgequake_storage::{hnsw_ef_construction_from_env, PgVectorStorage};

const DIM: usize = 64;

#[tokio::test]
async fn e2e_spec059_new_hnsw_indexdef_ef_construction_64() {
    let Some(config) = postgres_test_config::require_or_skip_postgres("perf059_ef64") else {
        return;
    };

    let expected = hnsw_ef_construction_from_env();
    assert!(
        expected >= 128 || std::env::var("EDGEQUAKE_HNSW_EF_CONSTRUCTION").is_ok(),
        "default ef_construction must be 128 when unset (got {expected})"
    );

    let storage = PgVectorStorage::with_dimension(config.clone(), DIM);
    storage.initialize().await.expect("vector init");
    // Seed a few rows so index exists.
    let emb: Vec<f32> = (0..DIM).map(|i| i as f32 * 0.01).collect();
    storage
        .upsert(&[(
            "ef64-0".into(),
            emb,
            serde_json::json!({"type": "chunk", "workspace_id": "ws"}),
        )])
        .await
        .expect("upsert");

    let pool = postgres_test_config::contract_pg_pool(&config).await;
    let table = format!("eq_{}_vectors", config.table_prefix());
    let row: (String,) = sqlx::query_as(
        r#"
        SELECT indexdef
        FROM pg_indexes
        WHERE tablename = $1
          AND indexdef ILIKE '%hnsw%'
        LIMIT 1
        "#,
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .expect("hnsw indexdef");

    let needle = format!("ef_construction='{expected}'");
    let needle_alt = format!("ef_construction={expected}");
    assert!(
        row.0.contains(&needle)
            || row.0.contains(&needle_alt)
            || row.0.contains(&format!("ef_construction={expected}")),
        "new HNSW indexdef must include ef_construction={expected}; got {}",
        row.0
    );
    eprintln!(
        "OK SPEC-059: HNSW indexdef ef_construction={expected}: {}",
        row.0
    );
    let _ = storage.clear().await;
}
