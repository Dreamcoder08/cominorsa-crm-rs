# Agent instructions

Read `README.md` before changing behavior. Feature tracking lives in `odd/tasks/`.

- Keep business rules in `crates/domain`, free of I/O and UI dependencies.
- Keep Dioxus types inside `crates/desktop`.
- Never add customer data, credentials, or real exports to this repository.
- Do not add outreach, WhatsApp sending, or SUNAT emission without explicit authorization for the destination, operation, and credential.
- Run `scripts/check.sh` before committing.

## Project skills

Load the matching skill from `skills/` before work.

| Skill | Load when |
|---|---|
| `cominorsa-quality-gate` | Before any commit or reporting a change as done |
| `cominorsa-self-improve` | End of every task; any action about to run a third time |
| `cominorsa-perf-cache` | Build speed, caching, binary size, memory, or query performance |
| `cominorsa-stack-freshness` | Dependency or toolchain changes; "is this current?" questions |
| `cominorsa-dioxus-desktop` | Changes in `crates/desktop` |
| `cominorsa-sqlite-storage` | Changes in `crates/storage` or migrations |
