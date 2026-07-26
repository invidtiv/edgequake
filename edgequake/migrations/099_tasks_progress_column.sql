-- ============================================================================
-- Migration 099: Task progress column (SPEC-090 F-090-04)
-- Version: 1.0.0 — 2026-07-26
--
-- PURPOSE:
--   Decouple hot progress updates from immutable payload JSONB (task_data).
--   Workers touch `progress` frequently; task_data stays stable after admit.
--
-- IDEMPOTENT: ADD COLUMN IF NOT EXISTS.
-- ============================================================================

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS progress JSONB;

COMMENT ON COLUMN tasks.progress IS
    'Task progress snapshot (SPEC-090); prefer this column over payload.progress';
