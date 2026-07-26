# SPEC-090 — After Full Closeout Measurements

**Date:** 2026-07-26  
**DB:** `postgresql://edgequake:***@localhost:5432/edgequake`  
**Binary:** local `cargo` debug + `edgequake migrate`

## Gates run

| Gate | Result |
|------|--------|
| `e2e_spec090_verify` (18) | PASS |
| `e2e_spec090_multi_pool` (2) | PASS |
| `e2e_spec090_claim_bounded` (6) | PASS (`claim` n=20: min=1ms p50=2ms p95=3ms max=3ms) |
| `e2e_issue336_node_counts_bounded` (6) | PASS |
| `e2e_issue331_node_counts_child_gin` (3) | PASS |
| `edgequake migrate` (M104+M105) | PASS |
| `check_migration_checksums.sh` | expected PASS via smoke |

## Schema proofs

- `tasks` is `PARTITION BY RANGE (created_at)` with `tasks_history` + monthly children.
- `pdf_documents.pdf_data` column **absent**; bytes served from `pdf_document_blobs`.
- `eq_hot_ann_workspaces` present for HNSW mutual exclusion.
- Vector upsert stamps `embedding_model` / `embedding_dim` / `embedding_norm`.

## Pool bundle

Defaults: query=16, ingest=12, queue=4, admin=2 (sum=34).  
Isolation e2e: ingest saturation does not block query `SELECT 1`.  
`DATABASE_READ_URL` wired for query pool when set.

## Boot / migrate split

- Serving: `bootstrap_for_serving` fail-closed on pending unless `EDGEQUAKE_ALLOW_BOOT_MIGRATE=1`.
- `execute_bootstrap_apply_sql` gated unless boot escape or `EDGEQUAKE_MIGRATE_CLI=1`.
- CLI: `edgequake migrate` on admin pool.
- `make backend-bg` exports `EDGEQUAKE_ALLOW_BOOT_MIGRATE=1` by default.

## Residuals (GUARD)

- Binary+float dual HNSW unless `EDGEQUAKE_BINARY_QUANTIZE=1`.
- True remote replica ops require operator-managed `DATABASE_READ_URL`.
