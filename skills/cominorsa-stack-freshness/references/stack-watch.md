# Stack watch

Verified facts only. Each entry has a check date and a source. Replace entries older than 90 days.

| Component | Ours | Latest verified | Checked | Source | Note |
|---|---|---|---|---|---|
| Rust stable | 1.98.0 | 1.99.1 (2026-10-15) | 2026-11-06 | blog.rust-lang.org/2026/10/15/Rust-1.99.1 | 1.99.1 fixes an x86_64 FFI bool miscompilation |
| Rust next | - | 1.100 due 2026-11-12 | 2026-11-06 | Rust 1.100 beta reports | Test binaries move to a new build-dir layout; tests must use `env!("CARGO_BIN_EXE_<name>")`, never `current_exe()`. This repo has no such tests today |
| Dioxus | 0.7 | 0.7.10 stable; 0.8.0-alpha.2 | 2026-11-06 | crates.io via hypedevgroup.pl (2026-10-08), Dioxus merged PRs (2026-10) | Main branch moved to wry 0.57 and tao 0.37; custom protocol handlers need `Send + Sync` in 0.8 |
| sqlx | 0.9 | 0.9.0 (2026-05-21), no patch yet | 2026-11-06 | dev-news.anonyfox.com/sqlx (2026-10-24) | Repo moved to transact-rs/sqlx; `cargo install --locked sqlx-cli` no longer works; per-crate `sqlx.toml` |
| sqlx advisories | - | RUSTSEC-2023-0071 (rsa) in default graph | 2026-11-06 | thecratedigest.com (2026-11-01) | Absent here: `Cargo.lock` has no `rsa` crate with our sqlite-only features (checked 2026-11-06) |
| Advisory sources | none in CI | RustSec and GitHub databases diverged in 2026 | 2026-11-06 | safeguard.sh (2026-10-18) | Run cargo-deny and Dependabot or OSV |
| Test runner | cargo test | cargo-nextest recommended for CI | 2026-11-06 | rustify.rs (2026-10-26) | Keep `cargo test --doc` for doctests |
| Linker | rust-lld (default) | mold 3.0, wild 0.11 | 2026-11-06 | mehdi.cz (2026-10-31), efraingaray.com (2026-10-27) | Gain is small for release rebuilds; measure first |
