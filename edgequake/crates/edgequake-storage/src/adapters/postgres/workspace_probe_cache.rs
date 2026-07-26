//! SPEC-090 F-090-05 — short-TTL cache for workspace ANN probes on the query path.
//!
//! Avoids per-request `count_workspace_rows` + `partial_ann_index_exists` round trips.
//! ANN DDL must never be triggered from this cache (warmup / ingest only).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceProbe {
    pub row_count: u64,
    pub partial_ann_ready: bool,
    inserted_at: Instant,
}

impl WorkspaceProbe {
    fn fresh(row_count: u64, partial_ann_ready: bool) -> Self {
        Self {
            row_count,
            partial_ann_ready,
            inserted_at: Instant::now(),
        }
    }

    fn is_fresh(&self) -> bool {
        self.inserted_at.elapsed() < TTL
    }
}

fn cache() -> &'static Mutex<HashMap<String, WorkspaceProbe>> {
    static CACHE: OnceLock<Mutex<HashMap<String, WorkspaceProbe>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(table: &str, workspace_id: &str) -> String {
    format!("{table}\0{workspace_id}")
}

pub fn get(table: &str, workspace_id: &str) -> Option<WorkspaceProbe> {
    let guard = cache().lock().ok()?;
    let p = guard.get(&key(table, workspace_id))?;
    if p.is_fresh() {
        Some(*p)
    } else {
        None
    }
}

pub fn put(table: &str, workspace_id: &str, row_count: u64, partial_ann_ready: bool) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(
            key(table, workspace_id),
            WorkspaceProbe::fresh(row_count, partial_ann_ready),
        );
    }
}

/// Test / measurement helper.
#[allow(dead_code)]
pub fn clear_for_tests() {
    if let Ok(mut guard) = cache().lock() {
        guard.clear();
    }
}
