-- Migration 101: Denormalized pdf_id for task lookups (SPEC-090 F-090-15)
--
-- WHY: find_active_pdf_* queried unindexed JSONB paths. Promote pdf_id to a column
-- populated at enqueue and index (workspace_id, pdf_id) for active-task lookups.
--
-- SAFE: ADD COLUMN IF NOT EXISTS + CREATE INDEX IF NOT EXISTS.

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pdf_id TEXT;

CREATE INDEX IF NOT EXISTS idx_tasks_workspace_pdf_id
    ON tasks (workspace_id, pdf_id)
    WHERE pdf_id IS NOT NULL;

COMMENT ON COLUMN tasks.pdf_id IS
    'Denormalized from payload task_data/metadata at enqueue (SPEC-090 F-090-15)';
