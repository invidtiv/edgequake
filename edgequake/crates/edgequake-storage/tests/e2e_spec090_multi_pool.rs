//! SPEC-090 F-090-28 — true multi-pool isolation (query vs ingest).
//!
//! Run:
//!   DATABASE_URL=postgresql://edgequake:edgequake_secret@localhost:5432/edgequake \
//!     cargo test -p edgequake-storage --features postgres --test e2e_spec090_multi_pool -- --nocapture

#![cfg(feature = "postgres")]

use edgequake_storage::{pool_role_max_connections, PgPoolBundle, PoolRole};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn e2e_spec090_multi_pool_sizes_and_isolation() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };

    // Keep test footprints small.
    std::env::set_var("EDGEQUAKE_DB_POOL_SIZE_QUERY", "3");
    std::env::set_var("EDGEQUAKE_DB_POOL_SIZE_INGEST", "2");
    std::env::set_var("EDGEQUAKE_DB_POOL_SIZE_QUEUE", "2");
    std::env::set_var("EDGEQUAKE_DB_POOL_SIZE_ADMIN", "1");

    assert_eq!(pool_role_max_connections(PoolRole::Query), 3);
    assert_eq!(pool_role_max_connections(PoolRole::Ingest), 2);

    let bundle = PgPoolBundle::connect(&url)
        .await
        .expect("PgPoolBundle::connect");
    assert_eq!(bundle.query_max, 3);
    assert_eq!(bundle.ingest_max, 2);
    assert_eq!(bundle.queue_max, 2);
    assert_eq!(bundle.admin_max, 1);
    assert_eq!(bundle.total_max_connections(), 8);

    // Saturate ingest pool with held connections; query pool must still serve SELECT 1.
    let ingest = Arc::new(bundle.ingest.clone());
    let mut holders = Vec::new();
    for _ in 0..bundle.ingest_max {
        let pool = Arc::clone(&ingest);
        holders.push(tokio::spawn(async move {
            let mut conn = pool.acquire().await.expect("ingest acquire");
            // Hold the connection past the query probe window.
            let _ = sqlx::query("SELECT pg_sleep(2)").execute(&mut *conn).await;
        }));
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let query_ok = tokio::time::timeout(Duration::from_secs(3), async {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&bundle.query)
            .await
    })
    .await;
    assert!(
        matches!(query_ok, Ok(Ok(1))),
        "query pool must remain usable while ingest is saturated: {query_ok:?}"
    );

    for h in holders {
        let _ = h.await;
    }

    // Distinct pool objects (size gauges independent).
    assert!(
        bundle.query.size() <= bundle.query_max,
        "query size {} > max {}",
        bundle.query.size(),
        bundle.query_max
    );
    assert!(bundle.ingest.size() <= bundle.ingest_max);

    let admin_one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&bundle.admin)
        .await
        .expect("admin SELECT 1");
    assert_eq!(admin_one, 1);

    // Optional read-URL flag is false when unset.
    if std::env::var("DATABASE_READ_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .is_none()
    {
        assert!(!bundle.query_uses_read_url);
    }
}

#[tokio::test]
async fn e2e_spec090_multi_pool_contract_source() {
    let src =
        std::fs::read_to_string("crates/edgequake-storage/src/adapters/postgres/pool_bundle.rs")
            .or_else(|_| std::fs::read_to_string("src/adapters/postgres/pool_bundle.rs"))
            .expect("pool_bundle.rs");
    assert!(src.contains("DATABASE_READ_URL"));
    assert!(src.contains("EDGEQUAKE_DB_POOL_SIZE_QUERY"));
    assert!(src.contains("struct PgPoolBundle"));

    let api = std::fs::read_to_string("../edgequake-api/src/state/postgres.rs")
        .or_else(|_| std::fs::read_to_string("crates/edgequake-api/src/state/postgres.rs"))
        .expect("postgres.rs");
    assert!(api.contains("PgPoolBundle::connect"));
    assert!(api.contains("pool_bundle.admin"));
    assert!(api.contains("pool_bundle.query"));
    assert!(api.contains("pool_bundle.queue"));
}
