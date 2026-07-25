//! SPEC-089 Wave 3 — list reconcile is page-scoped (LAW-H1).
//!
//! Seeds ≫ page_size completed docs with entity_count=0, then asserts
//! `GET /documents?page_size=K` finishes quickly and GIN reconcile saw ≤K prefixes.
//!
//! ```bash
//! export DATABASE_URL="$(cat /tmp/edgequake-db-url)"
//! cargo test -p edgequake-api --features postgres --test e2e_spec089_list_page_reconcile -- --nocapture
//! ```

#![cfg(feature = "postgres")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::extract_json;
use common::spec013_postgres;
use edgequake_storage::LAST_SOURCE_PREFIX_COUNT_LEN;
use serde_json::json;
use serial_test::serial;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tower::ServiceExt;
use uuid::Uuid;

async fn create_tenant_workspace(app: &axum::Router) -> (Uuid, Uuid) {
    let suffix = Uuid::new_v4();
    let tenant_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tenants")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "name": format!("iss089-tenant-{suffix}") }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tenant_resp.status(), StatusCode::CREATED);
    let tenant = extract_json(tenant_resp).await;
    let tenant_id = Uuid::parse_str(tenant["id"].as_str().unwrap()).unwrap();

    let ws_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tenants/{tenant_id}/workspaces"))
                .header("content-type", "application/json")
                .header("X-Tenant-ID", tenant_id.to_string())
                .body(Body::from(
                    json!({
                        "name": format!("iss089-ws-{suffix}"),
                        "llm_provider": "mock",
                        "embedding_provider": "mock",
                        "embedding_dimension": 384,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(ws_resp.status().is_success());
    let ws = extract_json(ws_resp).await;
    let workspace_id = Uuid::parse_str(ws["id"].as_str().unwrap()).unwrap();
    (tenant_id, workspace_id)
}

#[tokio::test]
#[serial]
async fn iss089_list_reconcile_prefixes_bounded_to_page() {
    let Some(app) = spec013_postgres::create_postgres_mock_app_or_skip().await else {
        eprintln!("SKIP: no PostgreSQL DATABASE_URL configured");
        return;
    };
    let (tenant_id, workspace_id) = create_tenant_workspace(&app).await;

    let db_url = std::env::var("DATABASE_URL")
        .or_else(|_| std::fs::read_to_string("/tmp/edgequake-db-url").map(|s| s.trim().to_string()))
        .expect("DATABASE_URL");
    let pool = sqlx::PgPool::connect(&db_url).await.expect("pool");

    // Seed 120 completed zero-entity docs — enough to blow up if reconcile is corpus-wide.
    const CORPUS: usize = 120;
    const PAGE: usize = 10;
    for i in 0..CORPUS {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO documents (id, tenant_id, workspace_id, title, content, status, chunk_count, entity_count)
            VALUES ($1, $2, $3, $4, $5, 'completed', 2, 0)
            "#,
        )
        .bind(id)
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(format!("iss089-doc-{i}"))
        .bind(format!("body {i}"))
        .execute(&pool)
        .await
        .expect("seed doc");
    }

    LAST_SOURCE_PREFIX_COUNT_LEN.store(usize::MAX, Ordering::SeqCst);
    let start = Instant::now();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/documents?page=1&page_size={PAGE}"))
                .header("X-Tenant-ID", tenant_id.to_string())
                .header("X-Workspace-ID", workspace_id.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert!(resp.status().is_success(), "list status={}", resp.status());
    let body = extract_json(resp).await;
    let docs = body["documents"].as_array().expect("documents array");
    assert!(
        docs.len() <= PAGE,
        "page must return ≤{PAGE} docs, got {}",
        docs.len()
    );

    let prefixes = LAST_SOURCE_PREFIX_COUNT_LEN.load(Ordering::SeqCst);
    // Reconcile may no-op if skip-under-pressure; then prefix len stays MAX or 0.
    if prefixes != usize::MAX && prefixes > 0 {
        assert!(
            prefixes <= PAGE,
            "LAW-H1: reconcile prefixes={prefixes} must be ≤ page_size={PAGE} (not corpus={CORPUS})"
        );
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "page-scoped list must finish quickly, took {elapsed:?}"
    );

    // Cleanup seeded rows for this workspace.
    let _ = sqlx::query("DELETE FROM documents WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(&pool)
        .await;

    eprintln!(
        "OK SPEC-089: list page_size={PAGE} corpus={CORPUS} prefixes={prefixes} elapsed={elapsed:?}"
    );
}

#[tokio::test]
#[serial]
async fn iss089_health_under_list_pressure() {
    let Some(app) = spec013_postgres::create_postgres_mock_app_or_skip().await else {
        eprintln!("SKIP: no PostgreSQL DATABASE_URL configured");
        return;
    };
    let (tenant_id, workspace_id) = create_tenant_workspace(&app).await;

    let db_url = std::env::var("DATABASE_URL")
        .or_else(|_| std::fs::read_to_string("/tmp/edgequake-db-url").map(|s| s.trim().to_string()))
        .expect("DATABASE_URL");
    let pool = sqlx::PgPool::connect(&db_url).await.expect("pool");
    for i in 0..80 {
        sqlx::query(
            r#"
            INSERT INTO documents (id, tenant_id, workspace_id, title, content, status, chunk_count, entity_count)
            VALUES ($1, $2, $3, $4, $5, 'completed', 3, 0)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(format!("iss089-health-{i}"))
        .bind("body")
        .execute(&pool)
        .await
        .expect("seed");
    }

    let app_list = app.clone();
    let tid = tenant_id.to_string();
    let wid = workspace_id.to_string();
    let list_storm = tokio::spawn(async move {
        for _ in 0..8 {
            let _ = app_list
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/documents?page=1&page_size=20")
                        .header("X-Tenant-ID", &tid)
                        .header("X-Workspace-ID", &wid)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await;
        }
    });

    let mut health_ok = false;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if resp.status().is_success() {
            health_ok = true;
            break;
        }
    }
    let _ = list_storm.await;
    assert!(
        health_ok,
        "LAW-H3: /health must succeed under concurrent list pressure"
    );

    let _ = sqlx::query("DELETE FROM documents WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(&pool)
        .await;
    eprintln!("OK SPEC-089: /health green under list storm");
}
