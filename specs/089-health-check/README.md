# SPEC-089 — Health Check / Pool Exhaustion (GH-336)

> **Product pin**: EdgeQuake v0.21.2+  
> **Status**: Wave 1–4 implemented (Phase 4 outer envelopes + amp proved)  
> **GitHub**: [#336](https://github.com/raphaelmansuy/edgequake/issues/336)  
> **Related**: [#331](https://github.com/raphaelmansuy/edgequake/issues/331) (FIXED — child GIN locality)  
> **Inherits**: [SPEC-084](../084-reliability-fix/) · [SPEC-087](../087-fix-issues/) · [SPEC-021 P-A3](../021-storage-study/) · [SPEC-054](../054-fix-bugs-17/) · [SPEC-017 DRY/SOLID](../017-dry-and-solid-audit/)

## Start here

1. [00-why.md](00-why.md) — Five WHYs + causal ASCII (health is the symptom)  
2. [00-first-principles.md](00-first-principles.md) — LAW-H1…H5  
3. [01-finding-register.md](01-finding-register.md) — F-336-*  
4. [02-cross-ref-matrix.md](02-cross-ref-matrix.md) — code ↔ law ↔ test  
5. [03-implementation-roadmap.md](03-implementation-roadmap.md) — waves + DoD  
6. [04-e2e-test-matrix.md](04-e2e-test-matrix.md) — gates  
7. [05-edge-cases.md](05-edge-cases.md) — EC register  
8. [06-sibling-audit.md](06-sibling-audit.md) — Wave 3 same-class audit  
9. Issue study → [`issues/GH-336-health-pool-cross-join.md`](issues/GH-336-health-pool-cross-join.md)  
10. Lenses → [`lenses/`](lenses/README.md)

## Locked decisions

1. **Misattribution**: `/health` does not run the CROSS JOIN; Documents-list P-A3 reconcile does. Health times out because the pool is starved.  
2. **Reconcile after pagination** (LAW-H1) — heal `entity_count` only for returned rows.  
3. **Keep GIN `@>` probe-first** (LAW-H4 / GH-331) — never replace with LIKE Seq Scan on the hot path.  
4. **`SET LOCAL statement_timeout = 300ms`** inside a transaction on the count path (LAW-H2).  
5. **`SOURCE_PREFIX_BATCH_LIMIT = 32`** — chunk larger prefix lists.  
6. **Probe bound** = `min(256, max(chunk_count_on_batch)+1)` when chunk_count known; else 256.  
7. **Phase 2 (out of scope)**: denormalized `document_id` on AGE nodes for O(docs) reverse count.

## Surfaces

| Surface | Role |
|---------|------|
| `edgequake-api` list | Move reconcile after `paginate_vec` |
| `document_read_model` | P-A3 reconcile + probe_limit from `chunk_count` |
| `edgequake-storage` analytics | Batch cap + statement_timeout + GIN probes |
| `/health` | Unchanged cheap path; benefits from pool recovery |
| WebUI Documents | Visible-page entity counts; no new chrome |

## Verification

```bash
cargo test -p edgequake-storage --features postgres --test e2e_issue336_node_counts_bounded
cargo test -p edgequake-storage --features postgres --test e2e_issue331_node_counts_child_gin
cargo test -p edgequake-api --test contract_spec089_list_reconcile_after_page
cargo test -p edgequake-api --features postgres --test e2e_spec089_list_page_reconcile
cargo test -p edgequake-api --features postgres --test e2e_spec089_phase4
```
