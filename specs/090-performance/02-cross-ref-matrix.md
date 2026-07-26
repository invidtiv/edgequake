# SPEC-090 — Cross-Ref Matrix

| Code / artifact | Law | Finding | Measure | Test ID |
|-----------------|-----|---------|---------|---------|
| `row_count_stats.rs` STATEMENT triggers | P2 | F-090-01 | M-3.1 | `e2e_spec090_counter_statement_trigger` |
| `upsert_report_created` per-chunk commit | P3 | F-090-02 | M-3.1 | `e2e_spec090_upsert_chunk_commit` |
| content_tsv UNNEST array | P3 | F-090-03 | — | unit in storage_impl |
| `AnnExactReorderPolicy` coupled | P5 | F-090-06 | M-4.2 | `e2e_spec090_relaxed_reorder` |
| `PgPoolOptions::after_release` | P4 | F-090-07 | M-4.3 | `e2e_spec090_pool_discard` |
| `pdf_list_query` metadata-only | P7 | F-090-16 | M-6 | `e2e_spec090_pdf_list_no_blob` |
| `pg_get_edges_for_node_set` ANY($1) | P1 | F-090-10 | — | `e2e_spec090_edge_any_param` |
| checksums.lock CI | P4 | F-090-21 | — | `contract_spec090_checksum_lock` |
| ANN off `query_filtered` | P1 | F-090-05 | — | `e2e_spec090_ann_off_query_path` |
| CIC runtime indexes | P4 | F-090-08 | — | unit ddl + e2e |
| delete UNION ALL / denorm | P1 | F-090-09 | — | e2e delete explain |
| `claim_next` bounded | P6 | F-090-11/12 | M-5.1 | `e2e_spec090_claim_bounded` |
| task keyset + metrics timeout | P6 | F-090-14 | — | e2e tasks list |
| `halfvec` default | P5 | F-090-26 | M-9 | `e2e_spec059_halfvec_perf_recall` |
| vector `LocalTimeoutTx` | P4 | F-090-27 | — | contract grep |
| workspace full UUID | P8 | F-090-17 | — | unit + migrate |
| AGE fail-closed | P8 | F-090-19 | — | boot / health contract |
| scaling harness | P1 | F-090-29 | M-* | release-gates |

## External refs

- Audit: [00-audit.md](00-audit.md)  
- PG triggers: https://www.postgresql.org/docs/current/sql-createtrigger.html  
- pgvector: https://github.com/pgvector/pgvector  
- CREATE INDEX CONCURRENTLY: https://www.postgresql.org/docs/current/sql-createindex.html  
- SPEC-089 sibling: ../089-health-check/  
