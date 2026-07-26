# SPEC-090 — E2E / Contract Test Matrix

| Test ID | Kind | Finding | Assertion |
|---------|------|---------|-----------|
| `e2e_spec090_counter_statement_trigger` | e2e PG | F-090-01 | Batch insert → ~1 stats upd/statement; STATEMENT triggerdef |
| `e2e_spec090_verify_counter_concurrency` | e2e PG | F-090-01 | Parallel upserts; no prolonged Lock on stats |
| `e2e_spec090_upsert_chunk_commit` | e2e PG | F-090-02 | Multi-chunk upsert + idempotent retry |
| `e2e_spec090_verify_upsert_xact_commit` | e2e PG | F-090-02 | `xact_commit` delta ≥ chunk count |
| `e2e_spec090_verify_content_tsv` | e2e PG | F-090-03 | content_ref FTS works; no correlated KV subquery in source |
| `e2e_spec090_verify_progress_column` | e2e PG | F-090-04 | Progress-only update; payload task_data stable |
| `e2e_spec090_ann_off_query_path` | contract | F-090-05 | `query_filtered` source has no `ensure_hot_workspace_ann` (also in verify) |
| `e2e_spec090_verify_ann_no_ddl` | e2e PG | F-090-05 | During query, no CREATE INDEX in pg_stat_activity |
| `e2e_spec090_relaxed_reorder` | e2e PG | F-090-06 | Scores non-increasing under relaxed_order |
| `e2e_spec090_pool_discard` | e2e PG | F-090-07 | after_release resets maintenance_work_mem / search_path |
| `contract_spec090_index_ddl_concurrently` | contract | F-090-08 | ddl.rs uses CONCURRENTLY for non-empty |
| `e2e_spec090_verify_delete_explain` | e2e PG | F-090-09 | clear_workspace plan not Seq Scan on fixture |
| `e2e_spec090_edge_any_param` | contract | F-090-10 | expand.rs uses `ANY($1::text[])` |
| `e2e_spec090_claim_bounded` | e2e PG | F-090-11/12 | Claim p95 flat vs 5k backlog |
| `contract_spec090_claim_sql_shape` | contract | F-090-11/12 | Sample LIMIT + two SKIP LOCKED |
| `e2e_spec090_prune_terminal_tasks` | e2e PG | F-090-13 | prune deletes old terminal rows |
| `e2e_spec090_verify_task_keyset` | e2e/API | F-090-14 | List supports after_* cursors |
| `e2e_spec090_verify_pdf_id_lookup` | e2e PG | F-090-15 | find_active_pdf uses pdf_id column |
| `e2e_spec090_pdf_list_no_blob` | contract | F-090-16 | List SELECT omits pdf_data/markdown |
| `e2e_spec090_verify_pdf_list_explain` | e2e PG | F-090-16 | EXPLAIN list + get_pdf blob + M105 column absent |
| `e2e_spec090_multi_pool` | e2e PG | F-090-28/31 | Four pools; ingest saturation ≠ query block |
| `e2e_spec090_verify_tasks_partitioned` | e2e PG | F-090-13 | RANGE parent + ≥2 partitions |
| `e2e_spec090_verify_boot_migrate_split_contract` | contract | F-090-20b | ALLOW_BOOT_MIGRATE + migrate CLI + apply gate |
| `e2e_spec090_verify_progress_and_relations_contract` | contract | F-090-04/09b/07/25 | progress wire + UNION + SET LOCAL + hot ANN |
| `e2e_spec090_verify_embedding_identity_and_manifest` | e2e PG | F-090-23/32 | identity cols on upsert + manifest probe |
| `e2e_spec090_verify_workspace_full_slug` | e2e PG | F-090-17 | Table name contains full uuid underscores |
| `e2e_spec090_verify_halfvec_default` | e2e PG | F-090-26 | New embedding column is halfvec |
| `e2e_spec090_verify_vector_timeout` | e2e PG | F-090-27 | Low timeout cancels / SET LOCAL present |
| `contract_spec090_no_foreach_row_stats` | contract | F-090-01 | No FOR EACH ROW in stats module |
| `check_migration_checksums.sh` | CI | F-090-21 | SHA-384 lock + unique numbers |

## Run

```bash
export DATABASE_URL=postgresql://edgequake:edgequake_secret@localhost:5432/edgequake
make spec090-perf-smoke
cargo test -p edgequake-storage --features postgres --test e2e_spec090_verify -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_spec090_multi_pool -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_spec090_wave1 -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_spec090_wave2 -- --nocapture
cargo test -p edgequake-tasks --features postgres --test e2e_spec090_claim_bounded -- --nocapture
```
