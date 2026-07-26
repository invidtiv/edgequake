# LENS — Security / Tenancy (SPEC-090)

## F-090-17 short-id

8 hex ≈ 32 bits → birthday collision ~1% near 9k workspaces. Collision = shared physical vector table = cross-tenant data mix. Fix with full UUID namespace; fail-closed on `pg_class` conflict.

## F-090-18 deprecated RLS

`RlsContext::Drop` clearing via **another** pool connection cannot clear the intended session and may clear an innocent one. Delete deprecated surface; keep `with_rls_transaction`.

## F-090-19 AGE fail-open

Missing AGE currently warns and continues. Fail-closed unless explicit escape; surface graph readiness in `/health`.
