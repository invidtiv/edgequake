# SPEC-089 — WHY (Five WHYs)

> **Cross-refs**: [README](README.md) · [Laws](00-first-principles.md) · [GH-336 study](issues/GH-336-health-pool-cross-join.md)  
> **Issue**: https://github.com/raphaelmansuy/edgequake/issues/336

---

## Symptom (reporter)

At ~9,500 documents, `/health` “task queue statistics” times out 100% of the time. Concurrent pollers exhaust `DATABASE_POOL_SIZE=15`. Unrelated endpoints fail with `pool timed out while waiting for an open connection`.

`pg_stat_activity` shows a `prefixes AS (...)` / `CROSS JOIN generate_series` query running 2+ minutes.

---

## Five WHYs

### WHY 1 — Why does `/health` time out?

Because `get_statistics` (cheap `tasks` aggregate) cannot acquire a pool connection within 750ms — the pool is full of long-running AGE count queries.

### WHY 2 — Why are pool slots held for minutes?

Because Documents-list P-A3 calls `node_counts_by_source_prefixes` with **every** zero-count finished document **before** pagination, generating `N × 256` GIN probe rows (`N ≈ 9500 → ~2.4M` probes).

### WHY 3 — Why generate synthetic chunk IDs?

Because entities store exact chunk ids in `source_ids` (e.g. `doc-chunk-0`). GIN `@>` needs exact values (LAW-H4 / GH-331). The code guesses `0..SOURCE_CHUNK_PROBE_LIMIT-1` instead of reading real chunk ids — acceptable for a **page**, catastrophic for a **corpus**.

### WHY 4 — Why doesn’t the 400ms Rust timeout free the pool?

Because `tokio::time::timeout` abandons the future; Postgres keeps executing until the client cancels or `statement_timeout` fires. The count path had **no** server-side timeout (LAW-H2). Abandoned queries become zombie holders.

### WHY 5 — Why do pollers make it worse?

Docker healthcheck + processor + frontend hit `/health` and `/documents` on short intervals. New CROSS JOIN instances stack faster than old ones finish → positive feedback into pool death.

**Root cause:** Pre-pagination O(corpus × 256) GIN probe reconcile + uncancellable SQL, mis-observed as a health-check bug.

---

## Causal ASCII

```
  UI / Docker / processor poll
           |
           v
  GET /documents ──► merge ALL docs
           |
           v
  P-A3 reconcile(ALL zero-count)     ◄── BUG (pre-pagination)
           |
           v
  CROSS JOIN prefixes × 256
           |
           v
  pool slots held 2+ min ──► acquire timeout
           |
     ┌─────┴─────┬──────────────┐
     v           v              v
  /health     task claim    checkpoint
  timeout     fail          fail
```

```
  FIXED shape
           |
           v
  filter → paginate_vec → page (≤ page_size)
           |
           v
  P-A3 reconcile(PAGE only) + batch≤32
           |
           v
  SET LOCAL statement_timeout=300ms
           |
           v
  GIN @> on "Node" (GH-331 preserved)
           |
           v
  pool free → /health O(1) succeeds
```

---

## What GH-331 did / did not fix

| | GH-331 | GH-336 |
|--|--------|--------|
| Defect | JOIN parent → no child GIN | Cartesian probe scale + zombie timeout |
| Fix | JOIN `"Node"` + MATERIALIZED probes | Page scope + batch + `statement_timeout` |
| Status | FIXED | This pack |
