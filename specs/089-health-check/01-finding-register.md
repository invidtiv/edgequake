# SPEC-089 — Finding Register

| ID | Finding | Status | Law | Primary locus |
|----|---------|--------|-----|---------------|
| F-336-01 | Reconcile runs on full list before `paginate_vec` | FIXED | H1 | `handlers/documents/query/list.rs` |
| F-336-02 | `CROSS JOIN generate_series(0,255) × |prefixes|` unbounded | FIXED | H1/H4 | `source_lineage_sql.rs` / `analytics_ops.rs` |
| F-336-03 | No `statement_timeout` on count path → zombie queries | FIXED | H2 | `analytics_ops.rs` |
| F-336-04 | Health “task queue statistics timed out” is pool symptom | OBSERVE | H3 | `handlers/health.rs` (no query change) |
| F-336-05 | Fixed global probe 256 ignores page `chunk_count` | FIXED | H1/H5 | `document_read_model.rs` + analytics |
| F-336-06 | No scale e2e for N×256 / pool coexistence | FIXED | — | `e2e_issue336_*` + contract |
| F-336-07 | GH-331 child GIN must not regress | GUARD | H4 | `e2e_issue331_*` |
| F-336-08 | Discovery `scan_ops` CROSS JOIN without kill | FIXED | H2/H4 | `scan_ops.rs` |
| F-336-09 | `get_statistics` abandon without kill | FIXED | H2/H3 | `edgequake-tasks` postgres |
| F-336-10 | Native graph SQL under timed wrapper no GUC | FIXED | H2 | `query_ops/search.rs` / expand |
| F-336-11 | INV-C Cypher STARTS WITH ×50 | FIXED | H2/H4 | `storage_inspector.rs` |
| F-336-15 | Labels search/popular + BFS incident edges untimed | FIXED | H2 | `search.rs` / `edges_ops.rs` |
| F-336-16 | Skip-reconcile app timeout < PG stats kill | FIXED | H2 | `read_path.rs` (550ms > 500ms) |
| F-336-12 | Reprocess/delete discovery amplification | FIXED | H1 | `reprocess.rs` retract SSOT |
| F-336-13 | Outer read_path / worker abandon | FIXED | H2 | `interactive_statement_timeout_ms` + docs |
| F-336-14 | Workspace stats 4s native counts | FIXED | H2 | `analytics_ops` WORKSPACE_STATS 3750ms |

Legend: FIXED = landed · FIX→Wave3 = this wave · PHASE4 = deferred · OBSERVE = documented · GUARD = regression.
