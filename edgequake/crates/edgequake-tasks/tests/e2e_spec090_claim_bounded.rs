//! SPEC-090 Wave 3: bounded claim_next latency gate.
//!
//! Run:
//!   DATABASE_URL=postgresql://edgequake:edgequake@localhost:5432/edgequake \
//!     cargo test -p edgequake-tasks --features postgres --test e2e_spec090_claim_bounded -- --nocapture
//!
//! Skips cleanly when DATABASE_URL is unset.

#![cfg(feature = "postgres")]

use std::env;
use std::time::{Duration, Instant};

use chrono::Utc;
use edgequake_tasks::postgres::PostgresTaskStorage;
use edgequake_tasks::{Task, TaskStatus, TaskStorage, TaskType};
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

const BACKLOG_SIZE: usize = 5_000;
const CLAIM_SAMPLES: usize = 20;
const P95_BUDGET_MS: u128 = 200;

fn get_database_url() -> Option<String> {
    env::var("DATABASE_URL").ok().or_else(|| {
        let password = env::var("POSTGRES_PASSWORD").ok()?;
        let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
        let db = env::var("POSTGRES_DB").unwrap_or_else(|_| "edgequake".to_string());
        let user = env::var("POSTGRES_USER").unwrap_or_else(|_| "edgequake".to_string());
        Some(format!(
            "postgresql://{}:{}@{}:{}/{}",
            user, password, host, port, db
        ))
    })
}

async fn create_test_pool() -> Option<PgPool> {
    let database_url = get_database_url()?;
    PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .ok()
}

async fn ensure_wave3_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS progress JSONB")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pdf_id TEXT")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_tasks_claim_pending_workspace_created
            ON tasks (workspace_id, created_at ASC)
            WHERE status = 'pending'
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_tasks_workspace_pdf_id
            ON tasks (workspace_id, pdf_id)
            WHERE pdf_id IS NOT NULL
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn schema_ready(pool: &PgPool) -> bool {
    sqlx::query("SELECT progress, pdf_id FROM tasks LIMIT 0")
        .execute(pool)
        .await
        .is_ok()
}

fn percentile(sorted_ms: &[u128], p: f64) -> u128 {
    if sorted_ms.is_empty() {
        return 0;
    }
    let idx = ((sorted_ms.len() as f64 * p).ceil() as usize)
        .saturating_sub(1)
        .min(sorted_ms.len() - 1);
    sorted_ms[idx]
}

