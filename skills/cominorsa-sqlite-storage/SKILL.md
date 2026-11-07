---
name: cominorsa-sqlite-storage
description: "Trigger: database, migration, SQLite, sqlx, schema, query, backup, storage. Change COMINORSA persistence safely with sqlx 0.9 and SQLite."
license: Apache-2.0
metadata:
  author: "dreamcoder08"
  version: "1.1"
---

## Activation Contract

Load for any change in `crates/storage`, its migrations, or the database file location.

## Hard Rules

- Migrations are append-only: never edit or delete a committed file in `crates/storage/migrations`.
- Every migration is high risk (stored customer data): it gets its own commit and a test that opens a database at the previous schema with synthetic rows and checks they survive.
- Bind parameters for every value; never build SQL with `format!`.
- Only `init` and `migrate` set WAL mode (through `SqliteLeads::open`); every other open keeps the journal mode it finds, because changing it rewrites the file header, so a rejected write would still change the file. Keep a single writer connection unless a measured need says otherwise.
- An upgrade holds an exclusive lock (`locking_mode = EXCLUSIVE`, no busy wait) from the safety copy to the swap and refuses a busy database; leave WAL on the old file before renaming over it, so a waiting connection gets `SQLITE_READONLY_DBMOVED` instead of writing into the new file's WAL.
- Working copies beside the database use a new name created with `create_new`; never remove a fixed name that a user file could hold.
- Rehydration tolerates one unreadable optional value per row, as with `last_backup_at` and consent, and counts it for `status`; it never echoes the value.
- A write never overwrites a stored value it could not read: the lead upsert keeps the consent columns when the lead carries no consent. Only an explicit record or revoke replaces them.
- Working, aside, and safety-copy files all come from `new_file_beside`. While a `*.aparte-*` file exists and the database path is missing, `open` refuses to create a database (`AsideFiles`).
- `crates/storage` maps rows to domain types and back; no business rules here.
- Tests use a temporary file or `:memory:`, never the user's real `crm.db`.
- Install sqlx-cli without `--locked` (the 0.9 release ships no lockfile).
- Backups and encryption are required before real customer data; do not claim either exists.

## Decision Gates

| Change | Requirement |
|---|---|
| New column | Additive migration with a default or nullable; domain test plus storage round-trip test |
| Rename or drop | Stop and ask the user; plan copy, verify, then drop |
| Slow query | `EXPLAIN QUERY PLAN`; add an index through a migration |
| Backup feature | Use SQLite online backup or `VACUUM INTO`; test restore, not only backup |

## Execution Steps

1. Write the failing storage test in `crates/storage/tests`.
2. Add the migration with the next number; never reuse one.
3. Update the adapter mapping.
4. Run `scripts/check.sh`; then run the app once against a copy of an older synthetic database.

## Output Contract

Report migrations added, data-preservation proof, and `Risk: item 1 (stored data)` for any schema change.

## References

- `../../crates/storage/tests/sqlite_leads.rs` — adapter test patterns.
