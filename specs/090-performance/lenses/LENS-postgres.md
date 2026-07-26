# LENS — Postgres Expert (SPEC-090)

## Hot tuple law (LAW-P2)

```
stats row id=1  →  FOR EACH ROW UPDATE  →  exclusive row lock per insert
batch INSERT 1000 → 1000 sequential updates inside one statement
```

Fix: `FOR EACH STATEMENT` + `REFERENCING NEW TABLE` → one `UPDATE … row_count + COUNT(*)`.

## Session hygiene (LAW-P4)

- Search path: `SET LOCAL` inside txn, or `after_release` → `DISCARD ALL`
- Never leave `statement_timeout=0` / inflated `maintenance_work_mem` on pool conns
- CIC cannot run inside a transaction block

## Plan cache

- Prefer `= ANY($1::text[])` over interpolated `IN (...)`
- Prefer bound `LIMIT` over format!-interpolated OFFSET strings

## EXPLAIN contracts

| Path | Must show |
|------|-----------|
| claim pending arm | Index on `(workspace_id, created_at) WHERE status='pending'` |
| PDF list | No TOAST fetch of `pdf_data` |
| delete_by_document (after fix) | Index/Bitmap, not Seq Scan on large tables |
