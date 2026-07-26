//! SPEC-076 / SPEC-090 — ANN → exact distance reorder (A3).
//!
//! When enabled, queries fetch `candidate_k` ANN hits then re-rank by exact
//! stored distance on a MATERIALIZED CTE (pgvector guidance).
//!
//! SPEC-090 F-090-06 / LAW-P5: when `hnsw.iterative_scan = relaxed_order`,
//! exact reorder is forced on with `candidate_k ≈ 4 * top_k` so results leave
//! storage in distance order.

use crate::filter_column_policy::env_flag_true;

/// Default candidate pool when reorder is on (before final `top_k`).
pub const DEFAULT_ANN_REORDER_CANDIDATE_K: usize = 50;

/// Runtime policy for two-stage ANN exact reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnExactReorderPolicy {
    pub enabled: bool,
    pub candidate_k: usize,
}

impl Default for AnnExactReorderPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            candidate_k: DEFAULT_ANN_REORDER_CANDIDATE_K,
        }
    }
}

impl AnnExactReorderPolicy {
    /// Load from env — default OFF unless coupled via [`Self::for_search`].
    pub fn from_env() -> Self {
        let enabled = env_flag_true("EDGEQUAKE_ANN_EXACT_REORDER");
        let candidate_k = std::env::var("EDGEQUAKE_ANN_REORDER_CANDIDATE_K")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_ANN_REORDER_CANDIDATE_K)
            .clamp(1, 10_000);
        Self {
            enabled,
            candidate_k,
        }
    }

    /// Resolve reorder for a search given iterative_scan mode and `top_k`.
    ///
    /// Forces reorder when mode is `relaxed_order` (pgvector may return out-of-order).
    pub fn for_search(iterative_scan_mode: &str, top_k: usize) -> Self {
        let mut policy = Self::from_env();
        if iterative_scan_mode == "relaxed_order" {
            policy.enabled = true;
            let coupled = (top_k.saturating_mul(4)).max(DEFAULT_ANN_REORDER_CANDIDATE_K);
            policy.candidate_k = policy.candidate_k.max(coupled).clamp(1, 10_000);
        }
        policy
    }

    /// Effective inner LIMIT: at least `top_k`, at most policy candidate_k when enabled.
    pub fn effective_candidate_k(&self, top_k: usize) -> usize {
        if !self.enabled {
            return top_k;
        }
        self.candidate_k.max(top_k).clamp(1, 10_000)
    }
}

/// Build ANN SELECT SQL. When `exact_reorder` is true, wraps a MATERIALIZED CTE
/// that pulls `candidate_k` then reorders by `distance + 0` to `top_k`.
///
/// `limit_param` is the bind index for the **outer** LIMIT (`top_k`).
/// When reorder is on, `candidate_k` is inlined (clamped usize) so bind arity
/// for filtered queries stays identical to the single-stage path.
pub fn build_ann_select_sql(
    table: &str,
    emb_type: &str,
    where_clause: &str,
    limit_param: u32,
    top_k: usize,
    policy: &AnnExactReorderPolicy,
) -> String {
    let where_sql = if where_clause.trim().is_empty() {
        String::new()
    } else if where_clause
        .trim_start()
        .to_ascii_uppercase()
        .starts_with("WHERE")
    {
        format!("\n            {}", where_clause.trim())
    } else {
        format!("\n            WHERE {}", where_clause.trim())
    };

    if !policy.enabled {
        return format!(
            r#"
            SELECT id, metadata, 1 - (embedding <=> $1::{emb_type}) as score
            FROM {table}{where_sql}
            ORDER BY embedding <=> $1::{emb_type}
            LIMIT ${limit_param}
            "#
        );
    }

    let candidate_k = policy.effective_candidate_k(top_k);
    // pgvector README: MATERIALIZED CTE + ORDER BY distance + 0 for strict order
    // after approximate / relaxed_order ANN.
    format!(
        r#"
            WITH candidates AS MATERIALIZED (
                SELECT id, metadata, (embedding <=> $1::{emb_type}) AS distance
                FROM {table}{where_sql}
                ORDER BY embedding <=> $1::{emb_type}
                LIMIT {candidate_k}
            )
            SELECT id, metadata, 1 - distance AS score
            FROM candidates
            ORDER BY distance + 0
            LIMIT ${limit_param}
            "#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_off() {
        let p = AnnExactReorderPolicy::default();
        assert!(!p.enabled);
        assert_eq!(p.effective_candidate_k(20), 20);
    }

    #[test]
    fn relaxed_order_forces_reorder() {
        std::env::remove_var("EDGEQUAKE_ANN_EXACT_REORDER");
        std::env::remove_var("EDGEQUAKE_ANN_REORDER_CANDIDATE_K");
        let p = AnnExactReorderPolicy::for_search("relaxed_order", 20);
        assert!(p.enabled);
        assert_eq!(p.effective_candidate_k(20), 80);
    }

    #[test]
    fn enabled_raises_candidate_k() {
        std::env::set_var("EDGEQUAKE_ANN_EXACT_REORDER", "1");
        std::env::set_var("EDGEQUAKE_ANN_REORDER_CANDIDATE_K", "50");
        let p = AnnExactReorderPolicy::from_env();
        assert!(p.enabled);
        assert_eq!(p.effective_candidate_k(20), 50);
        assert_eq!(p.effective_candidate_k(80), 80);
        std::env::remove_var("EDGEQUAKE_ANN_EXACT_REORDER");
        std::env::remove_var("EDGEQUAKE_ANN_REORDER_CANDIDATE_K");
    }

    #[test]
    fn sql_off_is_single_stage() {
        let p = AnnExactReorderPolicy::default();
        let sql = build_ann_select_sql("eq_v", "halfvec", "", 2, 20, &p);
        assert!(!sql.contains("MATERIALIZED"));
        assert!(sql.contains("LIMIT $2"));
        assert!(sql.contains("ORDER BY embedding <=> $1::halfvec"));
    }

    #[test]
    fn sql_on_uses_cte_and_distance_plus_zero() {
        let p = AnnExactReorderPolicy {
            enabled: true,
            candidate_k: 50,
        };
        let sql = build_ann_select_sql("eq_v", "vector", "WHERE workspace_id = $2", 3, 20, &p);
        assert!(sql.contains("WITH candidates AS MATERIALIZED"));
        assert!(sql.contains("LIMIT 50"));
        assert!(sql.contains("ORDER BY distance + 0"));
        assert!(sql.contains("LIMIT $3"));
        assert!(sql.contains("WHERE workspace_id = $2"));
    }
}
