# Stack watch

Verified facts only. Each entry has a check date and a source. Replace entries older than 90 days.

| Component | Ours | Latest verified | Checked | Source | Note |
|---|---|---|---|---|---|
| Rust stable | 1.99.1 (pinned in `rust-toolchain.toml`) | 1.99.1 (2026-10-15) | 2026-11-06 | blog.rust-lang.org/2026/10/15/Rust-1.99.1 | 1.99.1 fixes an x86_64 FFI bool miscompilation |
| Rust next | - | 1.100 due 2026-11-12 | 2026-11-06 | Rust 1.100 beta reports | Test binaries move to a new build-dir layout; tests must use `env!("CARGO_BIN_EXE_<name>")`, never `current_exe()`. This repo has no such tests today |
| Dioxus | 0.7 | 0.7.10 stable; 0.8.0-alpha.2 | 2026-11-06 | crates.io via hypedevgroup.pl (2026-10-08), Dioxus merged PRs (2026-10) | Main branch moved to wry 0.57 and tao 0.37; custom protocol handlers need `Send + Sync` in 0.8 |
| sqlx | 0.9 | 0.9.0 (2026-05-21), no patch yet | 2026-11-06 | dev-news.anonyfox.com/sqlx (2026-10-24) | Repo moved to transact-rs/sqlx; `cargo install --locked sqlx-cli` no longer works; per-crate `sqlx.toml` |
| sqlx advisories | - | RUSTSEC-2023-0071 (rsa) in default graph | 2026-11-06 | thecratedigest.com (2026-11-01) | Absent here: `Cargo.lock` has no `rsa` crate with our sqlite-only features (checked 2026-11-06) |
| Advisory sources | cargo-deny 0.20.2 via `scripts/check.sh --full` | RustSec and GitHub databases diverged in 2026 | 2026-11-06 | safeguard.sh (2026-10-18) | Run cargo-deny and Dependabot or OSV |
| Test runner | cargo-nextest 0.9.151 when installed | cargo-nextest recommended for CI | 2026-11-06 | rustify.rs (2026-10-26) | Keep `cargo test --doc` for doctests |
| Linker | rust-lld (default) | mold 3.0, wild 0.11 | 2026-11-06 | mehdi.cz (2026-10-31), efraingaray.com (2026-10-27) | Gain is small for release rebuilds; measure first |
| Transitive unmaintained | - | fxhash (RUSTSEC-2025-0057), paste (RUSTSEC-2024-0436), proc-macro-error (RUSTSEC-2024-0370) | 2026-11-06 | cargo deny check | All via Dioxus 0.7 (selectors, rav1e, gtk3-macros); `deny.toml` checks unmaintained for direct deps only. Recheck on each Dioxus upgrade |
| Dioxus image codecs | image with defaults (pulls rav1e AVIF encoder) | Dioxus main disables image defaults, PNG + ICO only (merged 2026-10-23) | 2026-11-06 | Dioxus merged PRs | Expect smaller build and binary after the next Dioxus release; unreleased today |
| csv | 1 | 1.4.0 (2025-10-17) | 2026-11-06 | crates.io API | Used only by `crates/importer` |
| embed-resource | 3 | 3.0.12 (2026-10-06) | 2026-11-06 | crates.io API | Windows-only build dependency for the app icon |
| krilla (PDF) | 0.8 (0.8.2), `simple-text` only | 0.8.2 (2026-06-04); last commit 2026-11-02 | 2026-11-07 | crates.io API, GitHub API | Chosen over printpdf: 67 crates without raster images, against 132 for printpdf's defaults; subsets fonts, shapes text, writes ToUnicode. MIT or Apache-2.0. No RustSec directory for it (GitHub API 404) |
| printpdf (PDF) | Not used | 0.12.8 (2026-09-05); last commit 2026-10-30 | 2026-11-07 | crates.io API, GitHub API | Newer release and maintained, but default `html` pulls azul-layout; 102 crates with no default features. MIT |
| skrifa | 0.42 (0.42.1, same minor as krilla) | 0.49.0 (2026-10-21) | 2026-11-07 | crates.io API | Direct dependency only to measure text for wrapping and right alignment; krilla exposes no text width. MIT or Apache-2.0. Upgrade together with krilla |
| ttf-parser | Transitive via rustybuzz via krilla | Unmaintained: RUSTSEC-2026-0192 | 2026-11-07 | cargo deny check | Found by `scripts/check.sh --full`; dropped as a direct dependency for skrifa. `deny.toml` checks unmaintained for direct deps only. Recheck on each krilla upgrade |
| Noto Sans | 2.015 static TTFs in `crates/desktop/assets/fonts` | Arch noto-fonts 1:2026.11.01-1 | 2026-11-07 | pacman, font name table, notofonts/latin-greek-cyrillic `OFL.txt` | Arch's `/usr/share/licenses/noto-fonts/LICENSE` holds Apache-2.0 text although the package and font declare OFL-1.1, so `OFL.txt` comes from the upstream repository |
| clap | 4 (4.6.7), `derive` only | 4.6.7 (2026-09-14) | 2026-11-07 | crates.io API | MSRV 1.85. MIT or Apache-2.0. No RustSec directory for it (GitHub API 404). Used only by `crates/cli` |
| serde_json | 1 (1.0.154) | 1.0.154 (2026-10-11) | 2026-11-07 | crates.io API | Already in the graph through Dioxus; now direct in `crates/cli`. MIT or Apache-2.0. No RustSec directory for it (GitHub API 404) |
| uuid | 1, `v5` and `v7` | 1.29.1 (2026-11-06) | 2026-11-07 | crates.io API | `v5` added for `--id-seed` and pulls `sha1_smol`; production ids stay v7 |
