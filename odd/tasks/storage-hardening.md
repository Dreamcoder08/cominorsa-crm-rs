# Storage hardening

Objective: close the four technical follow-ups left after PR #1.
Branch: `fix/storage-hardening` from `main` at 6ebfa62. Delivery strategy: `single-pr`. Test runner: `scripts/check.sh --full` and `scripts/e2e-cli.sh`.

## Specs

- S1. `migrate`, desktop startup upgrade, and importer upgrade never race a concurrent writer: when another connection holds or takes the database, the upgrade refuses with a machine-readable code, the original stays byte-identical, and no write made by the other connection is lost.
- S2. The upgrade working copy uses a name that cannot collide with an existing user file; an existing file with the old fixed name is never overwritten or deleted.
- S3. One corrupt consent row no longer fails every lead read: that lead loads as without consent, other leads load normally, and `status` reports how many rows were unreadable.
- S4. A rejected write leaves the database file byte-identical in every journal mode, including rollback-journal databases; only `init` and `migrate` may set the journal mode.

## Tasks

- [ ] T1. S1-S4. Route: delegated, context backstop. Risk: item 1. Commit: pending.

## Log

- L1 (2026-11-07, user answer, verbatim): "yes continue", to the question "¿Sigo con esos cuatro pendientes técnicos, o esperamos primero la prueba en la laptop?". The four items come from native review advisories and the T19 limit recorded in `odd/tasks/lead-pipeline-slice.md`, L46 and L48.
