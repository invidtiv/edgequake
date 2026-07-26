# SPEC-090 — Sibling Audit

Same-class defects beyond the primary audit loci.

| Sibling | Locus | Class | Action |
|---------|-------|-------|--------|
| KV row-count triggers | `row_count_stats` on KV tables | F-090-01 | Same STATEMENT trigger fix |
| Graph DDL `SET statement_timeout=0` | `graph/.../session.rs` | F-090-07 | SET LOCAL / after_release |
| Other list projections with large JSONB | conversation / blob APIs | F-090-16 | Audit list SELECT lists |
| `get_queue_metrics` vs `get_statistics` timeout asymmetry | tasks | F-090-14 | Apply LocalTimeoutTx |
| Memory task store `max_workers=4` | `memory.rs` | F-090-14 | Read from config |
| Admin ANN warmup still documented as query fallback | `admin.rs` | F-090-05 | Update docs when off-path |
| Migration support SET leaks | `support/*/apply.sql` | F-090-07/20 | Transaction + RESET |

| Sibling | Status |
|---------|--------|
| KV row-count STATEMENT triggers | FIXED (same `ensure_row_count_stats`) |
| Graph DDL session leak | MITIGATED (`after_release` RESET) |
| Task metrics timeout asymmetry | FIXED |
| Memory `max_workers=4` | FIXED (env) |
| Admin ANN query fallback docs | GUARD (query path no longer creates) |
| Migration support SET leaks | MITIGATED (pool reset) |

Closeout 2026-07-26: primary classes landed; remaining dual-HNSW write cost is GUARD (F-090-25).
