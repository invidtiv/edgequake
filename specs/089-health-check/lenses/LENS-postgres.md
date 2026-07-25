# LENS — Postgres Expert (SPEC-089)

## Probe math

```
probes = |prefixes| × probe_limit
before: 9500 × 256 ≈ 2.4e6
after:  ≤ 32 × min(256, max_chunk+1) per batch, page-scoped
```

## Planner law (unchanged from GH-331 / IMP-031-08)

- MATERIALIZED probe-first  
- JOIN `{graph}."Node"`  
- `@>` on `(agtype_to_json(properties)::jsonb -> 'source_ids')`  
- Index: `idx_node_source_ids_gin` (M038)  
- Never Seq Scan via LIKE/unnest on hot path (LAW-H4)

## Timeout law (LAW-H2)

```sql
BEGIN;
SET LOCAL statement_timeout = '300ms';
-- count SQL
COMMIT;  -- or ROLLBACK on error
```

`SET LOCAL` is transaction-scoped so pooled connections cannot leak a permanent timeout ([PG docs](https://www.postgresql.org/docs/16/runtime-config-client.html#GUC-STATEMENT-TIMEOUT)).

## EXPLAIN contract

Small-batch plans must show Bitmap Index Scan / GIN on `idx_node_source_ids_gin`, not `_ag_label_vertex`.
