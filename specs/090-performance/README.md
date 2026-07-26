# SPEC-090 — Scalability & Database Performance

> **Product pin**: EdgeQuake v0.21.3+  
> **Status**: Waves 0–4 complete (findings FIXED/GUARD; `make spec090-perf-smoke`)  
> **Input audit**: [00-audit.md](00-audit.md) (immutable)  
> **Inherits**: [SPEC-089](../089-health-check/) · [SPEC-011](../011-storage/) · [SPEC-017 DRY/SOLID](../017-dry-and-solid-audit/) · [SPEC-042](../042-update-age-pgvector/) · [SPEC-084](../084-reliability-fix/)

## Start here

1. [00-audit.md](00-audit.md) — Independent full-code audit (source of findings)  
2. [00-why.md](00-why.md) — Five WHYs (counter → vacuum → pool → “slow RAG”)  
3. [00-first-principles.md](00-first-principles.md) — LAW-P1…P8  
4. [01-finding-register.md](01-finding-register.md) — F-090-*  
5. [02-cross-ref-matrix.md](02-cross-ref-matrix.md) — code ↔ law ↔ measure ↔ test  
6. [03-implementation-roadmap.md](03-implementation-roadmap.md) — waves + DoD  
7. [04-e2e-test-matrix.md](04-e2e-test-matrix.md) — gates  
8. [05-edge-cases.md](05-edge-cases.md) — EC register  
9. [06-measurement-protocol.md](06-measurement-protocol.md) — live DB experiments  
10. [07-sibling-audit.md](07-sibling-audit.md) — same-class sweep  
11. Issue studies → [`issues/`](issues/)  
12. Lenses → [`lenses/`](lenses/README.md)  
13. Baselines → [`measurements/`](measurements/)

## Locked decisions

1. **LAW-P1**: Any cost that grows with total data volume must not sit on a per-request path.  
2. **Counters (3.1)**: `FOR EACH STATEMENT` + transition tables; keep O(1) `SELECT row_count`.  
3. **Upsert (3.2)**: Commit per chunk; idempotent `ON CONFLICT` is the recovery model.  
4. **ANN (4.1)**: Probes cached; `CREATE INDEX` only via warmup/ingest threshold — never on `query_filtered`.  
5. **Ranking (4.2)**: `relaxed_order` forces exact reorder (`candidate_k ≈ 4 * top_k`).  
6. **Pool (4.3)**: `after_release` resets session state (`DISCARD ALL` / `RESET ALL` + `search_path`).  
7. **Indexes (4.4)**: `CREATE INDEX CONCURRENTLY` for non-empty runtime builds.  
8. **Queue (5.1–5.2)**: Bounded claimable sample + two sargable `SKIP LOCKED` arms.  
9. **PDF list (6)**: Never project `pdf_data` / `markdown_content` on list.  
10. **Tenancy (7.1)**: Full-entropy workspace table ids; fail-closed on collision.  
11. **AGE (7.3)**: Fail-closed at startup unless `EDGEQUAKE_ALLOW_NO_GRAPH=1`.  
12. **Replicas / pool split**: After Waves 1–2 only.

## Surfaces

| Surface | Role |
|---------|------|
| `edgequake-storage` | Counters, vector upsert/query, PDF list, pool, graph edges, RLS, workspace tables |
| `edgequake-tasks` | `claim_next`, list/metrics, progress columns, retention |
| `edgequake-api` | Health graph readiness, migrate/reconcile boot, admin ANN warmup |
| CI | `checksums.lock` SHA-384 + contiguous migration numbers |
| WebUI | Benefits from faster list/query; no new chrome required |

## Verification

```bash
export DATABASE_URL=postgresql://edgequake:edgequake_secret@localhost:5432/edgequake
cargo test -p edgequake-storage --features postgres --test e2e_spec090_counter_statement_trigger -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_spec090_pdf_list_no_blob -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_spec090_pool_discard -- --nocapture
cargo test -p edgequake-storage --features postgres --test e2e_spec090_edge_any_param -- --nocapture
cargo test -p edgequake-tasks --features postgres --test e2e_spec090_claim_bounded -- --nocapture
```
