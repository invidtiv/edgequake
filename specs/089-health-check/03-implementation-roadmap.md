# SPEC-089 — Implementation Roadmap

## Wave 0 — Specs (this pack)

- [x] WHY, laws, lenses, findings, cross-ref, edge cases, issue study

## Wave 1 — Bleed stop (code)

1. [x] Move P-A3 reconcile to after `paginate_vec` in `list.rs` (F-336-01).  
2. [x] Add `SOURCE_PREFIX_BATCH_LIMIT`, `SOURCE_COUNT_STATEMENT_TIMEOUT_MS` in `source_lineage_sql.rs`.  
3. [x] Wrap count SQL in transaction + `SET LOCAL statement_timeout` (F-336-03).  
4. [x] Chunk prefixes into batches of 32 inside `pg_node_counts_by_source_prefixes`.  
5. [x] Pass probe limit from page `chunk_count` via capped trait/API path (F-336-05).  
6. [x] Update WHY comments citing GH-336 / SPEC-089 / LAW-H*.

## Wave 2 — Proof

1. [x] Contract: list reconcile after paginate.  
2. [x] E2E: large synthetic prefix list completes/fails soft under timeout.  
3. [x] E2E: concurrent counts + task stats / pool coexistence.  
4. [x] Keep GH-331 EXPLAIN green.

## Definition of Done (Wave 1–2)

- [x] At page scope, reconcile never builds `N_corpus × 256` probes.  
- [x] Abandoned/slow counts die within ~300ms server-side.  
- [x] Storage pool coexistence e2e green.  
- [x] GH-331 child GIN EXPLAIN still passes.  
- [x] Specs + task log written.

## Wave 3 — Sibling hardening (LAW-H2 fleet for same class)

1. [x] `06-sibling-audit.md` + finding register F-336-08…14  
2. [x] DRY `LocalTimeoutTx`; refactor count path  
3. [x] Discovery `scan_ops` SET LOCAL (2s)  
4. [x] Task `get_statistics` SET LOCAL (500ms)  
5. [x] Native graph search/popular/edges SET LOCAL = graph query budget  
6. [x] INV-C → batched GIN counts  
7. [x] E2E: list page-scope, discovery timeout, health-under-list  
8. [x] `docs/data-layer/improvements.md` RCA cross-ref  

## Phase 2 (future)

Denormalized document→entity reverse index on AGE nodes — see [00-first-principles.md](00-first-principles.md).

## Phase 4 — Outer envelopes + amp (done)

1. [x] F-336-12: reprocess admit uses retract SSOT only (no double `cleanup_document_graph_data`)
2. [x] F-336-13: `interactive_statement_timeout_ms` + read_path LAW-H2 docs (worker SQL via Local/session)
3. [x] F-336-14: `WORKSPACE_STATS_STATEMENT_TIMEOUT_MS=3750` on AGE workspace counts
4. [x] E2E: `e2e_spec089_phase4`, discovery counter, contracts; GH-331/336 still green
