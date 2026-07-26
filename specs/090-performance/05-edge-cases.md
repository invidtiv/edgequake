# SPEC-090 — Edge Cases

| ID | Case | Expected behavior |
|----|------|-------------------|
| EC-01 | Empty vector table ANN create | Blocking `CREATE INDEX` allowed (cheap); CIC optional |
| EC-02 | CIC interrupted → `INVALID` index | Drop INVALID and retry concurrent build |
| EC-03 | Partial upsert after mid-document failure | Retry converges via `ON CONFLICT DO UPDATE` |
| EC-04 | `EDGEQUAKE_ALLOW_NO_GRAPH=1` | Boot continues; `/health` reports graph unavailable |
| EC-05 | Workspace short-id collision (legacy) | Migrate to full UUID; fail-closed on new collision |
| EC-06 | `DISCARD ALL` vs prepared statements | Accept reset cost; correctness > micro-opt |
| EC-07 | Claim sample empty for fair workspace | Fall through to next workspace / stale arm |
| EC-08 | PDF list with null markdown | Metadata row still returns; content via by-id |
| EC-09 | Reorder with `top_k=0` / tiny candidate_k | Clamp `candidate_k >= top_k` |
| EC-10 | Stats trigger on TRUNCATE | Recreate/reset stats row; count self-heals |
