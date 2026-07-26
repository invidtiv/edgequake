# SPEC-090 — Measurement Protocol

> All experiments use live Postgres (`DATABASE_URL`). Record artifacts under [`measurements/`](measurements/).

## Environment

```bash
export DATABASE_URL=postgresql://edgequake:edgequake_secret@localhost:5432/edgequake
psql "$DATABASE_URL" -c "SELECT version(); SELECT extversion FROM pg_extension WHERE extname='vector';"
```

## M-3.1 — Counter serialization

```sql
SELECT relname, n_live_tup, n_dead_tup, n_tup_ins, n_tup_upd
FROM pg_stat_user_tables
WHERE relname LIKE '%_vectors_stats'
ORDER BY n_tup_upd DESC;
```

**Pass (before fix):** `n_live_tup = 1` and `n_tup_upd ≈` total vector inserts.  
**Pass (after fix):** batch of N inserts → stats `n_tup_upd` increases by ~1 (per statement).

## M-4.3 — Session GUC leak

After any workspace vector DDL via app pool:

```sql
SHOW statement_timeout;
SHOW maintenance_work_mem;
SHOW search_path;
```

**Fail (before):** `statement_timeout=0` or `maintenance_work_mem=256MB` or `search_path` includes `ag_catalog` on a recycled conn.  
**Pass (after):** defaults restored (`0` only if cluster default; app expects reset).

## M-5.1 — claim_next cost

Seed N pending tasks; capture `EXPLAIN (ANALYZE, BUFFERS)` of claim SQL; compare N=100 vs N=10000 wall time.

**Fail (before):** time grows ~linear with N.  
**Pass (after):** time flat within sample bound.

## M-4.2 — Recall / order under relaxed_order

Compare top-10 distances with reorder off vs on (same `ef_search`). Distances must be non-increasing when reorder on.

## M-6 — PDF list TOAST

```sql
EXPLAIN (ANALYZE, BUFFERS)
SELECT id, filename, status, created_at FROM pdf_documents
ORDER BY created_at DESC LIMIT 20;
-- vs SELECT including pdf_data, markdown_content
```

**Fail (before):** large shared buffer reads / heap TOAST fetches.  
**Pass (after):** list plan touches heap metadata only.

## M-9 — HNSW residency

```sql
SELECT pg_size_pretty(pg_relation_size(indexrelid))
FROM pg_stat_user_indexes
WHERE indexdef ILIKE '%hnsw%';
SHOW shared_buffers;
```

Informational: if index ≫ `shared_buffers`, prefer halfvec / pool split before binary quantize.

## Artifact naming

`measurements/YYYY-MM-DD-M-<id>-before.md` / `-after.md`
