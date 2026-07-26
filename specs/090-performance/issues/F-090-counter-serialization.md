# Issue study — F-090-01 Counter serialization

## Symptom

Concurrent vector upserts serialize; stats tables show `n_live_tup=1` and huge `n_tup_upd`.

## Mechanism

`row_count_stats.rs` creates `FOR EACH ROW` triggers that `UPDATE … WHERE id = 1`.

## Fix

`FOR EACH STATEMENT` + `REFERENCING NEW/OLD TABLE` + `row_count ± COUNT(*)`.

## Measure

See M-3.1 in [06-measurement-protocol.md](../06-measurement-protocol.md).

## Test

`e2e_spec090_counter_statement_trigger`
