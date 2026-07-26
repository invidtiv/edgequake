# SPEC-090 — Lenses

Each lens cites laws from [00-first-principles.md](../00-first-principles.md) and findings from [01-finding-register.md](../01-finding-register.md).

| Lens | File | Primary question |
|------|------|------------------|
| Postgres Expert | [LENS-postgres.md](LENS-postgres.md) | Locks, dead tuples, EXPLAIN, CIC, plan cache |
| Full Stack | [LENS-fullstack.md](LENS-fullstack.md) | Request → pool → txn → ANN/queue coupling |
| Product Owner | [LENS-product-owner.md](LENS-product-owner.md) | Honest latency+recall; what “done” means |
| SRE / Ops | [LENS-sre-ops.md](LENS-sre-ops.md) | Boot, checksums, timeouts, retention |
| Security / Tenancy | [LENS-security-tenancy.md](LENS-security-tenancy.md) | Isolation, RLS Drop, AGE fail-closed |
| Marketing / Credibility | [LENS-marketing-credibility.md](LENS-marketing-credibility.md) | Falsifiable harness vs hype numbers |
