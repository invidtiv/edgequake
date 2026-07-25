# LENS — Front Design (SPEC-089)

## Design constraint

**No new chrome** for this fix. Existing Documents list density and status chips stay.

## Honesty

- Zero entity count on a completed doc is allowed briefly (stale KV) — prefer truth over blocking.  
- Do not invent decorative badges, pills, or overlay stickers for reconcile state.

## Motion / layout

Unchanged. Backend latency budget is the design deliverable: page paints without fleet-wide stall.
