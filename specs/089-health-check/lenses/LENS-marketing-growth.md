# LENS — Marketing & Growth (SPEC-089)

## Trust signal

Self-hosted operators judge EdgeQuake by whether **health stays green at their real corpus size**. A product that dies at ~10k documents loses word-of-mouth and GH stars conversion from “try” → “run in prod”.

## Narrative

- “Built for production Postgres” requires interactive paths that honor pool budgets.  
- Closing GH-336 is a reliability story adjacent to GH-331 (index locality) — same failure class, deeper root cause.

## Metric

Ops retention proxy: continuous green `/health` under Documents UI polling at 5k–10k docs.
