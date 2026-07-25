# LENS — Product Owner (SPEC-089)

## Acceptance

1. At ≥9k documents, `/health` returns healthy under concurrent Docker + UI + processor pollers.  
2. Documents list returns within the interactive read budget; never takes down the pool.  
3. Visible-page `entity_count` may briefly show 0 when KV is stale — **never** acceptable to kill the fleet for a heal.  
4. Operators do not need `ALTER ROLE … statement_timeout` as a permanent workaround.

## Non-goals

- Perfect AGE entity counts for every document in the corpus on every list request.  
- New admin UI for reconcile status.

## Laws

LAW-H1, LAW-H3, LAW-H5.

## Done means

Pool exhaustion class of failure from P-A3 is closed; GH-331 GIN locality preserved; SPEC-089 tests green.
