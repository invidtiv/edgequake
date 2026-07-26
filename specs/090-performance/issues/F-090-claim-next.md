# Issue study — F-090-11/12 claim_next

## Symptom

Queue claim slows as pending backlog grows; annotation claims `O(W + log N)` but SQL scans all pending.

## Mechanism

Unbounded CTEs + `GROUP BY` over full claimable set; locking branch uses non-sargable OR.

## Fix

Bound sample (oldest ~1000); two sargable `FOR UPDATE SKIP LOCKED LIMIT 1` arms.

## Test

`e2e_spec090_claim_bounded`
