# Issue study — F-090-17 workspace short-id

## Symptom

Dedicated vector tables named `eq_{ns}_ws_{8hex}_vectors`.

## Risk

Birthday collision → shared table → cross-tenant mixing.

## Fix

Full UUID with hyphens→underscores + uniqueness check; quiet migration of existing tables.

## Disclosure

Fix before public detailed write-up.
