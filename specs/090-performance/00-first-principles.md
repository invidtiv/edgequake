# SPEC-090 — First Principles

> **Cross-refs**: [WHY](00-why.md) · [Roadmap](03-implementation-roadmap.md) · [SPEC-017](../017-dry-and-solid-audit/) · [SPEC-089](../089-health-check/00-first-principles.md)  
> **External**: [PG CREATE TRIGGER transition tables](https://www.postgresql.org/docs/current/sql-createtrigger.html) · [pgvector iterative scans](https://github.com/pgvector/pgvector) · [CREATE INDEX CONCURRENTLY](https://www.postgresql.org/docs/current/sql-createindex.html)

---

## Laws

| Law | Statement |
|-----|-----------|
| **LAW-P1** | Any work whose cost grows with total data volume must not sit on a per-request path. |
| **LAW-P2** | Write concurrency must not collapse to a single hot tuple (counters, leases, or metadata). |
| **LAW-P3** | Transaction scope equals the smallest idempotent unit that converges under retry (chunk, not document). |
| **LAW-P4** | DDL and maintenance GUCs never leak onto pooled connections; prefer `SET LOCAL` + `after_release` reset. |
| **LAW-P5** | Approximate ANN may trade order for recall only if an exact reorder stage restores distance order before results leave storage. |
| **LAW-P6** | Claim/list/metrics paths are O(bound) in backlog depth, never O(table) on every poll. |
| **LAW-P7** | List/projection endpoints transfer metadata only; blobs are by-id (or object storage). |
| **LAW-P8** | Fail-closed on tenancy and graph availability; silent degradation is a product defect. |

---

## SOLID / DRY mapping

| Principle | Application |
|-----------|-------------|
| **S** | Stats triggers own count maintenance; query path owns search only; warmup owns ANN DDL; migrate CLI owns reconcile. |
| **O** | Storage mode / HNSW manifest / reorder policy extend via env + manifest, not forked SQL strings. |
| **L** | Vector trait implementations remain interchangeable; warmup default no-op for non-PG. |
| **I** | Narrow traits: list PDFs without blob getters; claim without full metrics payload. |
| **D** | Depend on checksum-locked migrations + reconcile state hash, not ad-hoc boot DDL. |
| **DRY** | One `LocalTimeoutTx` / pool reset helper; one ANN select builder; one row-count stats helper for vector+KV. |

---

## Complexity budget

| Op | Before | After |
|----|--------|-------|
| Stats updates per 1k-row INSERT | 1000 row locks | 1 statement update |
| Upsert TX duration | O(document chunks) | O(chunk size) |
| Filtered query probes | 2–3 RTTs + possible DDL | cached flags; DDL async |
| `claim_next` | O(pending + processing) | O(sample bound + 2 indexed locks) |
| PDF list page I/O | Σ blob sizes | metadata rows only |
| Ranking under `relaxed_order` | possibly mis-ordered | exact reorder on |

---

## Physical operations (from audit §2)

1. Token → representation (async, inference-bound)  
2. Vector proximity (bytes resident + probe)  
3. Relational expansion (already batched BFS — keep)  
4. Context assembly (LLM-bound)

Corollary: (1) must not starve (2)(3) on the same pool forever — Wave 4 pool split after write-path fixes.