async fn ensure_tenant_workspace(
    pool: &PgPool,
    tenant_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    let tenant_slug = format!("spec090_t_{}", &tenant_id.to_string()[..8]);
    sqlx::query(
        r#"
        INSERT INTO tenants (tenant_id, name, slug, is_active, metadata, settings, created_at, updated_at)
        VALUES ($1, $2, $3, TRUE, '{}'::jsonb, '{}'::jsonb, NOW(), NOW())
        ON CONFLICT (tenant_id) DO NOTHING
        "#,
    )
    .bind(tenant_id)
    .bind(format!("spec090 tenant {tenant_id}"))
    .bind(&tenant_slug)
    .execute(pool)
    .await?;

    let ws_slug = format!("spec090_w_{}", &workspace_id.to_string()[..8]);
    sqlx::query(
        r#"
        INSERT INTO workspaces (
            workspace_id, tenant_id, name, slug, is_active, metadata, settings, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, TRUE, '{}'::jsonb, '{}'::jsonb, NOW(), NOW())
        ON CONFLICT (workspace_id) DO NOTHING
        "#,
    )
    .bind(workspace_id)
    .bind(tenant_id)
    .bind(format!("spec090 workspace {workspace_id}"))
    .bind(&ws_slug)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_backlog(
    pool: &PgPool,
    storage: &PostgresTaskStorage,
    tenant_id: Uuid,
    workspace_ids: &[Uuid],
    prefix: &str,
) -> Vec<String> {
    ensure_tenant_workspace(pool, tenant_id, workspace_ids[0])
        .await
        .expect("tenant/workspace");
    for ws in workspace_ids {
        ensure_tenant_workspace(pool, tenant_id, *ws)
            .await
            .expect("workspace");
    }

    let mut track_ids = Vec::with_capacity(BACKLOG_SIZE);
    let now = Utc::now();
    let payload =
        serde_json::json!({"task_data": {"seed": true}, "metadata": null, "progress": null});

    for chunk_start in (0..BACKLOG_SIZE).step_by(250) {
        let chunk_end = (chunk_start + 250).min(BACKLOG_SIZE);
        let mut ids = Vec::with_capacity(chunk_end - chunk_start);
        let mut tenants = Vec::with_capacity(chunk_end - chunk_start);
        let mut workspaces = Vec::with_capacity(chunk_end - chunk_start);
        let mut types = Vec::with_capacity(chunk_end - chunk_start);
        let mut statuses = Vec::with_capacity(chunk_end - chunk_start);
        let mut created = Vec::with_capacity(chunk_end - chunk_start);

        for i in chunk_start..chunk_end {
            let ws = workspace_ids[i % workspace_ids.len()];
            let track_id = format!("{prefix}-upload-{i:05}");
            ids.push(track_id.clone());
            tenants.push(tenant_id);
            workspaces.push(ws);
            types.push("upload".to_string());
            statuses.push("pending".to_string());
            created.push(now);
            track_ids.push(track_id);
        }

        sqlx::query(
            r#"
            INSERT INTO tasks (
                track_id, tenant_id, workspace_id, task_type, status,
                created_at, updated_at, payload,
                retry_count, max_retries, consecutive_timeout_failures, circuit_breaker_tripped
            )
            SELECT
                u.track_id,
                u.tenant_id,
                u.workspace_id,
                u.task_type,
                u.status,
                u.created_at,
                u.created_at,
                $7::jsonb,
                0,
                3,
                0,
                FALSE
            FROM UNNEST(
                $1::text[],
                $2::uuid[],
                $3::uuid[],
                $4::text[],
                $5::text[],
                $6::timestamptz[]
            ) AS u(track_id, tenant_id, workspace_id, task_type, status, created_at)
            "#,
        )
        .bind(&ids)
        .bind(&tenants)
        .bind(&workspaces)
        .bind(&types)
        .bind(&statuses)
        .bind(&created)
        .bind(&payload)
        .execute(pool)
        .await
        .expect("bulk seed");
    }

    let _ = storage;
    track_ids
}

async fn cleanup_tracks(pool: &PgPool, track_ids: &[String]) {
    for chunk in track_ids.chunks(500) {
        let _ = sqlx::query("DELETE FROM tasks WHERE track_id = ANY($1::text[])")
            .bind(chunk)
            .execute(pool)
            .await;
    }
}

#[tokio::test]
async fn e2e_spec090_claim_latency_bounded_with_backlog() {
    let Some(pool) = create_test_pool().await else {
        eprintln!("Skipping e2e_spec090_claim_bounded: DATABASE_URL not set");
        return;
    };
    if let Err(e) = ensure_wave3_schema(&pool).await {
        eprintln!("Skipping e2e_spec090_claim_bounded: schema prep failed: {e}");
        return;
    }
    if !schema_ready(&pool).await {
        eprintln!("Skipping: tasks.progress/pdf_id columns missing (run migrations 099-101)");
        return;
    }

    let storage = PostgresTaskStorage::new(pool.clone());
    let tenant_id = Uuid::new_v4();
    let workspace_ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
    let prefix = format!("spec090-{}", Uuid::new_v4());
    let seeded = seed_backlog(&pool, &storage, tenant_id, &workspace_ids, &prefix).await;

    let mut latencies_ms = Vec::with_capacity(CLAIM_SAMPLES);
    for n in 0..CLAIM_SAMPLES {
        let worker = format!("spec090-worker-{n}");
        let start = Instant::now();
        let claimed = storage
            .claim_next(&worker, Duration::from_secs(30))
            .await
            .expect("claim");
        latencies_ms.push(start.elapsed().as_millis());
        assert!(
            claimed.is_some(),
            "expected claim {n} to succeed with backlog"
        );
        if let Some(mut task) = claimed {
            task.status = TaskStatus::Indexed;
            task.completed_at = Some(Utc::now());
            storage.update_task(&task).await.expect("complete");
        }
    }

    latencies_ms.sort_unstable();
    let p95 = percentile(&latencies_ms, 0.95);
    eprintln!(
        "spec090 claim latencies ms (n={}): min={} p50={} p95={} max={}",
        latencies_ms.len(),
        latencies_ms.first().copied().unwrap_or(0),
        percentile(&latencies_ms, 0.50),
        p95,
        latencies_ms.last().copied().unwrap_or(0),
    );

    cleanup_tracks(&pool, &seeded).await;

    assert!(
        p95 < P95_BUDGET_MS,
        "claim_next p95 {p95}ms exceeds {P95_BUDGET_MS}ms budget with {BACKLOG_SIZE} pending tasks"
    );
}

#[tokio::test]
async fn e2e_spec090_prune_terminal_tasks() {
    let Some(pool) = create_test_pool().await else {
        eprintln!("Skipping e2e_spec090_prune: DATABASE_URL not set");
        return;
    };
    if let Err(e) = ensure_wave3_schema(&pool).await {
        eprintln!("Skipping e2e_spec090_prune: schema prep failed: {e}");
        return;
    }
    if !schema_ready(&pool).await {
        eprintln!("Skipping: tasks.progress/pdf_id columns missing (run migrations 099-101)");
        return;
    }

    let storage = PostgresTaskStorage::new(pool.clone());
    let tenant_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let prefix = format!("spec090-prune-{}", Uuid::new_v4());

    ensure_tenant_workspace(&pool, tenant_id, workspace_id)
        .await
        .expect("tenant/workspace");

    let mut old = Task::new(
        tenant_id,
        workspace_id,
        TaskType::Upload,
        serde_json::json!({"prune": "old"}),
    );
    old.track_id = format!("{prefix}-old");
    old.status = TaskStatus::Indexed;
    old.completed_at = Some(Utc::now() - chrono::Duration::days(45));
    storage.create_task(&old).await.expect("old task");

    let mut fresh = Task::new(
        tenant_id,
        workspace_id,
        TaskType::Upload,
        serde_json::json!({"prune": "fresh"}),
    );
    fresh.track_id = format!("{prefix}-fresh");
    fresh.status = TaskStatus::Indexed;
    fresh.completed_at = Some(Utc::now() - chrono::Duration::days(1));
    storage.create_task(&fresh).await.expect("fresh task");

    let deleted = storage.prune_terminal_tasks(30).await.expect("prune");
    assert_eq!(
        deleted, 1,
        "only the 45-day-old terminal row should be pruned"
    );

    assert!(
        storage.get_task(&old.track_id).await.unwrap().is_none(),
        "old task should be deleted"
    );
    assert!(
        storage.get_task(&fresh.track_id).await.unwrap().is_some(),
        "fresh task should remain"
    );

    let _ = sqlx::query("DELETE FROM tasks WHERE track_id LIKE $1")
        .bind(format!("{prefix}-%"))
        .execute(&pool)
        .await;
}

#[tokio::test]
async fn e2e_spec090_verify_progress_column() {
    let Some(pool) = create_test_pool().await else {
        eprintln!("Skipping progress verify: DATABASE_URL not set");
        return;
    };
    if ensure_wave3_schema(&pool).await.is_err() || !schema_ready(&pool).await {
        eprintln!("Skipping progress verify: schema not ready");
        return;
    }

    let storage = PostgresTaskStorage::new(pool.clone());
    let tenant_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    ensure_tenant_workspace(&pool, tenant_id, workspace_id)
        .await
        .expect("tenant/workspace");

    let mut task = Task::new(
        tenant_id,
        workspace_id,
        TaskType::Upload,
        serde_json::json!({"payload_stable": "x".repeat(2048)}),
    );
    task.track_id = format!("spec090-prog-{}", Uuid::new_v4());
    storage.create_task(&task).await.expect("create");

    let payload_before: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM tasks WHERE track_id = $1")
            .bind(&task.track_id)
            .fetch_one(&pool)
            .await
            .expect("payload before");
    assert!(
        payload_before.get("progress").is_none(),
        "create must not embed progress in payload"
    );
    let bytes_before = serde_json::to_vec(&payload_before).unwrap().len();

    let progress = edgequake_tasks::TaskProgress {
        current_step: "extract".into(),
        total_steps: 4,
        percent_complete: 50,
        chunk_progress: None,
    };
    storage
        .update_task_progress(&task.track_id, &progress)
        .await
        .expect("progress update");

    let payload_after: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM tasks WHERE track_id = $1")
            .bind(&task.track_id)
            .fetch_one(&pool)
            .await
            .expect("payload after");
    let bytes_after = serde_json::to_vec(&payload_after).unwrap().len();
    assert_eq!(
        bytes_before, bytes_after,
        "progress-only update must not rewrite payload bytes"
    );

    let col: serde_json::Value =
        sqlx::query_scalar("SELECT progress FROM tasks WHERE track_id = $1")
            .bind(&task.track_id)
            .fetch_one(&pool)
            .await
            .expect("progress col");
    assert_eq!(col["percent_complete"], 50);

    let _ = sqlx::query("DELETE FROM tasks WHERE track_id = $1")
        .bind(&task.track_id)
        .execute(&pool)
        .await;
}

#[tokio::test]
async fn e2e_spec090_verify_task_keyset() {
    use edgequake_tasks::{Pagination, SortField, SortOrder, TaskFilter};

    let Some(pool) = create_test_pool().await else {
        eprintln!("Skipping keyset verify: DATABASE_URL not set");
        return;
    };
    if ensure_wave3_schema(&pool).await.is_err() || !schema_ready(&pool).await {
        eprintln!("Skipping keyset verify: schema not ready");
        return;
    }

    let storage = PostgresTaskStorage::new(pool.clone());
    let tenant_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let prefix = format!("spec090-keyset-{}", Uuid::new_v4());
    ensure_tenant_workspace(&pool, tenant_id, workspace_id)
        .await
        .expect("tenant/workspace");

    let mut tracks = Vec::new();
    for i in 0..5 {
        let mut t = Task::new(
            tenant_id,
            workspace_id,
            TaskType::Upload,
            serde_json::json!({"i": i}),
        );
        t.track_id = format!("{prefix}-{i}");
        t.created_at = Utc::now() - chrono::Duration::seconds((5 - i) as i64);
        t.updated_at = t.created_at;
        storage.create_task(&t).await.expect("create");
        tracks.push(t);
    }

    let filter = TaskFilter {
        tenant_id: Some(tenant_id),
        workspace_id: Some(workspace_id),
        status: None,
        task_type: None,
    };
    let first = storage
        .list_tasks(
            filter.clone(),
            Pagination {
                page: 1,
                page_size: 2,
                sort_by: SortField::CreatedAt,
                order: SortOrder::Desc,
                after_created_at: None,
                after_track_id: None,
            },
        )
        .await
        .expect("page1");
    assert_eq!(first.tasks.len(), 2);

    let cursor_created = first.tasks[1].created_at;
    let cursor_track = first.tasks[1].track_id.clone();
    let second = storage
        .list_tasks(
            filter,
            Pagination {
                page: 1,
                page_size: 10,
                sort_by: SortField::CreatedAt,
                order: SortOrder::Desc,
                after_created_at: Some(cursor_created),
                after_track_id: Some(cursor_track),
            },
        )
        .await
        .expect("keyset page");
    assert!(
        !second.tasks.is_empty(),
        "keyset cursor must return subsequent rows"
    );
    assert!(
        second.tasks.iter().all(|t| t.track_id.starts_with(&prefix)),
        "keyset page must stay in fixture"
    );
    // Deep page without OFFSET: no overlap with first page track ids.
    let first_ids: std::collections::HashSet<_> =
        first.tasks.iter().map(|t| t.track_id.clone()).collect();
    assert!(
        second
            .tasks
            .iter()
            .all(|t| !first_ids.contains(&t.track_id)),
        "keyset page must not repeat first-page rows"
    );

    let _ = sqlx::query("DELETE FROM tasks WHERE track_id LIKE $1")
        .bind(format!("{prefix}-%"))
        .execute(&pool)
        .await;
}

#[tokio::test]
async fn e2e_spec090_verify_pdf_id_lookup() {
    let Some(pool) = create_test_pool().await else {
        eprintln!("Skipping pdf_id verify: DATABASE_URL not set");
        return;
    };
    if ensure_wave3_schema(&pool).await.is_err() || !schema_ready(&pool).await {
        eprintln!("Skipping pdf_id verify: schema not ready");
        return;
    }

    let storage = PostgresTaskStorage::new(pool.clone());
    let tenant_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let pdf_id = Uuid::new_v4();
    ensure_tenant_workspace(&pool, tenant_id, workspace_id)
        .await
        .expect("tenant/workspace");

    let mut task = Task::new(
        tenant_id,
        workspace_id,
        TaskType::PdfProcessing,
        serde_json::json!({"pdf_id": pdf_id.to_string()}),
    );
    task.track_id = format!("spec090-pdf-{}", Uuid::new_v4());
    storage.create_task(&task).await.expect("create");

    let col: Option<String> = sqlx::query_scalar("SELECT pdf_id FROM tasks WHERE track_id = $1")
        .bind(&task.track_id)
        .fetch_one(&pool)
        .await
        .expect("pdf_id col");
    assert_eq!(col.as_deref(), Some(pdf_id.to_string().as_str()));

    let found = storage
        .find_active_pdf_processing_task(pdf_id, workspace_id)
        .await
        .expect("lookup")
        .expect("row");
    assert_eq!(found.track_id, task.track_id);

    let _ = sqlx::query("DELETE FROM tasks WHERE track_id = $1")
        .bind(&task.track_id)
        .execute(&pool)
        .await;
}

#[test]
fn contract_spec090_claim_sql_shape() {
    let src = std::fs::read_to_string("crates/edgequake-tasks/src/postgres.rs")
        .or_else(|_| std::fs::read_to_string("src/postgres.rs"))
        .expect("postgres.rs");
    assert!(src.contains("claim_arm_sql") || src.matches("SKIP LOCKED").count() >= 2);
    assert!(src.contains("LIMIT"), "claim sample must be LIMIT-bound");
    assert!(
        src.matches("SKIP LOCKED").count() >= 2,
        "dual SKIP LOCKED arms required"
    );
}
