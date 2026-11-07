# Storage hardening

Objective: close the four technical follow-ups left after PR #1.
Branch: `fix/storage-hardening` from `main` at 6ebfa62. Delivery strategy: `single-pr`. Test runner: `scripts/check.sh --full` and `scripts/e2e-cli.sh`.

## Specs

- S1. `migrate`, desktop startup upgrade, and importer upgrade never race a concurrent writer: when another connection holds or takes the database, the upgrade refuses with a machine-readable code, the original stays byte-identical, and no write made by the other connection is lost.
- S2. The upgrade working copy uses a name that cannot collide with an existing user file; an existing file with the old fixed name is never overwritten or deleted.
- S3. One corrupt consent row no longer fails every lead read: that lead loads as without consent, other leads load normally, and `status` reports how many rows were unreadable.
- S4. A rejected write leaves the database file byte-identical in every journal mode, including rollback-journal databases; only `init` and `migrate` may set the journal mode.

## Tasks

- [x] T1. S1-S4. Route: delegated, context backstop. Risk: item 1. Commit: 8fa05e2.

## Log

- L1 (2026-11-07, user answer, verbatim): "yes continue", to the question "¿Sigo con esos cuatro pendientes técnicos, o esperamos primero la prueba en la laptop?". The four items come from native review advisories and the T19 limit recorded in `odd/tasks/lead-pipeline-slice.md`, L46 and L48.
- L2 (2026-11-07): T1 done in 8fa05e2. RED observed for 7 new CLI tests in `crates/cli/tests/storage_hardening.rs` (busy migrate succeeded, "Carrera 0 was written and then lost", user file `old.migrating.db` deleted, `lead list` exit 3 on a corrupt consent, rollback-journal header changed by a rejected write) and for the storage consent test. S1: `upgrade` opens a `locking_mode = EXCLUSIVE`, zero-wait connection, takes `BEGIN EXCLUSIVE`, makes the safety copy with `VACUUM INTO` under it, migrates the working copy, switches the old file to `journal_mode = DELETE`, and renames while the lock is held (Linux, macOS): no window. A connection waiting on the lock then gets `SQLITE_READONLY_DBMOVED`; without the switch, a probe showed it appending to the new file's WAL. Windows releases the lock first, moves the old file aside (fails while open) and compares its digest; a change puts it back and answers `database_busy`. Remaining Windows window: the path is missing for about 1.1-1.3 ms (measured on Linux, small database), where `init` or a first app launch could create an empty database and migrate then fails with the old data aside. S2: `*.migrando-<pid>-<n>.db` via `create_new`. S3: storage tolerates an unreadable consent per row; `unreadable_rows.lead_consents` in `status`, `export`, and MCP `status`. S4: `open_existing` no longer sets WAL; the storage skill rule was updated. `scripts/check.sh --full`: 607 tests passed, e2e 523 checks, deny passed, exit 0.
