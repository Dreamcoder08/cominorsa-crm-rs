# COMINORSA CRM

Local desktop CRM written in Rust. It registers leads, moves them through the sales pipeline, searches them, shows an agenda of follow-ups, case steps, and collections, and keeps a searchable prospect list that converts into leads. Advancing a lead to "Contrato firmado" opens one case in "Expedientes", with procedure steps you mark done and date. Each step date is a suggestion to adjust, labeled "Fecha sugerida, ajústala". A service that names several procedures, such as "IGAFOM + REINFO", combines their steps in the order of the text. Each case tracks its collections: contract total, billing milestones with an optional suggested 30 / 40 / 30 plan, and collected and pending amounts in soles. Unpaid milestones appear in the agenda as "Cobro". Closing a case is an explicit action with confirmation; a recurring service creates one renewal lead in the first stage, marked "Cliente existente", and sending any message stays a human action. Lead cards open a prepared WhatsApp chat that a person reviews and sends. Each lead can have a responsible person from the team list. Settings cover the theme, team, default follow-up days, the WhatsApp message with a preview, and backup and restore. Valid changes save automatically and report their state.

## Quick path

Requires Rust 1.99.1, pinned in `rust-toolchain.toml`; rustup installs it on first build. On Linux, also install WebKitGTK 4.1.

```sh
cargo test --workspace
cargo run --release -p cominorsa-desktop
```

Data is stored in the operating system's application data folder as `crm.db`. Set `COMINORSA_DB_PATH` to use another file.

## Windows

The `Windows build` workflow runs the tests and publishes `cominorsa-crm-windows-x64.zip` as a workflow artifact. The package holds `cominorsa-desktop.exe` and `cominorsa-import.exe`, and never customer data.

Data lives in `%APPDATA%\COMINORSA\CRM\data\crm.db`.

Cross-compile from Linux with `zig`, `cargo-zigbuild`, and `llvm-windres`:

```sh
RC=llvm-windres RC_x86_64_pc_windows_gnu=llvm-windres cargo zigbuild --release --locked --target x86_64-pc-windows-gnu -p cominorsa-desktop -p cominorsa-importer
```

## App icon

`crates/desktop/assets` holds the icon, redrawn from the COMINORSA logo. The window and sidebar embed it, `build.rs` adds `icon.ico` to the Windows executable, and `scripts/install-linux-launcher.sh` adds the Linux launcher.

## Import prospects

Keep source files outside this repository.

```sh
cargo run --release -p cominorsa-importer -- --db <crm.db> <companies.csv> <people.csv>
```

The importer validates both files first, copies an existing database next to it as `crm.before-import-*.db`, and skips titleholders already loaded. Errors name lines and columns, never values. Close the app before importing.

## Design system

Tokens live at the top of `crates/desktop/src/style.css` and use `light-dark()`, so each color is declared once for both themes. `crates/desktop/tests/contrast.rs` fails when text drops below 4.5:1 or a control or state indicator drops below 3:1. Layout has three modes: full sidebar, icon rail at 1080 px, and bottom navigation at 720 px.

Enrich company prospects with a prepared SUNAT file, kept outside this repository:

```sh
cargo run --release -p cominorsa-importer -- --db <crm.db> --sunat <companies-sunat.csv>
```

The file comes from the public reduced RUC roll, which has no phone or email. Approximate matches show "Confirmar RUC" and never prefill a lead.

## Structure

| Crate | Responsibility | May depend on |
|---|---|---|
| `crates/domain` | Business rules and the repository port | No I/O or UI crates |
| `crates/storage` | SQLite adapter and migrations | `domain` |
| `crates/importer` | `cominorsa-import` CLI for prospect CSV files | `domain`, `storage` |
| `crates/desktop` | Dioxus desktop UI and startup | `domain`, `storage` |

Dioxus stays inside `crates/desktop` because it is pre-1.0. A future server adapter can reuse `domain` and replace SQLite behind `LeadRepository` and `CaseRepository`.

## Current limits

| Topic | Status |
|---|---|
| Leads signed before cases existed | No case is created for them; only "Avanzar" opens one |
| RUC validation | Offline format and check digit only; it does not prove the RUC exists at SUNAT |
| Pipeline stages and lead fields | Assumptions pending client validation |
| Case steps | Titles and day offsets are planning suggestions pending client validation; legal deadlines are never stored in code |
| Billing | Tracks collections only; it does not issue tax invoices or receipts. The 30 / 40 / 30 split comes from an unvalidated example |
| Renewals | Only DAC suggests a renewal, 12 months after closing, pending client validation; a service naming DAC with others renews as a whole. A closed case with unpaid milestones stays listed until they are paid |
| Follow-up alerts | Visible only while the app is open |
| Leads from the website or WhatsApp | Registered manually; there is no automatic intake |
| Prospects | Concession titleholders without phone, RUC, or email; potential service is inferred, not verified |
| Backups | Manual to a user-chosen folder, automatic before an import, and verified restore from Settings; no scheduled backup yet |
| Disk encryption | Not provided by the app; enable it on the laptop |
| Windows | Cross-compiled with `cargo zigbuild --target x86_64-pc-windows-gnu`; not run on Windows yet; requires the WebView2 runtime. The `.github/workflows/windows.yml` build needs an active GitHub billing state |

## Measured baseline

Measured on Linux with WebKitGTK and an empty database: 10.4 MB release binary and about 213 MB total PSS at idle (app 96 MB, web process 92 MB, network process 25 MB). Windows uses WebView2, so measure it separately.

## Rules

- Never commit customer data, credentials, or real exports. Fixtures are synthetic.
- Write a failing test before each behavior change.
- Run `scripts/check.sh` before committing. It runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the workspace tests (with cargo-nextest when installed). `scripts/check.sh --full` also runs `cargo deny check` and reports the release binary size; it exits with status 2 when cargo-deny is missing.
