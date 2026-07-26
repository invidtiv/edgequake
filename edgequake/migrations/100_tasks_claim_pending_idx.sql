-- Migration 100: Partial index for bounded claim_next pending arm (SPEC-090 F-090-11/12)
--
-- WHY: claim_next pending arm filters (workspace_id, status='pending') ORDER BY created_at.
-- idx_tasks_claimable_pending (088) is created_at-only; this index supports the
-- workspace-scoped FOR UPDATE SKIP LOCKED arm after fair workspace pick.
--
-- SAFE: CREATE INDEX IF NOT EXISTS.

CREATE INDEX IF NOT EXISTS idx_tasks_claim_pending_workspace_created
    ON tasks (workspace_id, created_at ASC)
    WHERE status = 'pending';

COMMENT ON INDEX idx_tasks_claim_pending_workspace_created IS
    'SPEC-090: sargable pending arm for workspace-scoped claim_next SKIP LOCKED';
