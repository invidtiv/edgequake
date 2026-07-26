# SPEC-090 — Implementation Roadmap

## Wave 0 — Specs + baseline measurement

- [x] WHY, laws, lenses, findings, cross-ref, edge cases, measurement protocol  
- [x] Live DB baselines under `measurements/`

## Wave 1 — Bleed stop (audit #1–8 + 3.3)

1. [x] Statement-level stats triggers (F-090-01)  
2. [x] Drop blobs from PDF list (F-090-16)  
3. [x] Force exact reorder when `relaxed_order` (F-090-06)  
4. [x] Pool `after_release` (F-090-07)  
5. [x] Edge `ANY($1::text[])` (F-090-10)  
6. [x] Commit upsert per chunk (F-090-02)  
7. [x] App-layer `content_tsv` (F-090-03)  
8. [x] Checksum + unique migration numbers (F-090-21)  
9. [x] ANN off query path + probe cache (F-090-05)

## Wave 2 — Write amplification + read probes

1. [x] ANN TTL cache (F-090-05)  
2. [x] `CREATE INDEX CONCURRENTLY` (F-090-08)  
3. [x] Delete UNION sargability (F-090-09)  
4. [x] Default `halfvec` (F-090-26)  
5. [x] Vector `statement_timeout` (F-090-27)  
6. [x] Task `progress` column M099 (F-090-04)  
7. [x] `make spec090-perf-smoke` (F-090-29)

## Wave 3 — Queue + list API

1. [x] Bounded `claim_next` + dual arms (F-090-11/12)  
2. [x] Keyset pagination + estimated counts (F-090-14)  
3. [x] Metrics timeout + env workers (F-090-14)  
4. [x] `prune_terminal_tasks` (F-090-13)  
5. [x] `pdf_id` column M101 (F-090-15)

## Wave 4 — Tenancy, schema, economics

1. [x] Full-entropy workspace table ids (F-090-17)  
2. [x] Deprecated RLS Drop no-op + binds (F-090-18)  
3. [x] AGE fail-closed + health (F-090-19)  
4. [x] Reconcile state M102 (F-090-20)  
5. [x] RC version sort (F-090-22)  
6. [x] Embedding rebuild fail-closed GUARD (F-090-23)  
7. [x] `ef_construction` default 128 (F-090-24); dual HNSW GUARD (F-090-25)  
8. [x] PDF blob side-table M103 + dual-write (F-090-16)  
9. [x] Pool role env split (F-090-28)

## Definition of Done

- [x] All F-090-* FIXED or GUARD with e2e/contract  
- [x] Measurement before/after recorded for M-3.1 (+ protocol for others)  
- [x] Targeted e2e green (`e2e_spec090_*`)  
- [x] Finding register updated; sibling audit closed  

