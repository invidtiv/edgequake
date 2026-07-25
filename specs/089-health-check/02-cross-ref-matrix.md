# SPEC-089 — Cross-Ref Matrix

| Code / artifact | Law | Finding | Lens | Test ID |
|-----------------|-----|---------|------|---------|
| `list.rs` reconcile after `paginate_vec` | H1 | F-336-01 | fullstack, PO | `iss089_list_reconcile_after_page` |
| `SOURCE_PREFIX_BATCH_LIMIT=32` | H1 | F-336-02 | postgres, on | `iss089_batch_cap_constant` |
| `SET LOCAL statement_timeout` count txn | H2 | F-336-03 | postgres | `iss089_count_statement_timeout` |
| `SOURCE_COUNT_STATEMENT_TIMEOUT_MS=300` | H2 | F-336-03 | postgres | unit in `source_lineage_sql` |
| Probe limit from `chunk_count` | H1/H5 | F-336-05 | fullstack | `iss089_probe_from_chunk_count` |
| Child `"Node"` + GIN join | H4 | F-336-07 | postgres | `issue331_node_counts_uses_child_gin_explain` |
| `/health` task stats 750ms | H3 | F-336-04 | PO, ux | existing `health_probes` |
| Synthetic 500+ prefixes bound | H1/H2 | F-336-02/03 | postgres | `issue336_large_prefix_batch_bounded` |
| Concurrent count + stats | H2/H3 | F-336-03/04 | fullstack | `issue336_pool_coexistence_with_stats` |
| Spec pack `specs/089-health-check/` | — | all | all | doc existence |

## External refs

- Issue: https://github.com/raphaelmansuy/edgequake/issues/336  
- Prior: [`GH-331-pool-exhaustion-source-ids.md`](../084-reliability-fix/issues/GH-331-pool-exhaustion-source-ids.md)  
- PG GIN: https://www.postgresql.org/docs/current/gin.html  
- PG `statement_timeout`: https://www.postgresql.org/docs/16/runtime-config-client.html#GUC-STATEMENT-TIMEOUT  
