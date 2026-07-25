# SPEC-089 — Edge Cases

| ID | Case | Mitigation | Test |
|----|------|------------|------|
| EC-01 | All page rows `entity_count=0` | Batch ≤32; statement_timeout; soft fail → KV counts | e2e bounded |
| EC-02 | `chunk_count` unknown / 0 but entities exist | Fall back probe_limit 256 within batch | unit + e2e |
| EC-03 | Doc with >256 chunks | Lower-bound count OK; P-A1 primary (LAW-H5) | documented |
| EC-04 | Concurrent list + health + processor | Timeout kills PG; pool recovers | pool coexistence |
| EC-05 | Queue pressure skip reconcile | `should_skip_entity_reconcile` unchanged | existing |
| EC-06 | Empty page / no candidates | Early return | unit |
| EC-07 | Timeout mid multi-batch | Partial map apply; rest KV | analytics batches |
| EC-08 | GH-331 JOIN parent regression | EXPLAIN gate on child GIN | issue331 |
| EC-09 | Legacy nodes without `source_ids` | Count 0; no Seq Scan fallback on hot path | H4 |
| EC-10 | `DATABASE_POOL_SIZE=15` | Holds ≤ timeout × concurrency | pool e2e |
| EC-11 | Filter/status before page | Reconcile after filter+page — correct visible set | list order contract |
| EC-12 | Detail path single-doc reconcile | Single prefix stays cheap; same timeout | analytics path |
