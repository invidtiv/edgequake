# LENS — SRE / Ops (SPEC-090)

## Boot

- sqlx migrations checksum-locked  
- Reconcile hashed into `edgequake_reconcile_state`  
- Prefer `edgequake migrate` Job; serving binary verifies drift  

## Runtime knobs

| Knob | Intent |
|------|--------|
| `EDGEQUAKE_VECTOR_UPSERT_CHUNK` | Chunk size (commit unit) |
| `EDGEQUAKE_VECTOR_STORAGE` | Default → `halfvec` |
| `EDGEQUAKE_ALLOW_NO_GRAPH` | Escape only |
| Pool split (Wave 4) | Isolate ingest vs query |

## Alerts worth having

- `*_stats.n_dead_tup` growing without bound  
- Claim p95 vs backlog depth  
- Connections with non-default `statement_timeout` after release (should be zero)
