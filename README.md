# COMINORSA CRM

Local desktop CRM written in Rust. It registers leads, moves them through the sales pipeline, searches them, and summarizes which follow-ups are overdue or due today.

## Quick path

Requires Rust 1.98+. On Linux, also install WebKitGTK 4.1.

```sh
cargo test --workspace
cargo run --release -p cominorsa-desktop
```

Data is stored in the operating system's application data folder as `crm.db`. Set `COMINORSA_DB_PATH` to use another file.

## Structure

| Crate | Responsibility | May depend on |
|---|---|---|
| `crates/domain` | Business rules and the repository port | No I/O or UI crates |
| `crates/storage` | SQLite adapter and migrations | `domain` |
| `crates/desktop` | Dioxus desktop UI and startup | `domain`, `storage` |

Dioxus stays inside `crates/desktop` because it is pre-1.0. A future server adapter can reuse `domain` and replace SQLite behind `LeadRepository`.

## Current limits

| Topic | Status |
|---|---|
| RUC validation | Offline format and check digit only; it does not prove the RUC exists at SUNAT |
| Pipeline stages and lead fields | Assumptions pending client validation |
| Follow-up alerts | Visible only while the app is open |
| Leads from the website or WhatsApp | Registered manually; there is no automatic intake |
| Backups and disk encryption | Not implemented; required before real customer data |
| Windows | Not built or tested yet |

## Measured baseline

Measured on Linux with WebKitGTK and an empty database: 10.4 MB release binary and about 213 MB total PSS at idle (app 96 MB, web process 92 MB, network process 25 MB). Windows uses WebView2, so measure it separately.

## Rules

- Never commit customer data, credentials, or real exports. Fixtures are synthetic.
- Write a failing test before each behavior change.
- Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all -- --check` before committing.
