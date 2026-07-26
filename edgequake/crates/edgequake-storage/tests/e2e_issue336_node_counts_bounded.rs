//! SPEC-089 / GH-336 — node_counts_by_source_prefixes must stay bounded.
//!
//! Run:
//! ```bash
//! export DATABASE_URL="$(cat /tmp/edgequake-db-url)"
//! cargo test -p edgequake-storage --features postgres --test e2e_issue336_node_counts_bounded -- --nocapture
//! ```

#![cfg(feature = "postgres")]

#[path = "support/postgres_test_config.rs"]
mod postgres_test_config;

use edgequake_storage::traits::{
    EdgeListFilter, GraphScanOps, GraphStorage, GraphStorageAnalyticsOps, GraphStorageMutateOps,
    NodeListFilter,
};
use edgequake_storage::{PostgresAGEGraphStorage, SOURCE_PREFIX_DISCOVERY_CALLS};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Serializes tests that observe `SOURCE_PREFIX_DISCOVERY_CALLS` (process-global).
static DISCOVERY_CALLS_LOCK: Mutex<()> = Mutex::const_new(());

fn node_props(id: &str, prefix: &str) -> (String, HashMap<String, serde_json::Value>) {
    let mut props = HashMap::new();
    props.insert("node_id".into(), json!(id));
    props.insert("entity_type".into(), json!("CONCEPT"));
    props.insert(
        "source_ids".into(),
        json!([format!("{prefix}0"), format!("{prefix}1")]),
    );
    props.insert("tenant_id".into(), json!("t-issue336"));
    props.insert("workspace_id".into(), json!("ws-issue336"));
    (id.to_string(), props)
}

#[tokio::test]
async fn issue336_large_prefix_batch_bounded() {
    let Some(config) = postgres_test_config::require_or_skip_postgres("issue336_bound") else {
        return;
    };
    let prev = std::env::var("EDGEQUAKE_NATIVE_GRAPH_WRITES").ok();
    std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", "1");

    let graph = Arc::new(PostgresAGEGraphStorage::new(config.clone()));
    graph.initialize().await.expect("init");

    // Seed a modest real graph, then storm with many synthetic prefixes
    // (most miss → still generate probes, exercising batching + timeout).
    let mut nodes = Vec::new();
    let mut real_prefixes = Vec::new();
    for i in 0..16 {
        let prefix = format!("issue336-real-{i}-chunk-");
        real_prefixes.push(prefix.clone());
        nodes.push(node_props(&format!("ISSUE336_R{i}"), &prefix));
    }
    graph.upsert_nodes_batch(&nodes).await.expect("upsert");

    let mut prefixes = real_prefixes.clone();
    for i in 0..500 {
        prefixes.push(format!("issue336-miss-{i}-chunk-"));
    }

    let start = Instant::now();
    // Tight probe bound (page-like): 4 chunks max → far cheaper than 256.
    let result = graph
        .node_counts_by_source_prefixes_capped(&prefixes, 4)
        .await;
    let elapsed = start.elapsed();

    // Must not hold the connection for minutes (pre-fix GH-336 failure mode).
    assert!(
        elapsed < Duration::from_secs(15),
        "large prefix list must finish or fail soft within 15s, took {elapsed:?}"
    );

    match result {
        Ok(map) => {
            for p in &real_prefixes {
                assert!(
                    map.get(p).copied().unwrap_or(0) >= 1,
                    "seeded prefix {p} should count ≥1"
                );
            }
        }
        Err(e) => {
            // Soft failure under statement_timeout is acceptable (API swallows).
            let msg = e.to_string();
            assert!(
                msg.contains("timeout")
                    || msg.contains("cancel")
                    || msg.contains("failed")
                    || msg.contains("statement"),
                "unexpected error shape: {msg}"
            );
        }
    }

    let _ = graph.clear().await;
    match prev {
        Some(v) => std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", v),
        None => std::env::remove_var("EDGEQUAKE_NATIVE_GRAPH_WRITES"),
    }
}

