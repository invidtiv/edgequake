# LENS — UX / UI (SPEC-089)

## User-visible behavior

- Documents table: entity counts on the **current page** heal when AGE answers in budget.  
- If heal times out: keep KV/relational count (often 0) — list still loads.  
- Health indicator / ops dashboards: stop flapping red solely because list reconcile storms the pool.

## Anti-patterns avoided

- No spinner that waits on full-corpus reconcile.  
- No new banner/card for “entity count pending” unless product later asks.  
- No infinite “Processing…” caused by pool starvation (secondary symptom).

## Laws

H1, H3, H5.
