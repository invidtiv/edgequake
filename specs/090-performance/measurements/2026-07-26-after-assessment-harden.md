# SPEC-090 assessment harden — after metrics

Date: 2026-07-26  
Database: `postgresql://edgequake:edgequake_secret@localhost:5432/edgequake` (docker `edgequake-postgres`)

## Gate runs

| Suite                                | Result  | Notes                                                                              |
| --------------------------------------| ---------| ------------------------------------------------------------------------------------|
| `make spec090-perf-smoke`            | PASS    | wave1 (8) + wave2 (5) + verify (14) + claim/verify (6) + migration checksums (102) |
| `e2e_spec090_verify`                 | 14/14   | live PG + contracts                                                                |
| `e2e_spec090_claim_bounded`          | 6/6     | claim p95 5ms @ 5k backlog (budget 200ms)                                          |
| `e2e_issue331_node_counts_child_gin` | 3/3     | no regression                                                                      |
| `e2e_issue336_node_counts_bounded`   | 6/6 ×10 | discovery call-count race serialized                                               |

## Live metrics (verify suite)

| Finding | Metric | Observed |
|---------|--------|----------|
| F-090-01 | parallel upsert; stats `n_tup_upd` | concurrency ok; statement-level updates |
| F-090-02 | `xact_commit` delta vs chunks | delta 56–65 ≥ 3 chunks @ chunk_size=100 |
| F-090-03 | content_tsv FTS + 4th UNNEST | FTS hit on UNNEST-bound content |
| F-090-04 | progress column-only | payload byte size unchanged after `update_task_progress` |
| F-090-05 | no CREATE INDEX during `query_filtered` | no active CREATE INDEX observed |
| F-090-09 | clear_workspace EXPLAIN | Tid Scan + Append (UNION ctid) |
| F-090-11/12 | claim p95 @ 5k | p95 ≈ 5ms |
| F-090-14 | keyset list | second page no overlap with first |
| F-090-15 | `pdf_id` column lookup | `find_active_pdf_processing_task` hit |
| F-090-16 | list EXPLAIN | Index Scan on workspace; no `pdf_data` in plan |
| F-090-26 | embedding type | `halfvec` when env unset |
| F-090-27 | statement_timeout | cancel ~52ms under 50ms SET LOCAL |

## Product cutovers landed this wave

- `update_task_progress` column-only; create/update payload omits progress
- Worker idle tick calls `prune_terminal_tasks` (`EDGEQUAKE_TASK_RETENTION_DAYS`, default 30)
- Task list API wires `after_created_at` / `after_track_id`
- PDF dual-write fail-closed in one TX to `pdf_document_blobs`
- Graph `initialize()` fail-closed unless `EDGEQUAKE_ALLOW_NO_GRAPH=1`
- DRY `claim_arm_sql` helper for dual SKIP LOCKED arms

## Still PARTIAL / GUARD (honest)

- F-090-16: by-id still reads primary `pdf_data` (blob cutover open)
- F-090-28: env pool size knobs only (no true multi-pool)
- F-090-23 / F-090-25: GUARD unchanged
- F-090-13: monthly partitions deferred; wired prune is FIXED path
