//! Operator-facing stdout for `edgequake migrate` (SPEC-090 F-090-20b).
//!
//! Progress must be visible without `RUST_LOG` — keep `tracing` for structured logs.

#[cfg(feature = "postgres")]
use sqlx::PgPool;

/// Banner + redacted database URL.
pub fn print_banner(version: &str, redacted_database_url: &str) {
    println!("EdgeQuake migrate v{version}");
    println!("database: {redacted_database_url}");
}

/// List pending migrations before apply.
pub fn print_preflight(pending: &[(i64, String)]) {
    println!("preflight: {} pending migration(s)", pending.len());
    if pending.is_empty() {
        println!("  (schema up to date — reconcile / post-hooks still run)");
        return;
    }
    for (version, description) in pending {
        println!("  pending {version} — {description}");
    }
}

/// List versions applied in this run.
pub fn print_applied_this_run(applied: &[(i64, String)]) {
    println!("applied_this_run: {}", applied.len());
    for (version, description) in applied {
        let desc = if description.is_empty() {
            "(no description)"
        } else {
            description.as_str()
        };
        println!("  applied {version} — {desc}");
    }
}

/// Final one-line summary (machine-friendly).
pub fn print_summary(pending_before: usize, latest: Option<i64>, applied_count: usize) {
    println!(
        "migrate ok: pending_before={pending_before} latest={latest:?} applied_this_run={applied_count}"
    );
}

/// Actionable stderr hint when migrate fails.
pub fn print_failure_hint(err: &dyn std::fmt::Display) {
    eprintln!("migrate failed: {err}");
    eprintln!(
        "hint: re-run with RUST_LOG=edgequake.migration=info,edgequake=info; \
         if stuck on tasks DDL, check pg_locks / other backends holding locks on public.tasks"
    );
}

/// Post-migrate schema probes (partition / PDF cutover / HNSW / partition ensure).
#[cfg(feature = "postgres")]
pub async fn print_post_hooks(pool: &PgPool) {
    match probe_tasks_partitioned(pool).await {
        Ok((partitioned, children)) => {
            if partitioned {
                println!("tasks: RANGE-partitioned (children={children})");
            } else {
                println!("tasks: not partitioned (expected after M104)");
            }
        }
        Err(e) => eprintln!("tasks partition probe failed: {e}"),
    }

    match probe_pdf_data_column(pool).await {
        Ok(present) => {
            if present {
                println!("pdf_documents.pdf_data: present (M105 not applied)");
            } else {
                println!("pdf_documents.pdf_data: absent (blob side-table SSOT)");
            }
        }
        Err(e) => eprintln!("pdf_data column probe failed: {e}"),
    }

    match edgequake_storage::check_hnsw_index_manifest(pool).await {
        Ok(drifts) => {
            println!("hnsw_manifest: drift_count={}", drifts.len());
            for d in drifts.iter().take(5) {
                println!(
                    "  drift {} expected m={} ef={} found m={:?} ef={:?}",
                    d.index_name, d.expected_m, d.expected_ef, d.found_m, d.found_ef
                );
            }
            if drifts.len() > 5 {
                println!("  … {} more", drifts.len() - 5);
            }
        }
        Err(e) => eprintln!("hnsw manifest check failed: {e}"),
    }

    match sqlx::query("SELECT edgequake_ensure_tasks_month_partitions()")
        .execute(pool)
        .await
    {
        Ok(_) => println!("tasks partitions: ensure_month_partitions ok"),
        Err(e) => {
            // Function missing on pre-M104 DBs is non-fatal for the probe line.
            eprintln!("tasks partitions: ensure_month_partitions skipped ({e})");
        }
    }
}

#[cfg(feature = "postgres")]
async fn probe_tasks_partitioned(pool: &PgPool) -> Result<(bool, i64), sqlx::Error> {
    let partitioned: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
          SELECT 1 FROM pg_partitioned_table
          WHERE partrelid = 'public.tasks'::regclass
        )
        "#,
    )
    .fetch_one(pool)
    .await?;
    if !partitioned {
        return Ok((false, 0));
    }
    let children: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM pg_inherits i
        JOIN pg_class c ON c.oid = i.inhrelid
        JOIN pg_class p ON p.oid = i.inhparent
        WHERE p.relname = 'tasks'
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok((true, children))
}

#[cfg(feature = "postgres")]
async fn probe_pdf_data_column(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
          SELECT 1 FROM information_schema.columns
          WHERE table_schema = 'public'
            AND table_name = 'pdf_documents'
            AND column_name = 'pdf_data'
        )
        "#,
    )
    .fetch_one(pool)
    .await
}
