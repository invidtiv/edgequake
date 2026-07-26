# Full Code Audit: Scalability & Database Performance

<aside>
🌋

**Full code audit, grounded against source.** First-principles scalability and database-performance review of **EdgeQuake** (`raphaelmansuy/edgequake`), branch `edgequake-main`, commit `ee3b446`, v0.13.1, migrations through 098. Every finding cites a file and a code construct. Findings are ordered by physical cost, not by severity label.

</aside>

## 1. The system as the code defines it

One PostgreSQL instance, one `PgPool`, four workloads sharing it. `connection.rs` makes the sharing deliberate: `PostgresPool::from_existing` exists so every adapter uses a single pool (SPEC-011). Defaults are `max_connections = 32`, `min_connections = 1`, and no `statement_timeout` at connect time. There is no `after_release` reset hook.

Vector storage resolves through `AnnIndexPolicy::resolve(dimension, mode)` in `capabilities.rs`. HNSW is viable to 2000 dimensions on `vector` and 4000 on `halfvec`, 2001 to 4000 auto-promotes to `halfvec`, and above 4000 skips ANN entirely and degrades to sequential scan (issue #275). Distance is cosine only, asserted by contract test. The default physical layout is a shared `eq_{namespace}_vectors` table with denormalized `workspace_id` / `tenant_id` / `document_id` columns; dedicated per-workspace tables exist only through `PgWorkspaceVectorRegistry`, and the driver is heterogeneous embedding dimensions rather than tenancy.

Graph storage is Apache AGE, but the hot paths deliberately bypass Cypher and query the AGE label tables as plain SQL. `graph/query_ops/expand.rs` implements native batched BFS with hard caps on depth and node count. Task orchestration is a Postgres-backed queue in `edgequake-tasks` using `FOR UPDATE SKIP LOCKED` plus leases, with NOTIFY as wake-only.

The architecture is sound. The problems below are almost entirely about **state management, per-request overhead, transaction scope, and write amplification**, not about algorithm choice.

## 2. First principles

A GraphRAG system performs four physical operations. Everything else is bookkeeping.

1. **Token to representation.** Embedding and LLM extraction. Bound by external inference, not by Rust or Postgres.
2. **Vector proximity search.** Bound by bytes of index resident in RAM and bytes touched per probe.
3. **Relational expansion.** Bound by random page reads per hop and round trips per hop.
4. **Context assembly and generation.** Bound by LLM latency and prompt size.

Four corollaries order what follows. Step 1 is throughput-bound and asynchronous, so it must not share a resource pool with steps 2 and 3, which are latency-bound and synchronous. Step 2 is set by bytes per vector before it is set by algorithm. Step 3 is set by round trips per hop, which the BFS batching already handles correctly. Steps 2 and 3 are read-mostly and therefore replicable; step 1 is not.

A fifth principle governs most of this audit and is its organizing idea. **Any work whose cost grows with total data volume must not sit on a per-request path.** A counter that is correct but serializes, a probe that is cheap once but runs every query, an aggregate that is accurate but scans the backlog: each is individually defensible and collectively fatal. Sections 3 through 6 are largely instances of this one violation.

## 3. Write path: the counter is the bottleneck

### 3.1 Row-count triggers serialize every insert

This is the highest-severity finding in the audit. `row_count_stats.rs` creates, for each vector and KV table, a stats table holding exactly one row, pinned by `id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1)`, and two `FOR EACH ROW` triggers. The insert function body is `UPDATE {stats} SET row_count = row_count + 1 WHERE id = 1`.

Every inserted row therefore takes a row lock on the same physical tuple, and three consequences follow that no amount of tuning avoids.

Concurrent ingestion is fully serialized at the counter, so any two transactions inserting vectors block on the same tuple and the effective write concurrency of the entire pipeline is one, regardless of pool size or worker count. Each increment also creates a dead tuple, so a batch of 1,000 vectors produces 1,000 dead versions of a single-row table that autovacuum must then chase continuously. And the cost interacts multiplicatively with the batch upsert in 3.2: one `INSERT ... SELECT` of 1,000 rows fires the trigger 1,000 times and performs 1,000 sequential updates to one tuple inside a single statement.

The motivation was right. The docstring cites SPEC-011 iteration 02 Fix A, and `storage_impl.rs::count()` reads the counter with `SELECT row_count FROM {stats} WHERE id = 1` explicitly to avoid `SELECT COUNT(*)`. Avoiding the count was correct; the mechanism chosen is the most expensive one available.

**Fix.** Replace both `FOR EACH ROW` triggers with `FOR EACH STATEMENT` triggers using transition tables, supported since Postgres 10:

```sql
CREATE TRIGGER eq_x_vectors_stats_insert_trg
AFTER INSERT ON eq_x_vectors
REFERENCING NEW TABLE AS inserted
FOR EACH STATEMENT EXECUTE FUNCTION public.eq_x_vectors_stats_insert();
-- body: UPDATE stats SET row_count = row_count + (SELECT COUNT(*) FROM inserted) WHERE id = 1;
```

That turns 1,000 lock acquisitions into one. If concurrency across many simultaneous statements still matters, shard the counter across N rows keyed by `mod(hashint4(pg_backend_pid()), N)` and sum on read, which removes the single-tuple contention point entirely. Where an approximation suffices, `pg_class.reltuples` costs nothing.

### 3.2 The batch upsert holds one transaction for the whole document

`storage_impl.rs::upsert_report_created` is well engineered in most respects. It validates every embedding dimension up front, deduplicates ids within the batch to avoid the `ON CONFLICT DO UPDATE command cannot affect row a second time` error, uses `UNNEST($1::text[], $2::text[], $3::jsonb[])` so bind parameters stay at three regardless of row count, and detects true inserts with `RETURNING id, (xmax = 0) AS inserted`. All deliberate and correct.

The defect is scope. The comments state the intent plainly: "All chunks run in ONE transaction for atomicity." Chunk size defaults to 1,000 via `EDGEQUAKE_VECTOR_UPSERT_CHUNK`, but `for chunk in kept.chunks(chunk_size)` runs every chunk inside a single `tx`. A 200,000-chunk document holds one transaction across 200 statements, 200,000 HNSW insertions, and 200,000 counter increments from 3.1. That transaction pins the oldest xmin for its whole duration, which blocks vacuum **database-wide**, not just on this table, and accumulates index bloat that cannot be reclaimed until commit.

**Fix.** Commit per chunk and rely on idempotency rather than atomicity. The statement is already `ON CONFLICT DO UPDATE`, so a retry after partial completion converges to the same state. Atomicity buys very little here and costs a database-wide vacuum stall. If a specific caller genuinely needs all-or-nothing, make it opt-in.

### 3.3 Full-text vectors are computed with a correlated subquery per row

In the same statement, when the chunk KV table exists, `content_tsv` is computed as `to_tsvector('english', coalesce(t.metadata->>'content', (SELECT k.value->>'content' FROM {kv} k WHERE k.key = coalesce(t.metadata->>'content_ref', t.id) LIMIT 1), ''))`. That is a correlated lookup into the KV table for every row lacking inline content, executed during the write, plus a `to_tsvector` call per row.

The correctness reasoning is sound, since `content_ref` is the documented SSOT for chunk text. The cost is that ingestion throughput now depends on KV lookup latency. **Fix.** Resolve content in the application layer where it is already in memory and pass it as a fourth `UNNEST` array, which removes the subquery and makes write cost predictable.

### 3.4 Task rows rewrite a large JSONB payload on every progress update

`edgequake-tasks/src/postgres.rs::update_task` serializes `task_data`, `metadata`, and `progress` into one `payload` JSONB and writes the whole column on every call, even though its own comment notes "task_data is immutable". Postgres has no partial column update, so each progress tick rewrites the entire row, re-TOASTs the payload above the threshold, and emits a full-row WAL record.

The project already found this empirically. `touch_task` exists precisely to avoid it, and its docstring says it is "~10x cheaper than a full `update_task` because it doesn't serialize/deserialize the JSONB payload column". The correct diagnosis was applied to one call site only.

**Fix.** Promote the mutable fields to real columns, keeping `task_data` immutable in JSONB and moving `progress` to its own column, then update only those. Consider `ALTER TABLE tasks ALTER COLUMN payload SET STORAGE EXTERNAL` to skip compression cost on every rewrite.

## 4. Read path: per-request work that scales with data

### 4.1 Every filtered vector query makes up to three extra round trips, one of which can run DDL

This has the widest blast radius on query latency. In `storage_impl.rs::query_filtered`, before the search executes, for every request carrying a `workspace_id`:

```rust
workspace_row_count = Some(self.count_workspace_rows(ws).await?);
if crate::hnsw_partial_by_workspace_enabled() {
    let _created = self.ensure_hot_workspace_ann(ws).await?;
    wave2_partial_ready = self.partial_ann_index_exists(ws).await?;
}
```

`count_workspace_rows` is a per-workspace count on the hot path with no maintained counter behind it, unlike the global `count()`. `hnsw_partial_by_workspace_enabled()` defaults to on. And `ensure_hot_workspace_ann` is a **DDL operation invoked from a read path**: on the first query for a workspace crossing the 1,000-row threshold it issues `CREATE INDEX ... USING hnsw`, non-concurrently, and per 4.4 with no statement timeout. One unlucky user query pays for an entire index build while holding a request slot.

The surrounding code shows real sophistication about pool behavior. Comments explain that the count is taken before `begin()` to avoid a second pool acquire while holding a transaction, and that `supports_iterative_scan()` is resolved before `begin()` because doing otherwise "deadlocks when pool is saturated". The authors understand pool exhaustion well. The remaining problem is that these probes are on the request path at all.

**Fix.** Put all three probes behind a per-workspace cache with a short TTL, refreshed asynchronously. Move `ensure_hot_workspace_ann` off the query path into the background task pool, triggered by ingestion crossing the row threshold, and let queries read a cached readiness flag. The `warmup_workspace_ann` entry point already exists for exactly this and should become the only caller.

### 4.2 The default filtered configuration can return mis-ranked results

Three defaults compose badly. `HnswRuntimePolicy` defaults `iterative_scan_mode` to `relaxed_order` in `hnsw_runtime_policy.rs`; `search_tuning.rs` emits `SET LOCAL hnsw.iterative_scan = relaxed_order` for filtered queries; and `AnnExactReorderPolicy::from_env()` defaults to **off** in `ann_exact_reorder_policy.rs`.

By pgvector's definition `relaxed_order` may return rows out of distance order. The repository already contains the remedy, a `MATERIALIZED` CTE reordered by `distance + 0` built by `build_ann_select_sql`, and ships it disabled. The shipping default trades ranking correctness for latency without telling the operator.

**Fix.** Couple the policies: when `iterative_scan` resolves to `relaxed_order`, force exact reorder on with `candidate_k` near `4 * top_k`. The cost is one sort over a few hundred rows already in memory. Still the cheapest high-value change in the audit.

### 4.3 Session state leaks from the DDL and reconcile paths, not from search

This finding is narrower than an earlier pass claimed, and the correction matters. The **search path is correct**: both `query` and `query_filtered` open an explicit transaction and apply tuning through `SET LOCAL`, with a comment stating the intent never to leak "onto the shared pooled connection".

The leak is elsewhere. `vector/ddl.rs::setup_vector_ddl_session` issues plain `SET statement_timeout = 0`, `SET lock_timeout = '5s'`, and `SET maintenance_work_mem = '256MB'` on a pooled connection. `migrations/support/092/apply.sql` opens with `SET statement_timeout = 0`, executed via `reconcile/mod.rs::execute_bootstrap_apply_sql` using `sqlx::raw_sql(sql).execute(pool)`. And `graph/query_ops/expand.rs` sets `search_path = ag_catalog, "$user", public` without resetting it. None are `SET LOCAL`, and `connection.rs` configures no `after_release` hook.

A connection that has served DDL or reconcile returns to the pool with no statement timeout and 256 MB of maintenance memory reserved; across 32 connections that is up to 8 GB. The `row_count_stats.rs` module header documents the downstream symptom exactly: a pooled connection still carrying graph `search_path` causes `CREATE FUNCTION` to land in `ag_catalog`, after which `CREATE TRIGGER` fails with "function eq_*_stats_insert() does not exist". The invariant is understood; it is not enforced at the pool boundary.

**Fix.** One line of configuration plus a mechanical sweep. Add an `after_release` hook running `DISCARD ALL`, or at minimum `RESET ALL; SET search_path TO public`, and convert the remaining plain `SET` statements to `SET LOCAL` inside explicit transactions.

### 4.4 Index builds block writes

`create_table`, `ensure_ann_index`, and `ensure_partial_hnsw_for_workspace` all emit `CREATE INDEX IF NOT EXISTS ... USING hnsw`, never `CONCURRENTLY`. The reconcile path does the same for `idx_node_eq_node_id`, `idx_edge_eq_source_target_rel`, `idx_edge_eq_source_id`, and `idx_edge_eq_target_id`. Combined with `statement_timeout = 0` from 4.3, a build on a large table holds a write-blocking lock for an unbounded duration. The 5 second `lock_timeout` bounds acquisition, not hold.

**Fix.** `CREATE INDEX CONCURRENTLY` for all runtime index creation, run outside a transaction block, with cleanup of `INVALID` leftovers on retry. Reserve the blocking form for the empty-table case where it is free.

### 4.5 Delete predicates defeat every index on the table

Four delete paths in `storage_impl.rs` use `OR` across a materialized column and its JSONB equivalent, which prevents the planner from using either index and forces a sequential scan.

| Method | Predicate | Effect |
| --- | --- | --- |
| `clear_workspace` | `workspace_id = $1 OR metadata->>'workspace_id' = $1` | Full scan per workspace deletion |
| `delete_by_document` | Four-way OR including `id LIKE $2` | Full scan on every re-ingest and force-reindex |
| `delete_entity` | `metadata->>'entity_name' = $1` | Full scan per entity, called in loops |
| `delete_entity_relations` | `metadata->>'source' = $1 OR metadata->>'target' = $1` | Full scan per entity |

The reasoning is documented and honest: the comments explain that dual-write era rows may carry only one side, so matching the column alone "would silently leave legacy rows behind on delete". Correctness over performance is the right call in isolation. But `delete_by_document` runs on every re-ingest of an existing document, so this is a hot path, not a migration-window path.

**Fix.** Backfill the materialized columns once, assert completeness, then drop the JSONB arms. Until then add expression indexes on `(metadata->>'entity_name')` and `(metadata->>'source')`, and rewrite each two-arm OR as a `UNION ALL` of two indexed scans, which is sargable where `OR` is not.

### 4.6 The graph edge lookup defeats the plan cache

`pg_get_edges_for_node_set` interpolates escaped string literals into an `IN (...)` list and emits that list twice in the same statement. Every distinct node set produces unique SQL text, so no prepared statement is ever reused, parse and plan time scales with node count, and `pg_stat_statements` fragments into thousands of near-duplicate entries. Escaping is also the only injection defense.

**Fix.** Bind once as `= ANY($1::text[])` and reference the parameter twice. This restores plan caching, removes the injection surface, and makes the query measurable as a single entry.

## 5. Task queue: the claim scans the backlog

### 5.1 `claim_next` aggregates every pending task on every claim

The claim query in `edgequake-tasks/src/postgres.rs` implements workspace fairness (SPEC-084, GH-316) through a chain of CTEs. The intent is good and the fairness requirement is real. The cost is not what the annotation claims.

The query builds `pending` as `SELECT track_id, workspace_id, created_at FROM tasks WHERE status = 'pending'` with **no limit**, builds `stale` similarly over expired-lease processing rows, unions them into `claimable`, computes `ws_load` as a `GROUP BY workspace_id` aggregate over all live processing rows, then computes `fair_workspace` as another `GROUP BY` over all of `claimable` ordered by load and age with `LIMIT 1`.

The `@complexity` annotation on the function states `time: O(W + log N) fair pick + row lock`. The code does not do that. Selecting the fairest workspace requires grouping the entire claimable set, so the true cost is **O(N) in backlog depth per claim attempt**, plus O(P) in live processing rows. Every worker pays this on every poll. With four workers and a 100,000-row backlog, the queue spends its time repeatedly aggregating the backlog rather than draining it, and the cost rises exactly when the system is most loaded. Code is law, and here the law and its documentation disagree.

### 5.2 The locking branch reintroduces the OR the CTEs were written to avoid

The comment above the query is explicit about the optimization: split status predicates via `UNION ALL` "so planner can use idx_tasks_claim_workspace_created ... instead of a non-sargable OR across statuses". The `candidate` CTE then does this:

```sql
WHERE t.status = 'pending'
   OR (t.status = 'processing'
       AND (t.lease_expires_at IS NULL OR t.lease_expires_at < NOW()))
```

That is precisely the non-sargable OR across statuses the split was designed to eliminate, reintroduced in the one branch that actually takes the locks. A second comment acknowledges the constraint that forced it, since Postgres forbids `FOR UPDATE` on a `UNION` result. The constraint is real; the workaround silently discards the benefit.

**Fix for 5.1 and 5.2 together.** Bound the candidate set before aggregating. Pick the fair workspace from a bounded sample rather than the full backlog, for example the oldest 1,000 claimable rows, which preserves fairness in practice because starvation is a property of the head of the queue. Then run two separate `FOR UPDATE SKIP LOCKED LIMIT 1` statements, one per status arm, each sargable against its own partial index, and take the first that returns. Two indexed statements beat one unindexed statement. Add a partial index `ON tasks (workspace_id, created_at) WHERE status = 'pending'` so the pending arm never touches completed rows.

### 5.3 The tasks table grows without bound and everything above scales with it

Nothing observed in `postgres.rs` prunes or partitions `tasks`. Completed, failed, and cancelled rows accumulate permanently in the same heap that `claim_next` scans, that `get_total_count` counts, and that `get_queue_metrics_filtered` aggregates. Partial indexes limit the damage for well-planned queries, but 5.1 and 5.4 are not well-planned queries.

**Fix.** Partition `tasks` by range on `created_at` monthly and detach old partitions, or add a retention job moving terminal rows older than N days to `tasks_archive`. Partitioning is preferable because it also bounds index size for the claim path.

### 5.4 Listing and metrics run unbounded aggregates, inconsistently guarded

Three small issues in one file that together make the task API a reliable source of slow queries.

`list_tasks` calls `get_total_count`, which executes `SELECT COUNT(*) FROM tasks` with the same filters, on **every page request**. Pagination also uses `LIMIT {} OFFSET {}` with values interpolated rather than bound, so deep pagination is O(offset) and the statement text varies per page, defeating plan caching a second time.

`get_queue_metrics_filtered` computes five aggregates including `AVG(EXTRACT(EPOCH FROM (started_at - created_at)))` and `MAX(...)` across the entire tasks table with no time bound and, unlike its sibling, **no statement timeout**. `get_statistics` does this correctly, wrapping its counts in a transaction with `SET LOCAL statement_timeout = '500ms'` and a comment citing SPEC-089 Wave 3 and the GH-336 zombie-query class. The guard was written and then not applied to the neighbouring method.

Finally, `worker_utilization` is computed against a hardcoded `let max_workers = 4u32` with the comment "assuming 4 max workers". Any deployment with a different worker count reports a wrong utilization figure, which matters because this is an autoscaling-relevant signal.

**Fix.** Use keyset pagination on `(created_at, track_id)` instead of OFFSET, bind the limit, and return an estimated total from `pg_class.reltuples` or a cached count. Apply the `SET LOCAL statement_timeout` guard from `get_statistics` to `get_queue_metrics_filtered` and bound its aggregates to a recent window. Read the real worker count from configuration.

### 5.5 PDF task lookups filter on unindexed JSONB paths

`find_active_pdf_processing_task` and `find_active_pdf_ingest_task` filter on `payload->'task_data'->>'pdf_id'` and `payload->'task_data'->'metadata'->>'pdf_id'`. Both are workspace-scoped, which helps, but neither JSONB path is a column and no matching expression index was observed. **Fix.** Promote `pdf_id` to a real nullable column populated at enqueue time and index it together with the workspace.

## 6. Blob storage: the list endpoint ships every PDF

`pdf_list_query.rs::list_pdfs_dynamic` builds the PDF listing query. Its projection includes `pdf_data`, the full binary content of the PDF, alongside `markdown_content`. The query is a paginated list ordered by `created_at DESC` with `LIMIT` and `OFFSET`.

So rendering a page of 20 PDFs reads 20 complete PDF binaries out of TOAST storage, decompresses them, transfers them to the application, and deserializes them into `PdfDocument` structs, so that the caller can display filenames and statuses. At an average of 2 MB per PDF that is 40 MB of I/O and network per page view. It also evicts a large amount of useful data from `shared_buffers` on every list request, degrading unrelated query performance.

This is the clearest single-line win in the audit. **Fix.** Remove `pdf_data` and `markdown_content` from the list projection and fetch them only in the by-id path. Separately, move PDF binaries out of the row into a dedicated table keyed by `pdf_id`, or out of Postgres entirely into object storage with only a reference retained, which also removes them from every base backup and from WAL.

## 7. Tenancy and isolation

### 7.1 Eight hex characters is not enough workspace identity

`workspace_vector.rs` derives table names from `&config.workspace_id.to_string()[..8]`, producing `eq_{ns}_ws_{short_id}_vectors`. That is 32 bits. By the birthday bound, collision probability reaches roughly 1 percent near 9,000 workspaces and 50 percent near 77,000. A collision means two tenants share a physical vector table. The code comment acknowledges "same short-id collision risk" in the narrower context of orphan tables, but the real consequence is cross-tenant data mixing in a product whose central claim is fail-closed workspace isolation. This is a security finding surfaced by a scalability review, and it only bites at scale.

**Fix.** Use the full UUID with hyphens mapped to underscores, or a 64-bit hash with a `pg_class` uniqueness check at creation that fails closed on collision.

### 7.2 The RLS transaction helper is right; the deprecated surface around it is not

Credit first, because this module shows the strongest engineering judgment in the codebase. `rls.rs::with_rls_transaction` is correct and its docstring explains exactly why it must exist: `set_tenant_context()` uses `set_config(..., is_local = true)`, so calling it in autocommit clears the GUC when the statement ends and every subsequent RLS policy silently matches nothing. The module states the invariant as "GUC MUST be set inside BEGIN … COMMIT on the same connection" and deprecates three older entry points with specific spec references. Silent-deny-on-misuse is a nasty failure mode and it has been correctly diagnosed and fenced.

Two problems remain in the deprecated surface, which is still compiled and callable. `RlsContext::drop` spawns a Tokio task that calls `clear_tenant_context(&pool)`, which acquires **a different connection from the pool** and clears context there. It cannot clear the connection it meant to clear, so it both fails at its purpose and can corrupt the state of an unrelated in-flight request. And `RlsQueryBuilder::where_clause()` interpolates tenant and workspace UUIDs directly into SQL text with `format!`. UUIDs are structurally safe from injection, but every distinct tenant produces a distinct statement, fragmenting the plan cache by tenant.

**Fix.** Delete the deprecated surface rather than deprecating it further, since the replacement is complete. Change `where_clause` to emit placeholders and return the bind values.

### 7.3 Graph storage fails open

`setup_extensions` treats a missing AGE extension as a warning and continues, logging that graph operations will use a fallback. Everywhere else the system is emphatically fail-closed, including forced RLS in migrations 081 and 096 and the "fail-closed DDL policy" the vector annotations advertise. A deployment that silently loses its graph engine still answers queries, just without graph-derived context, and the degradation is invisible in the response. This matters most on newer Postgres majors, because AGE support for each new major historically lags by months.

**Fix.** Make AGE availability a startup readiness gate unless an explicit `EDGEQUAKE_ALLOW_NO_GRAPH=1` escape is set, and surface graph availability in the health endpoint and in query responses.

## 8. Migration and schema evolution

Schema evolution is `sqlx` against `_sqlx_migrations` with SHA-384 checksums, a committed `migrations/checksums.lock`, and a Rust reconcile layer in `edgequake-api/src/state/migration_bootstrap/`. The convention in `migrations/NOTES.md` is `NNN_descriptive_name.sql`, currently 001 through 098. There is no Flyway anywhere in the tree.

### 8.1 Two tiers, only one of them locked

`NOTES.md` opens with "Never edit an existing `.sql` migration file", and that tier holds. The same file then documents a second tier under "Every-boot reconcile SSOT (not checksum-locked)", and there are **31 directories** under `migrations/support/` dispatched by **34 hooks** registered in `reconcile/mod.rs`, of which only `m040` runs in the background.

The pattern is a defensible response to a real constraint, since AGE creates `Node` and `EDGE` label tables lazily per graph and a static migration cannot know which graphs exist. But the escape hatch has swallowed the guarantee. The header of `support/092/apply.sql` states that `092_eq_id_denorm_marker.sql` and `097_edge_multigraph_rel_type.sql` are marker only, so the actual DDL for the `eq_*` denormalization columns, indexes, and triggers lives entirely in the non-checksummed file. `_sqlx_migrations` records that 092 succeeded while carrying no information about what schema it produced.

**Fix.** Record reconcile execution as first-class state in an `edgequake_reconcile_state (support_version, apply_sha384, applied_at, duration_ms, outcome)` table, writing the hash of the `apply.sql` actually executed.

### 8.2 Every boot pays an unbounded reconcile

In the non-maintenance path, `support/092/apply.sql` loops over every graph in `ag_catalog.ag_graph` and runs an unbatched `UPDATE ... WHERE eq_node_id IS NULL` on `Node` and `EDGE` per graph. The backfill is correctly NULL-only so it converges to zero rows changed, but the predicate still requires a relation scan per graph on every boot, permanently. The well-written `ctid`-batched path with 10,000-row batches exists but is gated behind `EDGEQUAKE_EQ_MAINTENANCE=1`, so the safe path is opt-in and the unbounded path is the default. With `statement_timeout = 0` set at the top of the file, there is no upper bound on how long a boot can take.

**Fix.** Move reconcile out of the serving binary into an `edgequake migrate` subcommand run as a Job or init container, leaving boot with read-only verification that fails closed on drift. Invert the default so batched backfill is always used, and add an `EXISTS (... LIMIT 1)` precondition before entering any backfill loop.

<aside>
⚠️

**Not verified.** `migration_bootstrap/mod.rs` is roughly 80 KB and was not read in full, so whether an advisory lock already wraps the reconcile phase is unconfirmed. `sqlx` locks the migration phase, but reconcile runs after it. Concurrent replica boots running blocking `CREATE INDEX` on AGE tables would be serious. Check this before acting on the index recommendations.

</aside>

### 8.3 Checksum repair is disciplined but the enforcement point is missing

`reconcile/m071.rs` and `m078.rs` detect known-bad historical checksums and **refuse to rewrite migration history** unless `EDGEQUAKE_DEV_MODE` is set, returning an operator runbook instead (SPEC-083 X-02). `m071.rs` carries a contract test that greps its own source to prove the guard survives refactoring. Fail-loud-by-default on schema history is correct and uncommon, and deserves credit.

But the existence of two repair modules is evidence that the immutability rule was broken twice in shipped releases: 071 for the pgvector dimension ceiling in #275, and 078 for an invalid `->>>` operator in v0.13.2, corrected in v0.13.3. `checksums.lock` is committed but nothing verifies it in CI. A pre-merge job recomputing SHA-384 over every `migrations/*.sql` and diffing the lockfile would have converted both incidents into failed pull requests instead of field upgrade failures. That single check is the highest-leverage migration improvement available. Separately, `EDGEQUAKE_DEV_MODE` is a broad global flag authorizing a narrow dangerous operation; a version-scoped `EDGEQUAKE_ALLOW_CHECKSUM_REPAIR=71,78` would keep the blast radius proportional.

### 8.4 Version comparison treats release candidates as newer than releases

`migration_bootstrap/helpers.rs::extension_version_at_least` splits on every non-digit and compares numerically. It handles the case that matters, since `0.8.10` yields `[0,8,10]` and correctly beats `0.8.2`. But `0.8.0-rc1` parses to `[0,8,0,1]`, which compares greater than `[0,8,0]`. Since this backs `pgvector_meets_cve_floor`, a release candidate would satisfy the CVE-2026-3172 gate. **Fix.** Treat a non-numeric suffix as a pre-release sorting below the release, or reject unrecognized formats.

### 8.5 Sequential integers do not survive parallel development

With 98 migrations on a strict integer sequence and `NOTES.md` tracking "Next available: 099", two concurrent branches both pick 099, git merges cleanly because the descriptions differ, and the conflict surfaces only as a checksum mismatch at deploy. Latent today given single-author development; standard as soon as it is not. **Fix.** A CI check asserting migration numbers are unique and contiguous, cheaper than timestamp prefixes and preserving readable ordering.

### 8.6 Embeddings have a semantic version that no migration tool can see

One class of schema change is specific to AI systems. The physical schema and the semantic contract of the stored vectors are different things, and only the first is under migration control. Changing an embedding model, a chunking strategy, or a normalization step invalidates every stored vector without changing a single column type.

The expand-and-contract discipline that applies to columns must therefore extend to embeddings: write the new representation alongside the old, backfill by re-embedding as a tracked job, cut reads over once coverage is complete, then drop the old. What happens without this is visible in `create_workspace_storage`, which calls `storage.ensure_dimension(config.dimension)` and, on mismatch, drops and recreates the table. Switching a workspace from a 1536-dimension provider to a 768-dimension one silently discards every embedding, logged at `info`. Re-embedding is the most expensive operation in the system, so this is a cost event as well as a data-loss event.

**Fix.** Record embedding identity per row, meaning model name, dimension, and normalization, so a mixed-generation table is queryable rather than corrupt. Replace the implicit `DROP TABLE` with an explicit re-embed task that creates the new-dimension table alongside and cuts over atomically.

## 9. Index shape and storage economics

### 9.1 Index shape is not reproducible

Three HNSW parameter sets coexist. `config.rs` defaults `m = 16` with `ef_construction = 64`, its own comment recommends 128 for production, and migration 071 rebuilds at `ef_construction = 32`. Achieved recall therefore depends on which code path last created the index, which is not a property an operator can reason about or reproduce.

**Fix.** Declare one target index shape per dimension class in a manifest, reconcile actual against declared at startup, and report drift rather than silently accepting whatever exists. Standardize on `m = 16`, `ef_construction = 128` for production.

### 9.2 Hot workspaces are indexed twice

`ensure_hot_workspace_ann` creates a partial HNSW for any workspace above 1,000 rows, `partial_by_workspace` defaults to on, and the docstring confirms the global HNSW is retained. Rows in hot workspaces are therefore inserted into two HNSW graphs. HNSW insertion is the expensive index operation, roughly logarithmic with a large constant driven by `ef_construction`, so this doubles the dominant write cost. Every additional hot workspace also adds a partial index whose predicate every insert on the shared table must evaluate.

**Fix.** Make global and partial mutually exclusive per workspace, or adopt native `PARTITION BY HASH (workspace_id)` with one HNSW per partition, which gives the same pruning with exactly one index per row.

### 9.3 Full float32 is the default storage mode

`VectorStorageMode::from_env` defaults to `Full`. For 1536 dimensions that is 6,144 bytes per vector before HNSW link overhead.

| Representation | Bytes per 1536-dim vector | Vector data at 10M chunks |
| --- | --- | --- |
| `vector` (float32, current default) | 6,144 | about 61 GB |
| `halfvec` (float16) | 3,072 | about 31 GB |
| `bit` (binary quantized) | 192 | about 1.9 GB |

As of July 2026, `halfvec` at cosine distance is a well-established near-free win for embeddings of 1024 dimensions and above, typically under 1 percent recall loss. Binary quantization is strong at 1536 and above with normalized embeddings and a rerank stage, and poor below about 768. Note that `binary_quantize_policy.rs` builds an **additive** index and explicitly does not drop the float HNSW, so as shipped it increases footprint rather than reducing it.

**Fix.** Default `EDGEQUAKE_VECTOR_STORAGE=halfvec`. Gate binary quantization on dimension at or above 1024 plus a measured recall@10 guard, and drop the float HNSW when the binary index is adopted.

### 9.4 Vector queries have no timeout, graph queries do

The graph path is well built here: `LocalTimeoutTx` plus `graph_query_statement_timeout_ms` bound every graph statement, and `get_statistics` bounds its counts at 500ms. The vector search path has no equivalent, and per 4.3 may run on a connection where the timeout was explicitly disabled. **Fix.** Apply the same transaction-local timeout to the vector search path, as a default rather than a per-call-site decision.

## 10. Platform baseline as of July 2026

The code already probes `server_version_num` and gates `uuidv7()` on major 18, defaulting to 16 on probe failure. That is the right shape.

| Component | Recommended floor | Why it matters for EdgeQuake |
| --- | --- | --- |
| PostgreSQL | **18** | Native `uuidv7()` already used by `capabilities.rs`. Asynchronous I/O with `io_method = io_uring` directly targets HNSW's random-read profile. B-tree skip scan improves the `(status, workspace_id, created_at)` claim index from migration 098 |
| PostgreSQL fallback | 17 | Streaming I/O for sequential scans and the rewritten vacuum memory structure, which matters given `batch_deletion` and the counter bloat in 3.1. Backfill a `uuidv7()` polyfill so identifier ordering matches 18 |
| pgvector | **0.8.5** | Code encodes a CVE-safe floor of 0.8.2 for CVE-2026-3172 affecting parallel HNSW builds, and recommends pinning 0.8.5. Iterative scan requires 0.8.0 minimum |
| Apache AGE | **1.7** | `age_supports_rls` and the COPY loader both gate on 1.7. Verify the AGE build targets your Postgres major before upgrading, per 7.3 |
| pgvectorscale | Optional | `diskann_runtime_policy.rs` is harness-only today. Worth a bake-off above roughly 10M vectors, where DiskANN's disk-resident design beats HNSW's RAM residency requirement |

On `uuidv7`: the benefit is not novelty. Time-ordered primary keys turn random B-tree insertion into near-append, reducing page splits, WAL volume, and index bloat across every high-insert table. On a write-dominated ingestion path this is a real throughput lever and should not be conditional on the Postgres major.

## 11. Prioritized roadmap

Ordered by gain per unit of complexity. Items 1 through 8 are all small and together address the dominant costs.

| # | Action | Finding | Effort | Confidence |
| --- | --- | --- | --- | --- |
| 1 | Statement-level triggers with transition tables for row counters | 3.1 | Low | High |
| 2 | Drop `pdf_data` and `markdown_content` from the PDF list projection | 6 | Low | High |
| 3 | Cache workspace probes; move `ensure_hot_workspace_ann` off the query path | 4.1 | Low | High |
| 4 | Commit the upsert per chunk instead of one transaction per document | 3.2 | Low | High |
| 5 | Pool `after_release` reset plus `SET LOCAL` in DDL and reconcile paths | 4.3 | Low | High |
| 6 | Force exact reorder whenever `iterative_scan` is `relaxed_order` | 4.2 | Low | High |
| 7 | CI job recomputing SHA-384 and diffing `checksums.lock` | 8.3 | Low | High |
| 8 | Bind edge IN-lists as `= ANY($1::text[])` | 4.6 | Low | High |
| 9 | `CREATE INDEX CONCURRENTLY` for all runtime and reconcile builds | 4.4 | Low | High |
| 10 | Statement timeout on queue metrics; real worker count | 5.4 | Low | High |
| 11 | Default storage mode to `halfvec` | 9.3 | Low | High |
| 12 | Full-entropy workspace table identifiers with collision check | 7.1 | Low | High |
| 13 | Rewrite `claim_next` to a bounded sample plus two sargable arms | 5.1, 5.2 | Medium | High |
| 14 | Backfill denorm columns, then drop the JSONB OR arms from deletes | 4.5 | Medium | High |
| 15 | Split mutable task fields out of the `payload` JSONB | 3.4 | Medium | High |
| 16 | Keyset pagination and estimated counts for task listing | 5.4 | Medium | High |
| 17 | Partition `tasks` by month with retention | 5.3 | Medium | High |
| 18 | Move migration and reconcile into a CLI subcommand; boot verifies only | 8.2 | Medium | High |
| 19 | Delete the deprecated RLS surface; parameterize `where_clause` | 7.2 | Medium | High |
| 20 | Global and partial HNSW mutually exclusive, or hash partitioning | 9.2 | Medium | Medium |
| 21 | Declared index manifest with startup drift reconciliation | 9.1 | Medium | High |
| 22 | PDF binaries to object storage or a side table | 6 | Medium | High |
| 23 | Separate pools for query, ingest, and queue, each with its own timeout | 1 | Medium | Medium |
| 24 | Falsifiable scaling harness wired into release gates | 12 | Medium | High |
| 25 | Read replicas for retrieval, primary reserved for ingestion | 2 | High | Medium |

Item 25 should not be attempted before items 1 through 8. Scaling out a system whose ingestion serializes on a single tuple merely buys more machines to wait on that tuple.

## 12. Measurement before further action

Six numbers falsify or confirm the highest-value claims above. None require code changes to obtain.

1. `pg_stat_all_tables` for the `*_vectors_stats` tables: `n_tup_upd` versus `n_live_tup`, and `n_dead_tup`. A dead-tuple count in the same order as total inserts confirms 3.1 directly.
2. `pg_stat_activity` sampled during ingestion, counting sessions in `Lock` wait state on the stats relation. Confirms the serialization rather than just the bloat.
3. `pg_settings` for `statement_timeout` and `maintenance_work_mem` read from a pooled application connection immediately after any workspace initialization. Non-default values confirm 4.3.
4. `pg_stat_statements` ordered by `total_exec_time` for the `claim_next` query text, with its `rows` versus `calls` ratio. High time-per-row confirms 5.1.
5. Recall@10 for filtered ANN with exact reorder on versus off, same dataset and same `ef_search`. Quantifies the ranking cost of the current default in 4.2.
6. HNSW index size summed against `shared_buffers`, plus `pg_stat_io` read counts on the vector table. Determines whether the index is resident or thrashing, which decides between item 11 and the binary quantization path.

The README claims sub-200ms hybrid queries and 1000 or more concurrent users with no corpus size, hardware specification, or recall figure. Latency without a recall number is not a performance result, because recall can always be traded for speed. For a project at roughly two thousand stars this is a credibility exposure as much as an engineering one, and item 24 resolves it.

## 13. What the codebase gets right

Worth recording, because an audit that only lists defects misrepresents the system. The single-pool discipline via `from_existing` is deliberate and documented. `AnnIndexPolicy::resolve` is a genuine single source of truth for a genuinely messy dimension-versus-index-type matrix. The native BFS in `expand.rs` with hard caps on depth and fan-out is the correct answer to graph traversal cost and replaces variable-length Cypher entirely. The batch upsert's `UNNEST` design keeps bind parameters constant at three, sidestepping the 65535 parameter cap, and `RETURNING (xmax = 0)` is an elegant insert-versus-update discriminator. `with_rls_transaction` correctly diagnoses and fences a silent-deny failure mode that most projects ship broken. The checksum repair path fails loud in production and proves it with a self-inspecting contract test.

The `@dataop` annotation blocks carrying intent, complexity, limits, and per-major Postgres support are a genuinely good practice. The fact that finding 5.1 could be written by comparing an annotation against its own implementation is evidence the practice is working, not that it is failing.

## 14. Sovereignty notes

EdgeQuake is **Hammer**, not House: a reusable engine and framework. Ownership belongs in the Factory (Estonian OÜ), held personally under the Trust Declaration until incorporation, then assigned at nominal value without delay.

**License posture.** Apache-2.0 and public. That is a deliberate distribution and credibility asset, but it means no exclusivity can be granted to any client, and any engagement embedding EdgeQuake must carry the Background IP clause so client integrations stay House while the engine stays Hammer.

**Entity boundary.** Findings 3.1, 4.1, 5.1, 6, and 7.1 are defects a paying client would plausibly encounter and ask to have fixed. Fixing them improves the core engine, so that work is Factory asset creation and must not be delivered inside an Elitizon deliverable without a prior carve-out. Route it through Shift 1, funded by revenue, owned by the Factory.

**Disclosure flag.** Finding 7.1 is a cross-tenant isolation defect in a public repository. It bites only above roughly 9,000 workspaces, so there is time, but it should be fixed quietly and released before it is described publicly in any detail.

---

*Grounded against `connection.rs`, `config.rs`, `capabilities.rs`, `hnsw_runtime_policy.rs`, `ann_exact_reorder_policy.rs`, `binary_quantize_policy.rs`, `diskann_runtime_policy.rs`, `workspace_vector.rs`, `row_count_stats.rs`, `rls.rs`, `pdf_list_query.rs`, `vector/ddl.rs`, `vector/search_tuning.rs`, `vector/storage_impl.rs`, `graph/query_ops/expand.rs`, `edgequake-tasks/src/postgres.rs`, `edgequake-tasks/src/queue.rs`, `migration_bootstrap/helpers.rs`, `reconcile/{mod,m071,m078}.rs`, `migrations/NOTES.md`, `migrations/support/092/apply.sql`, and migrations 036, 071, 073, 081, 096, 098. Not yet audited: `kv.rs`, `conversation.rs`, `pdf_storage_impl.rs`, `worker.rs`, `migration_bootstrap/mod.rs`.*