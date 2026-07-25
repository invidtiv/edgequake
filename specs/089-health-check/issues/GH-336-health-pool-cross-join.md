# `GH-336` — Health / pool exhaustion via CROSS JOIN probe reconcile

> **Priority**: P0  
> **Audit status**: FIXED (SPEC-089 Wave 1–2)  
 
> **Laws**: LAW-H1…H5  
> **GitHub**: https://github.com/raphaelmansuy/edgequake/issues/336  
> **Related FIXED**: [#331](./../../084-reliability-fix/issues/GH-331-pool-exhaustion-source-ids.md)

---

## 1. WHY

See [00-why.md](../00-why.md). Reporter correctly captured SQL shape and pool cascade; the `/health` label is a **symptom**. Root work is Documents-list P-A3 pre-pagination reconcile.

---

## 2. Audit (code is law)

| Field          | Value                                                           |
| ----------------| -----------------------------------------------------------------|
| Probe CTE      | `source_lineage_sql.rs` — `CROSS JOIN generate_series(0, $2-1)` |
| Execute        | `analytics_ops.rs` — `pg_node_counts_by_source_prefixes`        |
| Caller         | `document_read_model::reconcile_entity_counts_with_graph`       |
| List order bug | `list.rs` reconcile **before** `paginate_vec`                   |
| Health         | `health.rs` — 750ms `get_statistics` only                       |
| Verdict        | **OPEN until SPEC-089 Wave 1**                                  |

---

## 3. Solution

| Layer | Change |
|-------|--------|
| API | Reconcile after pagination |
| Storage | Batch ≤32; `SET LOCAL statement_timeout=300ms`; probe from chunk_count |
| Tests | `e2e_issue336_*` + list contract |
| Specs | This pack |

---

## 4. ASCII (before → after)

```
BEFORE:  list(all) → reconcile(N×256) → paginate → JSON
AFTER:   list(all) → filter → paginate → reconcile(page×bound) → JSON
```

---

## 5. Cross-refs

- SPEC-089 README, laws, lenses, edge cases  
- SPEC-084 GH-331 (GIN locality preserved)  
- SPEC-021 P-A3 / SPEC-054 L1-a  
