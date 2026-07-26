-- SPEC-090 F-090-20: durable reconcile apply ledger (support migrations).
-- Records which support/NNN apply.sql SHA was last executed at boot.

CREATE TABLE IF NOT EXISTS edgequake_reconcile_state (
    support_version TEXT PRIMARY KEY,
    apply_sha384 TEXT NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_ms BIGINT,
    outcome TEXT NOT NULL
);
