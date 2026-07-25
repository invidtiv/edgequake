# LENS — Full Stack (SPEC-089)

```
WebUI Documents poll
    → GET /api/v1/documents?page=&page_size=
        → merge KV + relational
        → filter / status_counts
        → paginate_vec                    ← page boundary
        → reconcile_entity_counts (PAGE)  ← LAW-H1
            → graph.node_counts_*(≤32×probe)
                → txn + SET LOCAL 300ms
                → GIN @> on "Node"
        → JSON page

Docker /health
    → get_statistics (tasks COUNT, 750ms)
    → must succeed when list is active     ← LAW-H3
```

## Breakpoints fixed

| Layer | Before | After |
|-------|--------|-------|
| API list | Reconcile full corpus | Page only |
| Storage | Unbounded probes, no PG timeout | Batch + `statement_timeout` |
| Pool | Zombie 2+ min holds | ≤300ms cancel |
| Health | False failure | Stable |

## Laws

H1–H5.
