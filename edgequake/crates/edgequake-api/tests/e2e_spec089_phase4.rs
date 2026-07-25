//! SPEC-089 Phase 4 — outer envelopes (F-336-13/14) + reprocess amp contract (F-336-12).
//!
//! ```bash
//! export DATABASE_URL="$(cat /tmp/edgequake-db-url)"
//! cargo test -p edgequake-api --features postgres --test e2e_spec089_phase4 -- --nocapture
//! ```

#![cfg(feature = "postgres")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::extract_json;
use common::spec013_postgres;
use serde_json::json;
use serial_test::serial;
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
                    json!({ "name": format!("iss089-p4-tenant-{suffix}") }).to_string(),
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
                        "name": format!("iss089-p4-ws-{suffix}"),
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
async fn iss089_phase4_workspace_stats_and_health_under_pressure() {
    let Some(app) = spec013_postgres::create_postgres_mock_app_or_skip().await else {
        eprintln!("SKIP: no PostgreSQL DATABASE_URL configured");
        return;
    };
    let (tenant_id, workspace_id) = create_tenant_workspace(&app).await;

    let app_stats = app.clone();
    let tid = tenant_id.to_string();
    let wid = workspace_id.to_string();
    let storm = tokio::spawn(async move {
        for _ in 0..12 {
            let _ = app_stats
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!("/api/v1/workspaces/{wid}/stats"))
                        .header("X-Tenant-ID", &tid)
                        .header("X-Workspace-ID", &wid)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await;
        }
    });

    let mut health_ok = false;
    for _ in 0..24 {
        tokio::time::sleep(Duration::from_millis(40)).await;
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
    let _ = storm.await;
    assert!(
        health_ok,
        "LAW-H3 / F-336-14: /health must succeed under workspace-stats storm"
    );

    let start = Instant::now();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/workspaces/{workspace_id}/stats"))
                .header("X-Tenant-ID", tenant_id.to_string())
                .header("X-Workspace-ID", workspace_id.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_success(), "stats status={}", resp.status());
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "workspace stats must finish within app 4s envelope (+slack)"
    );
    eprintln!("OK SPEC-089 Phase4: workspace stats + health under storm");
}
