-- SPEC-090 F-090-13: range-partition tasks by month + hot-ANN registry (F-090-25).
SET search_path = public;

CREATE TABLE IF NOT EXISTS eq_hot_ann_workspaces (
  table_prefix TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (table_prefix, workspace_id)
);

CREATE OR REPLACE FUNCTION edgequake_ensure_tasks_month_partitions()
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
  month_start timestamptz := date_trunc('month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC');
  i int;
  a timestamptz;
  b timestamptz;
  pname text;
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_partitioned_table WHERE partrelid = 'public.tasks'::regclass
  ) THEN
    RETURN;
  END IF;
  FOR i IN 0..3 LOOP
    a := month_start + (i || ' month')::interval;
    b := a + interval '1 month';
    pname := format('tasks_p_%s', to_char(a, 'YYYY_MM'));
    IF to_regclass(format('public.%I', pname)) IS NULL THEN
      BEGIN
        EXECUTE format(
          'CREATE TABLE %I PARTITION OF tasks FOR VALUES FROM (%L) TO (%L)',
          pname, a, b
        );
      EXCEPTION WHEN duplicate_object OR invalid_object_definition THEN
        -- Range may already be covered by the historical partition.
        NULL;
      END;
    END IF;
  END LOOP;
END;
$$;

-- Drop empty named month partitions older than retention (history partition kept).
CREATE OR REPLACE FUNCTION edgequake_detach_old_task_partitions(retention_days int)
RETURNS int
LANGUAGE plpgsql
AS $$
DECLARE
  r record;
  cnt bigint;
  dropped int := 0;
  month_start timestamptz := date_trunc('month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC');
  cutoff timestamptz := month_start - make_interval(days => GREATEST(retention_days, 1));
  part_month timestamptz;
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_partitioned_table WHERE partrelid = 'public.tasks'::regclass
  ) THEN
    RETURN 0;
  END IF;
  FOR r IN
    SELECT c.relname AS pname
    FROM pg_inherits i
    JOIN pg_class c ON c.oid = i.inhrelid
    JOIN pg_class p ON p.oid = i.inhparent
    WHERE p.relname = 'tasks'
      AND c.relname ~ '^tasks_p_[0-9]{4}_[0-9]{2}$'
  LOOP
    BEGIN
      part_month := to_date(substring(r.pname from 9), 'YYYY_MM')::timestamptz;
    EXCEPTION WHEN OTHERS THEN
      CONTINUE;
    END;
    IF part_month >= cutoff THEN
      CONTINUE;
    END IF;
    EXECUTE format('SELECT COUNT(*) FROM %I', r.pname) INTO cnt;
    IF cnt = 0 THEN
      BEGIN
        EXECUTE format('ALTER TABLE tasks DETACH PARTITION %I', r.pname);
        EXECUTE format('DROP TABLE IF EXISTS %I', r.pname);
        dropped := dropped + 1;
      EXCEPTION WHEN OTHERS THEN
        NULL;
      END;
    END IF;
  END LOOP;
  PERFORM edgequake_ensure_tasks_month_partitions();
  RETURN dropped;
END;
$$;

DO $$
DECLARE
  month_start timestamptz := date_trunc('month', CURRENT_TIMESTAMP AT TIME ZONE 'UTC');
  next_start timestamptz := month_start + interval '1 month';
  next2_start timestamptz := month_start + interval '2 month';
  next_name text := format('tasks_p_%s', to_char(next_start, 'YYYY_MM'));
BEGIN
  IF EXISTS (
    SELECT 1 FROM pg_partitioned_table
    WHERE partrelid = 'public.tasks'::regclass
  ) THEN
    PERFORM edgequake_ensure_tasks_month_partitions();
    RAISE NOTICE 'SPEC-090: tasks already partitioned — ensured future months';
    RETURN;
  END IF;

  ALTER TABLE IF EXISTS tasks DROP CONSTRAINT IF EXISTS tasks_tenant_id_fkey;
  ALTER TABLE IF EXISTS tasks DROP CONSTRAINT IF EXISTS tasks_workspace_id_fkey;

  ALTER TABLE tasks RENAME TO tasks_history;

  ALTER TABLE tasks_history DROP CONSTRAINT IF EXISTS tasks_pkey;
  ALTER TABLE tasks_history ADD CONSTRAINT tasks_history_pkey PRIMARY KEY (id, created_at);

  CREATE TABLE tasks (
    LIKE tasks_history INCLUDING DEFAULTS INCLUDING COMMENTS
  ) PARTITION BY RANGE (created_at);

  ALTER TABLE tasks ADD CONSTRAINT tasks_pkey PRIMARY KEY (id, created_at);

  -- Bounds must be literals (EXECUTE); plpgsql vars are rejected as "column references".
  EXECUTE format(
    'ALTER TABLE tasks ATTACH PARTITION tasks_history FOR VALUES FROM (MINVALUE) TO (%L)',
    next_start
  );

  EXECUTE format(
    'CREATE TABLE %I PARTITION OF tasks FOR VALUES FROM (%L) TO (%L)',
    next_name, next_start, next2_start
  );

  ALTER TABLE tasks
    ADD CONSTRAINT tasks_tenant_id_fkey
    FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id) ON DELETE CASCADE;
  ALTER TABLE tasks
    ADD CONSTRAINT tasks_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES workspaces(workspace_id) ON DELETE CASCADE;

  CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks (status);
  CREATE INDEX IF NOT EXISTS idx_tasks_type ON tasks (task_type);
  CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks (created_at DESC);
  CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks (updated_at DESC);
  CREATE INDEX IF NOT EXISTS idx_tasks_track_id ON tasks (track_id);
  CREATE INDEX IF NOT EXISTS idx_tasks_tenant_workspace ON tasks (tenant_id, workspace_id);
  CREATE INDEX IF NOT EXISTS idx_tasks_claim_pending_workspace_created
    ON tasks (workspace_id, created_at ASC)
    WHERE status = 'pending';
  CREATE INDEX IF NOT EXISTS idx_tasks_workspace_pdf_id
    ON tasks (workspace_id, pdf_id)
    WHERE pdf_id IS NOT NULL;

  PERFORM edgequake_ensure_tasks_month_partitions();
  RAISE NOTICE 'SPEC-090: tasks partitioned by created_at (monthly)';
END $$;
