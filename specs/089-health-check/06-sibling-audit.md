# SPEC-089 Wave 3–4 — Sibling Pool-Exhaustion Audit

> **Status**: Wave 3 + Phase 4 implemented  
> **Cross-refs**: [Laws](00-first-principles.md) · [Findings](01-finding-register.md) · [Roadmap](03-implementation-roadmap.md)  
> **Incident class**: unbounded/concurrent SQL × abandoned app timeout × shared pool

## Failure class (first principles)

```
Analytical or semi-analytical SQL
  × tokio::time::timeout / timeout_at abandons the future
  × No SET LOCAL / session statement_timeout on that path
  × Shared DATABASE_POOL_SIZE
  → Health / claim / checkpoints starve
```

## Finding table

| ID | Path | Pattern | Action |
|----|------|---------|--------|
| F-336-08 | `scan_ops` find nodes/edges by source prefix | Same `CROSS JOIN generate_series` ×256; no kill | `LocalTimeoutTx` + `SOURCE_DISCOVERY_STATEMENT_TIMEOUT_MS=2000` |
| F-336-09 | `tasks.get_statistics` (health/ready/list skip) | COUNT aggregate; 750ms/550ms abandon | `SET LOCAL 500ms` in txn (PG < app) |
| F-336-10 | Native graph SQL under `run_timed_graph_query` | App detects statement timeout but never sets it | `LocalTimeoutTx` with `graph_query_statement_timeout_ms()` (app − 250ms) |
| F-336-11 | `storage_inspector` INV-C | 50× Cypher `STARTS WITH` SeqScan risk | Batch GIN `node_counts_*_capped` shape + timeout |
| F-336-15 | `search_labels` / `popular_labels` / BFS incident edges | Native SQL without kill | `LocalTimeoutTx` on all three |
| F-336-16 | List skip stats app 100ms vs PG 500ms | App abandons before PG kill → zombie | App timeout raised to **550ms** |
| F-336-12 | Reprocess retract + cleanup double cascade | Call amplification (2× discovery) | **Phase 4**: retract SSOT only |
| F-336-13 | `run_with_read_path_guard` / worker 7200s | Outer envelopes without kill | **Phase 4**: `interactive_statement_timeout_ms`; worker SQL via Local/session (LLM wall-clock separate) |
| F-336-14 | Workspace stats 4s AGE counts | App timeout, native SQL | **Phase 4**: `WORKSPACE_STATS_STATEMENT_TIMEOUT_MS=3750` |

## DRY solution

Single helper: `LocalTimeoutTx` in
`edgequake-storage/.../helpers/statement_timeout.rs` — begin + `SET LOCAL` + commit/rollback.

Budget helpers (app − 250ms headroom):

- `graph_query_statement_timeout_ms`
- `interactive_statement_timeout_ms` (read-path / F-336-13)
- Constants: count 300 / discovery 2000 / workspace stats 3750 / task stats 500

**Invariant**: for every path that uses `tokio::time::timeout`, Postgres
`statement_timeout` must be **strictly less** than the app budget.

## Explicit non-goals (still deferred)

- Fleet-wide pool `statement_timeout` default (DDL already clears to 0)
- Denormalized `document_id` reverse index (Phase 2 schema)
