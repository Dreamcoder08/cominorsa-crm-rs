# COMINORSA CRM

Local desktop CRM written in Rust. It registers leads, moves them through the sales pipeline, searches them, shows an agenda of follow-ups, case steps, and collections, and keeps a searchable prospect list that converts into leads. Advancing a lead to "Contrato firmado" opens one case in "Expedientes", with procedure steps you mark done and date. Each step date is a suggestion to adjust, labeled "Fecha sugerida, ajústala". A service that names several procedures, such as "IGAFOM + REINFO", combines their steps in the order of the text. Each case tracks its collections: contract total, billing milestones with an optional suggested 30 / 40 / 30 plan, and collected and pending amounts in soles. Unpaid milestones appear in the agenda as "Cobro", and the case warns when milestones do not add up to the contract total. A lead card opens "Cotizar": numbered quotes such as COT-2026-0001 with lines, IGV, and a preview of totals, saved as a PDF to a file you choose. A sent quote never changes; changing it means a new version, such as COT-2026-0001-V2. Marking a quote accepted makes its total the suggested contract total when the case opens. Closing a case is an explicit action with confirmation; a recurring service creates one renewal lead in the first stage, marked "Cliente existente", and sending any message stays a human action. Lead cards open a prepared WhatsApp chat that a person reviews and sends. Each lead can have a responsible person from the team list. Settings cover the theme, team, default follow-up days, the company data and IGV rate for quotes, the WhatsApp message with a preview, and backup and restore. Valid changes save automatically and report their state.

## Quick path

Requires Rust 1.99.1, pinned in `rust-toolchain.toml`; rustup installs it on first build. On Linux, also install WebKitGTK 4.1.

```sh
cargo test --workspace
cargo run --release -p cominorsa-desktop
```

Data is stored in the operating system's application data folder as `crm.db`. Set `COMINORSA_DB_PATH` to use another file.

## Windows

The `Windows build` workflow runs the tests and publishes `cominorsa-crm-windows-x64.zip` as a workflow artifact. The package holds `cominorsa-desktop.exe`, `cominorsa-import.exe`, and the `cominorsa.exe` CLI, and never customer data.

Data lives in `%APPDATA%\COMINORSA\CRM\data\crm.db`.

Cross-compile from Linux with `zig`, `cargo-zigbuild`, and `llvm-windres`:

```sh
RC=llvm-windres RC_x86_64_pc_windows_gnu=llvm-windres cargo zigbuild --release --locked --target x86_64-pc-windows-gnu -p cominorsa-desktop -p cominorsa-importer
```

## CLI

`cominorsa` runs every business flow without a window, through the same application layer as the desktop app, so an agent can test behavior and state from a terminal. It sends no messages: `lead whatsapp` only prints a prepared URL.

```sh
cargo run -p cominorsa-cli -- --help
cargo run -p cominorsa-cli -- --db <crm.db> init
cargo run -p cominorsa-cli -- --db <crm.db> --json status
scripts/e2e-cli.sh
```

| Group | Commands |
|---|---|
| State | `init`, `status`, `export`, `agenda`, `migrate` |
| `lead` | `add`, `list`, `show`, `summary`, `advance`, `assign`, `whatsapp` |
| `prospect` | `import`, `enrich`, `search`, `count`, `convert` |
| `case` | `list`, `show`, `close` |
| `step` | `done`, `undo`, `date` |
| `billing` | `total`, `plan`, `add`, `edit`, `pay`, `unpay`, `delete` |
| `quote` | `create`, `edit`, `list`, `show`, `preview`, `send`, `accept`, `reject`, `version`, `pdf` |
| `settings` | `show`, `set`, `whatsapp-preview` |
| `backup` | `create`, `restore` |

The database path is mandatory, through `--db` or `COMINORSA_DB_PATH`; the CLI has no default location, so it cannot reach the app's real data by accident.

The CLI never creates or upgrades a database by itself. `init` creates a new one and refuses any existing file with `database_exists`. Every other command checks the file before it validates its own arguments, and a refused file stays byte-identical:

| Database | Result |
|---|---|
| Missing | `database_missing`; nothing is created |
| Older schema | `schema_outdated`, naming both versions and `cominorsa migrate` |
| Newer schema | `schema_newer`; the journal mode does not change either |

`migrate` is the only command that upgrades. It copies the database beside itself as `*.antes-de-migrar-*.db`, migrates a working copy, and replaces the database with a rename. If a migration fails, it reports `migration_failed`, the database stays as it was, and the copy stays beside it. Close the app before migrating. `backup restore` is the one other path that changes the schema: an older backup is brought to the current schema before it replaces the database, and the backup file itself does not change.

