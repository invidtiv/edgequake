# LENS — Product Owner (SPEC-090)

## “Done” at scale

- Concurrent ingest uses the pool (not a single lock).  
- Hybrid query latency published **with** corpus size, hardware, and recall@k.  
- Workspace isolation remains fail-closed (no short-id roulette).  
- Operators can upgrade without unbounded boot reconcile.

## Non-goals this wave

- Marketing claims of “1000 concurrent users” without harness (F-090-29).  
- Public write-up of short-id collision before fix ships.
