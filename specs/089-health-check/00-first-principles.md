# SPEC-089 — First Principles

> **Cross-refs**: [WHY](00-why.md) · [Roadmap](03-implementation-roadmap.md) · [SPEC-017](../017-dry-and-solid-audit/)  
> **External**: [PG statement_timeout](https://www.postgresql.org/docs/16/runtime-config-client.html#GUC-STATEMENT-TIMEOUT) · [sqlx query timeouts](https://github.com/launchbadge/sqlx/issues/3060)

---

## Laws

| Law | Statement |
|-----|-----------|
| **LAW-H1** | Interactive reads must never do O(corpus × chunk_probe) work for a page of results. |
| **LAW-H2** | Application `tokio::time::timeout` without a server-side kill is not a timeout — it is a zombie-connection factory. |
| **LAW-H3** | Health checks must remain O(1) / cheap aggregates; they must not share fate with unbounded analytical probes. |
| **LAW-H4** | GIN `@>` exact containment stays the indexed path (keep GH-331); never replace with `LIKE`/unnest Seq Scan on the hot path. |
| **LAW-H5** | Write-path `entity_count` (P-A1) is primary SSOT; list reconcile is a **bounded** safety net for visible rows only. |

---

## SOLID / DRY mapping

| Principle | Application |
|-----------|-------------|
| **S** | `analytics_ops` owns bounded count SQL; `list.rs` owns when reconcile runs; health stays cheap. |
| **O** | Probe CTE SSOT in `source_lineage_sql`; batch/timeout constants co-located. |
| **L** | Count path still honors child `"Node"` + GIN (GH-331 contract). |
| **I** | Trait gains optional capped entry; default preserves discovery-style fallback. |
| **D** | Depend on M038 `idx_node_source_ids_gin`; no parent index. |
| **DRY** | One probe CTE helper; one timeout helper; reconcile eligibility SSOT unchanged. |

---

## Complexity budget

| Op | Before | After |
|----|--------|-------|
| List reconcile probes | `N_corpus × 256` | `≤ page_size × min(256, max_chunk+1)` and batched ≤32 |
| Pool hold on timeout | minutes (zombie) | ≤ ~300ms (`statement_timeout`) |
| Health task stats | starved | O(1) when pool free |

---

## Phase 2 (not this fix)

Denormalize `document_id` (or equivalent) onto AGE nodes + B-tree/GIN for direct `WHERE document_id = ANY($1)` counts — true O(docs) reverse index. Documented only; no schema change in Wave 1.