#[tokio::test]
async fn issue336_small_batch_still_gin() {
    let Some(config) = postgres_test_config::require_or_skip_postgres("issue336_gin") else {
        return;
    };
    let prev = std::env::var("EDGEQUAKE_NATIVE_GRAPH_WRITES").ok();
    std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", "1");

    let graph = PostgresAGEGraphStorage::new(config.clone());
    graph.initialize().await.expect("init");
    let graph_name = graph.graph_name().to_string();

    let prefix = "issue336-gin-doc-chunk-";
    graph
        .upsert_nodes_batch(&[node_props("ISSUE336_GIN", prefix)])
        .await
        .expect("upsert");

    let pool = postgres_test_config::contract_pg_pool(&config).await;
    let sql = format!(
        r#"EXPLAIN (FORMAT TEXT)
           WITH prefixes AS MATERIALIZED (
             SELECT prefix, ord
             FROM unnest($1::text[]) WITH ORDINALITY AS t(prefix, ord)
           ),
           probes AS MATERIALIZED (
             SELECT p.prefix, p.ord, (p.prefix || gs.i::text) AS chunk_id
             FROM prefixes p
             CROSS JOIN generate_series(0, $2::int - 1) AS gs(i)
           )
           SELECT count(DISTINCT v.id)::BIGINT
           FROM probes pr
           JOIN {graph}."Node" v
             ON ((ag_catalog.agtype_to_json(v.properties))::jsonb -> 'source_ids')
                @> to_jsonb(pr.chunk_id)"#,
        graph = graph_name
    );
    let plan_rows: Vec<(String,)> = sqlx::query_as(&sql)
        .bind(vec![prefix.to_string()])
        .bind(4_i32)
        .fetch_all(&pool)
        .await
        .expect("EXPLAIN");
    let plan = plan_rows
        .into_iter()
        .map(|r| r.0)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        plan.to_lowercase().contains("idx_node_source_ids_gin")
            || plan.to_lowercase().contains("bitmap")
            || plan.contains("Index Scan")
            || plan.to_lowercase().contains("gin"),
        "EXPLAIN must use Node source_ids GIN; plan:\n{plan}"
    );
    assert!(
        !plan.contains("_ag_label_vertex"),
        "must not plan against parent _ag_label_vertex; plan:\n{plan}"
    );

    let counts = graph
        .node_counts_by_source_prefixes_capped(&[prefix.to_string()], 4)
        .await
        .expect("count");
    assert_eq!(counts.get(prefix).copied().unwrap_or(0), 1);

    let _ = graph.clear().await;
    match prev {
        Some(v) => std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", v),
        None => std::env::remove_var("EDGEQUAKE_NATIVE_GRAPH_WRITES"),
    }
}

#[tokio::test]
async fn issue336_pool_coexistence_with_stats() {
    let Some(config) = postgres_test_config::require_or_skip_postgres("issue336_pool") else {
        return;
    };
    let prev = std::env::var("EDGEQUAKE_NATIVE_GRAPH_WRITES").ok();
    std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", "1");

    let graph = Arc::new(PostgresAGEGraphStorage::new(config.clone()));
    graph.initialize().await.expect("init");

    let mut prefixes = Vec::new();
    for i in 0..200 {
        prefixes.push(format!("issue336-storm-{i}-chunk-"));
    }

    let g_storm = Arc::clone(&graph);
    let storm_prefixes = prefixes.clone();
    let storm = tokio::spawn(async move {
        // Intentionally large probe budget — statement_timeout must bound hold time.
        g_storm
            .node_counts_by_source_prefixes_capped(&storm_prefixes, 256)
            .await
    });

    // Cheap ping while storm runs — must not starve forever (LAW-H3).
    let mut ping_ok = false;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        match tokio::time::timeout(Duration::from_millis(750), graph.ping()).await {
            Ok(Ok(())) => {
                ping_ok = true;
                break;
            }
            _ => continue,
        }
    }
    assert!(
        ping_ok,
        "graph ping must succeed while bounded count storm runs (pool coexistence)"
    );

    let _ = storm.await;
    let _ = graph.clear().await;
    match prev {
        Some(v) => std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", v),
        None => std::env::remove_var("EDGEQUAKE_NATIVE_GRAPH_WRITES"),
    }
}

