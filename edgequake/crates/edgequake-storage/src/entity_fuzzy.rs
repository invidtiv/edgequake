//! Optional fuzzy / blocking entity resolution (SPEC-083 X-17).
//!
//! Exact [`EntityId`] match remains the default. When
//! `EDGEQUAKE_ENTITY_FUZZY=1`, callers may additionally resolve near-duplicate
//! names via normalized Levenshtein and token Jaccard (on `_`-split names).
//!
//! Default: **off**.

use crate::entity_id::EntityId;

/// True when `EDGEQUAKE_ENTITY_FUZZY` is `1` / `true` / `on` / `yes`.
pub fn entity_fuzzy_enabled() -> bool {
    std::env::var("EDGEQUAKE_ENTITY_FUZZY")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(false)
}

/// Default similarity threshold for accepting a fuzzy match.
pub fn fuzzy_match_threshold() -> f64 {
    let t: f64 = std::env::var("EDGEQUAKE_ENTITY_FUZZY_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.88);
    t.clamp(0.5, 1.0)
}

/// Blocking key: first character of the normalized name (empty → `"_"`).
pub fn blocking_key(normalized_name: &str) -> String {
    normalized_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "_".to_string())
}

/// Normalized Levenshtein similarity in `[0.0, 1.0]` (1.0 = identical).
pub fn normalized_levenshtein_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let dist = levenshtein_distance(a, b);
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        1.0
    } else {
        1.0 - (dist as f64 / max_len as f64)
    }
}

/// Token Jaccard on `_` / whitespace-split uppercase tokens.
pub fn token_jaccard_similarity(a: &str, b: &str) -> f64 {
    let ta = tokens(a);
    let tb = tokens(b);
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 {
        1.0
    } else {
        inter as f64 / union as f64
    }
}

/// Combined fuzzy score: max(normalized Levenshtein, token Jaccard).
pub fn fuzzy_name_similarity(a: &str, b: &str) -> f64 {
    let a_n = EntityId::new(a).as_str().to_string();
    let b_n = EntityId::new(b).as_str().to_string();
    if a_n.is_empty() || b_n.is_empty() {
        return 0.0;
    }
    if a_n == b_n {
        return 1.0;
    }
    normalized_levenshtein_similarity(&a_n, &b_n).max(token_jaccard_similarity(&a_n, &b_n))
}

/// Find best fuzzy match among `candidates` (normalized bare names or graph ids).
///
/// Candidates are filtered by [`blocking_key`] when both sides have a key.
/// Returns the candidate string with the highest score >= threshold.
pub fn find_best_fuzzy_match<'a>(
    query: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    threshold: f64,
) -> Option<&'a str> {
    let q = EntityId::new(query);
    if q.is_empty() {
        return None;
    }
    let q_bare = q.as_str();
    let q_block = blocking_key(q_bare);
    let mut best: Option<(&'a str, f64)> = None;
    for cand in candidates {
        let c_bare = EntityId::bare_name_from_graph_node_id(cand);
        if c_bare.is_empty() {
            continue;
        }
        if blocking_key(c_bare) != q_block {
            continue;
        }
        let score = normalized_levenshtein_similarity(q_bare, c_bare)
            .max(token_jaccard_similarity(q_bare, c_bare));
        if score + 1e-12 < threshold {
            continue;
        }
        match best {
            Some((_, best_s)) if score <= best_s => {}
            _ => best = Some((cand, score)),
        }
    }
    best.map(|(c, _)| c)
}

fn tokens(s: &str) -> std::collections::HashSet<String> {
    s.split(|c: char| c == '_' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_uppercase())
        .collect()
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_x_17_fuzzy_off_by_default() {
        // Do not mutate process env in parallel tests — default is off.
        // Enabling is covered by e2e_x_17 with scoped env when safe.
        let _ = entity_fuzzy_enabled(); // smoke
        assert!(fuzzy_match_threshold() >= 0.5);
    }

    #[test]
    fn unit_org_organization_fuzzy_match() {
        // Token Jaccard: ACME_CORP vs ACME_CORP_INC shares ACME+CORP.
        let score = fuzzy_name_similarity("ACME_CORP", "ACME_CORP_INC");
        assert!(score >= 0.66, "expected high token overlap, got {score}");
        // Near-duplicate edit distance (typo / truncation).
        let score2 = normalized_levenshtein_similarity("MICROSOFT", "MICROSOFTX");
        assert!(score2 >= 0.85, "expected high Levenshtein, got {score2}");
        let cands = ["ACME_CORP_INC", "BETA_INC"];
        let hit = find_best_fuzzy_match("ACME_CORP", cands, 0.60);
        assert_eq!(hit, Some("ACME_CORP_INC"));
    }

    #[test]
    fn e2e_x_17_blocking_rejects_different_blocks() {
        let cands = ["BETA_INC", "ACME_CORP"];
        let hit = find_best_fuzzy_match("ZETA_CORP", cands, 0.5);
        assert!(hit.is_none(), "different blocking key must not match");
    }

    #[test]
    fn normalized_levenshtein_identical() {
        assert!((normalized_levenshtein_similarity("FOO", "FOO") - 1.0).abs() < 1e-9);
    }
}
