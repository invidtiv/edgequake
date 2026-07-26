# SPEC-090 — Finding Register

> Full closeout wave complete (2026-07-26).  
> Legend: **FIXED** = code + live PG proof · **GUARD** = intentional residual · **OPEN** = not landed

| ID | Audit | Finding | Status | Proof | Law | Primary locus |
|----|-------|---------|--------|-------|-----|---------------|
| F-090-01 | 3.1 | FOR EACH ROW stats serialize inserts | FIXED | `e2e_spec090_verify_counter_concurrency` | P2 | `row_count_stats.rs` |
| F-090-02 | 3.2 | Upsert one TX across chunks | FIXED | `verify_upsert_xact_commit` | P3 | per-chunk commit |
| F-090-03 | 3.3 | Correlated KV subquery `content_tsv` | FIXED | `verify_content_tsv` | P3 | 4th UNNEST |
| F-090-04 | 3.4 | Full JSONB rewrite on progress | FIXED | `update_task_progress` + processor `bump_task_progress` | P2 | M099 + processor |
| F-090-05 | 4.1 | ANN DDL on query path | FIXED | `verify_ann_no_ddl` | P1 | probe cache |
| F-090-06 | 4.2 | relaxed_order without reorder | FIXED | `e2e_spec090_relaxed_reorder` | P5 | `for_search` |
| F-090-07 | 4.3 | Session GUC leak | FIXED | DISCARD ALL + SET LOCAL in DDL TX | P4 | session hygiene / ddl.rs |
| F-090-08 | 4.4 | Runtime index not CIC | FIXED | `verify_cic_contract` | P4 | `ddl.rs` |
| F-090-09 | 4.5 | Delete OR column↔JSONB | FIXED | clear/delete_by_document UNION ctid | P1 | `storage_impl.rs` |
| F-090-09b | 4.5 | `delete_entity_relations` OR | FIXED | UNION ctid + verify contract | P1 | `storage_impl.rs` |
| F-090-10 | 4.6 | Edge IN-list interpolation | FIXED | `ANY($1::text[])` contract | P1 | `expand.rs` |
| F-090-11 | 5.1 | claim_next O(N) | FIXED | `e2e_spec090_claim_bounded` | P6 | bounded sample |
| F-090-12 | 5.2 | Non-sargable OR lock | FIXED | dual SKIP LOCKED | P6 | `postgres.rs` |
| F-090-13 | 5.3 | Tasks unbounded | FIXED | M104 monthly partitions + prune/detach | P6 | M104 + prune |
| F-090-14 | 5.4 | COUNT+OFFSET; metrics; workers | FIXED | keyset API + metrics timeout | P6 | tasks API |
| F-090-15 | 5.5 | PDF task JSONB lookups | FIXED | `verify_pdf_id_lookup` | P1 | M101 |
| F-090-16 | 6 | PDF list / blob cutover | FIXED | M105 DROP + get_pdf side-table | P7 | M103/M105 |
| F-090-17 | 7.1 | 8-hex short-id | FIXED | `verify_workspace_full_slug` | P8 | workspace slug |
| F-090-18 | 7.2 | Deprecated RLS Drop | FIXED | Drop no-op + binds | P8 | `rls.rs` |
| F-090-19 | 7.3 | AGE warn-not-fail | FIXED | fail-closed init + escape | P8 | connection + state |
| F-090-20 | 8.1 | Reconcile not recorded | FIXED | ledger + verify | P4 | M102 |
| F-090-20b | 8.2 / #18 | Reconcile on serving boot | FIXED | `edgequake migrate` + `ALLOW_BOOT_MIGRATE` gate | P4 | migration_bootstrap |
| F-090-21 | 8.3–8.5 | Checksum / unique mig | FIXED | `check_migration_checksums.sh` | P4 | CI |
| F-090-22 | 8.4 | RC sorts newer | FIXED | unit version compare | P8 | helpers |
| F-090-23 | 8.6 | Silent DROP on dim | FIXED | identity cols + fail-closed (`ALLOW_VECTOR_TABLE_REBUILD` escape) | P8 | upsert + dimension |
| F-090-24 | 9.1 | HNSW param drift | FIXED | default ef=128 + manifest check | P5 | config + hnsw_manifest |
| F-090-25 | 9.2 | Dual global+partial HNSW | FIXED | `eq_hot_ann_workspaces` + global exclude rebuild | P2 | ddl.rs |
| F-090-26 | 9.3 | Default Full storage | FIXED | halfvec default | P5 | capabilities |
| F-090-27 | 9.4 | Vector no timeout | FIXED | LocalTimeoutTx | P4 | statement_timeout |
| F-090-28 | 23 | Single pool all roles | FIXED | `PgPoolBundle` + `e2e_spec090_multi_pool` | P1 | pool_bundle |
| F-090-29 | 24 | No scaling harness | FIXED | `make spec090-perf-smoke` | P1 | scripts/perf |
| F-090-30 | §12 | Baseline measurements | FIXED | `measurements/` | — | Wave 0 |
| F-090-31 | 25 | Read replicas | FIXED* | `DATABASE_READ_URL` → query pool (wire ready; remote replica ops GUARD if unset) | P1 | pool_bundle |
| F-090-32 | 9.1 | Index shape manifest | FIXED | `check_hnsw_index_manifest` at migrate/boot | P5 | hnsw_manifest.rs |

\* F-090-31: in-process query pool uses `DATABASE_READ_URL` when set; operating a true remote replica is out of band.

## Residual GUARD

| Topic | Note |
|-------|------|
| Binary HNSW dual | Additive binary remains GUARD (audit 9.3) unless `EDGEQUAKE_BINARY_QUANTIZE=1` + recall gate |
| Object storage | Side-table cutover satisfies audit #22; S3 out of scope |
| HASH vector partitions | Mutual-exclusive HNSW chosen instead |
| DiskANN product | Harness-only remains |
