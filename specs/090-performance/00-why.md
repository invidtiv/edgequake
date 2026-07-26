# SPEC-090 — WHY (Five WHYs)

> **Cross-refs**: [README](README.md) · [Laws](00-first-principles.md) · [Audit](00-audit.md) · [Counter study](issues/F-090-counter-serialization.md)

---

## Symptom

Ingestion throughput collapses under concurrent workers despite a 32-connection pool. Hybrid queries occasionally stall for seconds. Task claim latency grows with backlog depth. PDF document list pages transfer tens of megabytes. Operators see “slow RAG” and pool timeouts that look like SPEC-089 again — but the root causes are write amplification and per-request work that scales with corpus size.

---

## Five WHYs

### WHY 1 — Why does concurrent ingest serialize?

Because every vector/KV insert fires a `FOR EACH ROW` trigger that `UPDATE`s the **same single stats row** (`id = 1`). Concurrent transactions block on that tuple lock. Live proof: `n_live_tup=1` with `n_tup_upd` in the tens of thousands on `*_vectors_stats`.

### WHY 2 — Why does vacuum fall behind during large documents?

Because `upsert_report_created` holds **one transaction across all chunks** (default 1000 rows each). Long xmin horizons block database-wide vacuum while HNSW and counter updates pile up dead tuples.

### WHY 3 — Why do filtered queries pay DDL and extra round trips?

Because `query_filtered` calls `count_workspace_rows`, `ensure_hot_workspace_ann` (possible non-concurrent `CREATE INDEX`), and `partial_ann_index_exists` on every request with a `workspace_id`. Warmup exists but is not the only caller.

### WHY 4 — Why does the task queue get slower as load rises?

Because `claim_next` aggregates **all** pending/stale rows to pick a fair workspace (`O(N)` backlog), then reintroduces a non-sargable `OR` in the `FOR UPDATE` branch. Cost peaks exactly when the system is most loaded.

### WHY 5 — Why do unrelated interactive reads degrade?

Because PDF list selects full `pdf_data` + `markdown_content` (TOAST blowouts), session GUCs leak from DDL (`statement_timeout=0`, `maintenance_work_mem=256MB`), and volume-scaling probes share the same pool as latency-bound query traffic.

**Root cause:** Per-row counter serialization + document-scoped write transactions + per-request ANN/DDL probes + unbounded queue aggregates + blob-on-list — not algorithm choice (BFS batching and `UNNEST` upsert shape are sound).

---

## Causal chain (ASCII)

```
FOR EACH ROW stats UPDATE (single tuple)
        |
        v
ingest concurrency = 1  ----+---- long upsert TX holds xmin
        |                   |
        v                   v
dead tuples / vacuum lag   HNSW insert amplification
        |
        +---- shared PgPool (max 32, no after_release)
        |
        v
query path: count + CREATE INDEX + relaxed_order without reorder
        |
        v
claim_next O(N) + PDF list TOAST + leaked GUCs
        |
        v
"slow RAG" / pool starvation / credibility gap (latency sans recall)
```
