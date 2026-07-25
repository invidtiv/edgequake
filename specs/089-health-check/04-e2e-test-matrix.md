# SPEC-089 — E2E / Contract Test Matrix

| Test ID | Kind | Assertion |
|---------|------|-----------|
| `iss089_list_reconcile_after_page` | contract | In `list.rs`, `reconcile_entity_counts_with_graph` appears **after** `paginate_vec` |
| `iss089_batch_cap_constant` | unit | `SOURCE_PREFIX_BATCH_LIMIT == 32`, timeout ms == 300 |
| `iss089_count_statement_timeout` | contract/source | analytics count path contains `SET LOCAL statement_timeout` |
| `iss089_probe_cte_materialized` | unit | count probes CTE still `MATERIALIZED` + `generate_series` |
| `issue336_large_prefix_batch_bounded` | e2e PG | 500+ prefixes returns within budget or soft timeout; no multi-minute hold |
| `issue336_pool_coexistence_with_stats` | e2e PG | Concurrent oversized count + cheap query completes; pool usable |
| `issue336_small_batch_still_gin` | e2e PG | Small batch EXPLAIN uses `idx_node_source_ids_gin` / Bitmap |
| `issue331_*` | e2e PG | Regression guard — child GIN locality |

## Run

```bash
export DATABASE_URL=postgres://edgequake:edgequake_secret@localhost:5432/edgequake
cargo test -p edgequake-storage --features postgres --test e2e_issue336_node_counts_bounded -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_issue331_node_counts_child_gin -- --nocapture
cargo test -p edgequake-api --test contract_spec089_list_reconcile_after_page -- --nocapture
```
