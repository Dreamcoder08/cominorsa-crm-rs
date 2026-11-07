# Performance baseline

## Environment (2026-11-06)

- Linux, kernel 7.2, `target/` on ext4.
- rustc 1.98.0; default linker on x86_64 Linux is rust-lld since Rust 1.90.
- No `[profile.release]` overrides yet.

## Measurements

| Metric | Value | Date | Source |
|---|---|---|---|
| Release binary | 10.4 MB | 2026-11-06 | odd/tasks/lead-pipeline-slice.md L6 |
| Idle PSS total, empty DB | ~213 MB (app 96, web 92, network 25) | 2026-11-06 | L6 |
| Tests | 23 pass (12 desktop, 8 domain, 3 storage) | 2026-11-06 | scripts/check.sh |

## Pending measurements

- Clean and incremental build times with `cargo build --timings`.
- Cold start to first paint.
- Windows (WebView2) size and memory.

## External evidence (dated)

- 2026-10-27, 2026-10-31: linker benchmarks show link time is 1-4% of release rebuilds for mid-size projects; mold or wild matter mainly for large debug links. Wild is slow on btrfs only with kernels older than 7.2.