#[tokio::test]
async fn issue336_discovery_timeout_keeps_pool_usable() {
    let Some(config) = postgres_test_config::require_or_skip_postgres("issue336_discovery") else {
        return;
    };
    let prev = std::env::var("EDGEQUAKE_NATIVE_GRAPH_WRITES").ok();
    std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", "1");

    let graph = Arc::new(PostgresAGEGraphStorage::new(config.clone()));
    graph.initialize().await.expect("init");

    let mut prefixes = Vec::new();
    for i in 0..64 {
        prefixes.push(format!("issue336-disc-{i}-chunk-"));
    }
    // Seed one hit so the path is exercised.
    graph
        .upsert_nodes_batch(&[node_props("ISSUE336_DISC", &prefixes[0])])
        .await
        .expect("upsert");

    let filter = NodeListFilter::default();
    let start = Instant::now();
    // Hold discovery lock so phase4 call-count assert cannot race this storm.
    let _discovery_guard = DISCOVERY_CALLS_LOCK.lock().await;
    let mut handles = Vec::new();
    for _ in 0..8 {
        let g = Arc::clone(&graph);
        let p = prefixes.clone();
        let f = filter.clone();
        handles.push(tokio::spawn(async move {
            g.find_nodes_by_source_prefixes(&f, &p).await
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    drop(_discovery_guard);
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "discovery storms must finish under statement_timeout budget"
    );

    // Pool must still serve analytics after discovery storm (F-336-08).
    let ping = tokio::time::timeout(Duration::from_millis(750), graph.ping()).await;
    assert!(
        matches!(ping, Ok(Ok(()))),
        "pool must remain usable after discovery timeout path"
    );

    let _ = graph.clear().await;
    match prev {
        Some(v) => std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", v),
        None => std::env::remove_var("EDGEQUAKE_NATIVE_GRAPH_WRITES"),
    }
}

#[test]
fn iss089_source_constants_visible_in_helpers() {
    let src = include_str!("../src/adapters/postgres/graph/helpers/source_lineage_sql.rs");
    assert!(src.contains("SOURCE_PREFIX_BATCH_LIMIT: usize = 32"));
    assert!(src.contains("SOURCE_COUNT_STATEMENT_TIMEOUT_MS: u32 = 300"));
    assert!(src.contains("SOURCE_DISCOVERY_STATEMENT_TIMEOUT_MS: u32 = 2000"));
    assert!(src.contains("WORKSPACE_STATS_STATEMENT_TIMEOUT_MS: u32 = 3_750"));
    assert!(src.contains("CROSS JOIN generate_series"));
    assert!(src.contains("MATERIALIZED"));
    let helper = include_str!("../src/adapters/postgres/statement_timeout.rs");
    assert!(helper.contains("LocalTimeoutTx"));
    assert!(helper.contains("SET LOCAL statement_timeout"));
    assert!(helper.contains("interactive_statement_timeout_ms"));
}

#[tokio::test]
async fn iss089_phase4_single_cascade_discovery_is_one_pair() {
    let Some(config) = postgres_test_config::require_or_skip_postgres("iss089_phase4_disc") else {
        return;
    };
    let prev = std::env::var("EDGEQUAKE_NATIVE_GRAPH_WRITES").ok();
    std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", "1");

    let graph = Arc::new(PostgresAGEGraphStorage::new(config.clone()));
    graph.initialize().await.expect("init");
    let prefix = "iss089-phase4-doc-chunk-";
    graph
        .upsert_nodes_batch(&[node_props("ISSUE089_P4", prefix)])
        .await
        .expect("upsert");

    let prefixes = vec![prefix.to_string(), "iss089-phase4-doc".to_string()];
    let _discovery_guard = DISCOVERY_CALLS_LOCK.lock().await;
    let before = SOURCE_PREFIX_DISCOVERY_CALLS.load(Ordering::SeqCst);
    let _ = graph
        .find_nodes_by_source_prefixes(&NodeListFilter::default(), &prefixes)
        .await
        .expect("nodes");
    let _ = graph
        .find_edges_by_source_prefixes(&EdgeListFilter::default(), &prefixes)
        .await
        .expect("edges");
    let calls = SOURCE_PREFIX_DISCOVERY_CALLS.load(Ordering::SeqCst) - before;
    drop(_discovery_guard);
    assert_eq!(
        calls, 2,
        "F-336-12: one cascade = 1 node discovery + 1 edge discovery, got {calls}"
    );

    // Workspace stats counts must complete under PG kill budget (F-336-14).
    let ws = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000089").unwrap();
    let start = Instant::now();
    let _ = graph.node_count_by_workspace(&ws).await;
    let _ = graph.edge_count_by_workspace(&ws).await;
    let _ = graph.distinct_node_type_count_by_workspace(&ws).await;
    assert!(
        start.elapsed() < Duration::from_secs(12),
        "workspace AGE counts must finish under WORKSPACE_STATS timeout"
    );

    let _ = graph.clear().await;
    match prev {
        Some(v) => std::env::set_var("EDGEQUAKE_NATIVE_GRAPH_WRITES", v),
        None => std::env::remove_var("EDGEQUAKE_NATIVE_GRAPH_WRITES"),
    }
}
