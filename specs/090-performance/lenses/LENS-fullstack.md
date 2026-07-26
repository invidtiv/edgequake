# LENS — Full Stack (SPEC-090)

## Coupling map

```
WebUI Documents list → API → pdf_list_query (blobs?) → TOAST → shared_buffers eviction
WebUI Query → query_filtered → count + ensure_hot_workspace_ann DDL → pool slot held
Ingest worker → upsert one TX → counter locks → claim_next starved
/health → cheap probes → fails when pool full (SPEC-089 class)
```

## Done means

1. Interactive reads never issue DDL.  
2. List pages never download PDF binaries.  
3. Ingest workers do not serialize on one stats row.  
4. Task claim stays fast as backlog grows.  
5. Health remains cheap (inherits SPEC-089).