A command that only reads opens the file read-only. SQLite may leave an empty `-wal` and a `-shm` file beside the database after a read or a rejected write; the database file itself does not change. A database in the rollback journal is the exception, listed under [Current limits](#current-limits).

The desktop app has no upgrade of its own: at startup it creates a missing database and sends an older one through the same path as `migrate`. If that fails, the app shows the cause and where the copy is, works on no half-upgraded file, and the database stays as it was. A newer schema is refused the same way. Each failed launch keeps its own copy.

| Exit code | Meaning |
|---|---|
| 0 | Success |
| 1 | Business rejection: the data did not change |
| 2 | Usage error |
| 3 | Storage failure |

Output is Spanish for people. With `--json`, stdout holds one document on success and stderr holds one on failure, and the other stream stays empty:

```json
{"data": {"lead": {"id": "...", "stage": "initial_contact"}}}
{"error": {"code": "final_stage", "message": "El lead ya está en la etapa final"}}
```

| Exit | Error codes |
|---|---|
| 1 | Records: `lead_not_found`, `case_not_found`, `case_not_active`, `step_not_found`, `milestone_not_found`, `quote_not_found`, `prospect_not_found`, `prospect_ambiguous`, `member_not_found` |
| 1 | Values: `missing_field`, `invalid_ruc`, `invalid_date`, `invalid_amount`, `invalid_quantity`, `invalid_igv_rate`, `invalid_follow_up_days`, `invalid_quote_validity`, `invalid_theme`, `invalid_kind`, `unknown_placeholder`, `empty_quote`, `no_whatsapp_number` |
| 1 | Rules: `final_stage`, `contract_not_signed`, `owner_not_in_team`, `duplicate_member`, `missing_contract_total`, `milestones_already_planned`, `quote_locked`, `quote_superseded`, `invalid_quote_transition`, `company_name_missing`, `backup_folder_missing`, `id_seed_reused` |
| 1 | Files: `database_exists`, `import_invalid`, `import_unreadable`, `pdf_failed`, `pdf_target_exists`, `pdf_target_is_database` |
| 2 | `usage`, `missing_database` |
| 3 | `database_missing`, `schema_outdated`, `schema_newer`, `migration_failed`, `storage_error`, `corrupt_record`, `backup_folder_unavailable`, `pdf_write_failed`, `runtime_error` |

JSON keys and error codes are stable and in English. Dates are ISO 8601, money is integer cents, and enums use their stored codes. Quotes accept their number, such as `COT-2026-0001-V2`, or their id.

For reproducible runs, `--today <AAAA-MM-DD>` fixes the day, `--now <seconds>` fixes the instant, and `--id-seed <text>` derives new ids from the text. A day alone means noon in Peru. A seed names one request, so use a different one per command. Repeating the identical request with its seed changes nothing in `lead add`, `prospect convert`, `quote create`, `quote version`, `billing add`, and `case close`. When a seed derives an id that already belongs to another record of that kind, the command fails with `id_seed_reused` and nothing changes. Without these flags the CLI uses the system clock and the domain's generated ids, like the app. Two runs of the same commands with the same flags give byte-identical exports, as long as stored paths such as the backup folder match.

`quote pdf --out` refuses an existing file unless `--force` is given, and always refuses the database and its `-wal`, `-shm`, and `-journal` files, under any name, including a hard link to them. `settings set --remove-member` refuses a name that is not on the team, and `billing edit` needs at least one field. Only a team member can own a lead: `lead add`, `lead assign`, and `prospect convert` refuse any other name, and a blank `--owner` clears it.

`quote preview --line` returns the totals of some lines at the stored IGV rate, and `settings whatsapp-preview` renders the stored template for a sample client, or a `--template` to try; neither saves anything. `lead show` and each row of `lead list` add `follow_up`, the status for the day: `overdue`, `today`, `upcoming`, or `closed`. Commands that change a quote add `notice`, the sentence the app shows.

`export` returns `schema_version`, `leads`, `cases`, `quotes`, `quote_counters`, `prospects`, and `settings` in a stable order, including closed cases with nothing left to collect. `status` returns `schema_version`, `integrity_check`, and `counts`.

## App icon

`crates/desktop/assets` holds the icon, redrawn from the COMINORSA logo. The window and sidebar embed it, `build.rs` adds `icon.ico` to the Windows executable, and `scripts/install-linux-launcher.sh` adds the Linux launcher.

## Quotes

Totals use integer cents. Unit prices exclude IGV. A line is quantity times unit price, and IGV is charged once on the subtotal, rounding half a cent up. Each year numbers restart at 0001; the number comes from a counter inside the transaction that saves the quote, so a failed save gives it back.

A draft PDF says "Borrador" and uses the current company data. Marking a quote as sent freezes that data on it. A blank company field is left out of the document, and the app asks before sending a quote without a company name. Every page states that the quote is not a tax receipt or invoice; the app issues no SUNAT documents.

`crates/application` embeds, from `crates/desktop/assets/fonts`, Noto Sans Regular and Bold under the SIL Open Font License, in `OFL.txt` beside them. PDF fonts are subset, so a one-page quote weighs about 23 KB.

## Import prospects

Keep source files outside this repository. The importer follows the CLI contract: it never creates or upgrades a database, so create one first, by opening the app once or with `init`.

```sh
cargo run -p cominorsa-cli -- --db <crm.db> init
cargo run --release -p cominorsa-importer -- --db <crm.db> <companies.csv> <people.csv>
```

The importer validates its files first and exits with 1 when they are invalid, whatever the state of the database. It then refuses a missing database, naming `cominorsa init`, and an older or newer schema, with the messages of `cominorsa` and exit code 3, and the file stays byte-identical. Upgrade an older database with `cominorsa migrate`, or by opening the app. The Windows package does not include `cominorsa`, so there the app does both. A usable database is copied next to itself as `crm.before-import-*.db`, and titleholders already loaded are skipped. Errors name lines and columns, never values. Close the app before importing.

## Design system

Tokens live at the top of `crates/desktop/src/style.css` and use `light-dark()`, so each color is declared once for both themes. `crates/desktop/tests/contrast.rs` fails when text drops below 4.5:1 or a control or state indicator drops below 3:1. Layout has three modes: full sidebar, icon rail at 1080 px, and bottom navigation at 720 px.

Enrich company prospects with a prepared SUNAT file, kept outside this repository:

```sh
cargo run --release -p cominorsa-importer -- --db <crm.db> --sunat <companies-sunat.csv>
```

It needs an existing database at the current schema, like the import. The file comes from the public reduced RUC roll, which has no phone or email. Approximate matches show "Confirmar RUC" and never prefill a lead.

## Structure

| Crate | Responsibility | May depend on |
|---|---|---|
| `crates/domain` | Business rules and the repository port | No I/O or UI crates |
| `crates/storage` | SQLite adapter and migrations | `domain` |
| `crates/importer` | `cominorsa-import` CLI for prospect CSV files | `domain`, `storage` |
| `crates/application` | Use cases, display rules, and the quote PDF, free of UI types | `domain`, `storage` |
| `crates/cli` | `cominorsa` CLI: argument parsing and JSON shapes | `application`, `domain`, `importer`, `storage` |
| `crates/desktop` | Dioxus desktop UI and startup | `application`, `domain`, `storage` |

Dioxus stays inside `crates/desktop` because it is pre-1.0. A future server adapter can reuse `domain` and replace SQLite behind `LeadRepository` and `CaseRepository`. Only the application's backup and inspection code, the importer, and the desktop name `SqliteLeads`: the desktop opens it at startup and passes it to the use cases.

## Current limits

| Topic | Status |
|---|---|
| Leads signed before cases existed | No case is created for them; only "Avanzar" opens one |
| RUC validation | Offline format and check digit only; it does not prove the RUC exists at SUNAT |
| Pipeline stages and lead fields | Assumptions pending client validation |
| Case steps | Titles and day offsets are planning suggestions pending client validation; legal deadlines are never stored in code |
| Quotes | Not tax documents. The 18% IGV default, the 15-day suggested validity, and whole-number quantities are unvalidated assumptions; marking a quote sent does not move the lead's stage |
| Billing | Tracks collections only; it does not issue tax invoices or receipts. The 30 / 40 / 30 split comes from an unvalidated example |
| Renewals | Only DAC suggests a renewal, 12 months after closing, pending client validation; a service naming DAC with others renews as a whole. A closed case with unpaid milestones stays listed until they are paid |
| Follow-up alerts | Visible only while the app is open |
| Leads from the website or WhatsApp | Registered manually; there is no automatic intake |
| Prospects | Concession titleholders without phone, RUC, or email; potential service is inferred, not verified |
| Backups | Manual to a user-chosen folder, automatic before an import, and verified restore from Settings; no scheduled backup yet |
| Disk encryption | Not provided by the app; enable it on the laptop |
| Rollback-journal databases | Every write opens in WAL mode, so a rejected write on a database in the rollback journal keeps its rows but changes two header bytes. `init`, an upgrade, and a restore all leave WAL, so this needs a file altered outside the app |
| Hard links on Windows | Stable Rust has no file index there, so a PDF target is matched to the database by size, creation, and modification time; another file matching all three is refused too |
| Windows | Cross-compiled with `cargo zigbuild --target x86_64-pc-windows-gnu`; not run on Windows yet; requires the WebView2 runtime. The `.github/workflows/windows.yml` build needs an active GitHub billing state |

## Measured baseline

Measured on Linux with WebKitGTK and an empty database. The memory figure predates the case, billing, and quote features: 20.2 MB release binary, including embedded fonts and about 213 MB total PSS at idle (app 96 MB, web process 92 MB, network process 25 MB). Windows uses WebView2, so measure it separately.

## Rules

- Never commit customer data, credentials, or real exports. Fixtures are synthetic.
- Write a failing test before each behavior change.
- Run `scripts/check.sh` before committing. It runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the workspace tests (with cargo-nextest when installed), and `scripts/e2e-cli.sh`, which needs `jq` and `sqlite3` and fails without them. `scripts/check.sh --full` also runs `cargo deny check` and reports the release binary size; it exits with status 2 when cargo-deny is missing.
