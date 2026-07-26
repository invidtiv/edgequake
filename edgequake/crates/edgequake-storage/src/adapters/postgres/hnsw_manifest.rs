//! SPEC-090 F-090-32 — HNSW index shape manifest drift check.

use crate::adapters::postgres::config::hnsw_ef_construction_from_env;
use sqlx::PgPool;

/// Target HNSW build parameters (product default + env override for ef).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HnswIndexManifest {
    pub m: u32,
    pub ef_construction: u32,
}

impl HnswIndexManifest {
    pub fn target_from_env() -> Self {
        Self {
            m: 16,
            ef_construction: hnsw_ef_construction_from_env(),
        }
    }
}

/// Drift row for a single HNSW index.
#[derive(Debug, Clone)]
pub struct HnswManifestDrift {
    pub index_name: String,
    pub indexdef: String,
    pub expected_m: u32,
    pub expected_ef: u32,
    pub found_m: Option<u32>,
    pub found_ef: Option<u32>,
}

/// Compare catalog HNSW `WITH (m, ef_construction)` to the product manifest.
///
/// Logs + records storage-drift metrics when shapes diverge. Optional auto-reconcile
/// is behind `EDGEQUAKE_HNSW_MANIFEST_AUTO_RECONCILE=1` (not implemented as DDL here —
/// operators re-run ANN warmup / migrate).
pub async fn check_hnsw_index_manifest(pool: &PgPool) -> Result<Vec<HnswManifestDrift>, String> {
    let target = HnswIndexManifest::target_from_env();
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT indexname, indexdef
        FROM pg_indexes
        WHERE schemaname = 'public'
          AND indexdef ILIKE '% USING hnsw %'
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("hnsw manifest catalog probe failed: {e}"))?;

    let mut drifts = Vec::new();
    for (name, def) in rows {
        let found_m = parse_with_option(&def, "m");
        let found_ef = parse_with_option(&def, "ef_construction");
        let m_ok = found_m == Some(target.m);
        let ef_ok = found_ef == Some(target.ef_construction);
        if m_ok && ef_ok {
            continue;
        }
        let drift = HnswManifestDrift {
            index_name: name,
            indexdef: def,
            expected_m: target.m,
            expected_ef: target.ef_construction,
            found_m,
            found_ef,
        };
        tracing::warn!(
            index = %drift.index_name,
            expected_m = drift.expected_m,
            expected_ef = drift.expected_ef,
            found_m = ?drift.found_m,
            found_ef = ?drift.found_ef,
            "SPEC-090 F-090-32: HNSW index shape drift vs manifest"
        );
        drifts.push(drift);
    }

    if drifts.is_empty() {
        tracing::debug!(
            m = target.m,
            ef_construction = target.ef_construction,
            "SPEC-090: HNSW index manifest matches target"
        );
    } else if std::env::var("EDGEQUAKE_HNSW_MANIFEST_AUTO_RECONCILE")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
    {
        tracing::warn!(
            count = drifts.len(),
            "SPEC-090: HNSW manifest auto-reconcile requested — re-run ANN warmup / migrate \
             (DDL rebuild not performed inline)"
        );
    }

    Ok(drifts)
}

fn parse_with_option(indexdef: &str, key: &str) -> Option<u32> {
    // Match `m = 16` or `ef_construction = 128` inside WITH (...).
    let needle = format!("{key} = ");
    let lower = indexdef.to_ascii_lowercase();
    let key_l = needle.to_ascii_lowercase();
    let idx = lower.find(&key_l)?;
    let rest = &indexdef[idx + needle.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_m_and_ef() {
        let def = "CREATE INDEX t ON v USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 128)";
        assert_eq!(parse_with_option(def, "m"), Some(16));
        assert_eq!(parse_with_option(def, "ef_construction"), Some(128));
    }
}
