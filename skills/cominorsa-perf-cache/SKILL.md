---
name: cominorsa-perf-cache
description: "Trigger: slow build, compile time, binary size, memory, startup, cache, linker, sccache, performance. Measure then optimize builds and the desktop app."
license: Apache-2.0
metadata:
  author: "dreamcoder08"
  version: "1.0"
---

## Activation Contract

Load for any build-speed, CI cache, binary size, memory, startup, or query performance work.

## Hard Rules

- Measure before and after on the same machine; report medians, never a single run.
- No optimization without a baseline in `references/baseline.md`; update it after each accepted change.
- Change one knob at a time; revert it when the gain is within noise.
- Never trade correctness, `unsafe_code = "forbid"`, or test coverage for speed.
- Cache keys include toolchain version, target, features and `Cargo.lock` hash; a cache that restores slower than it rebuilds is removed.
- Release profile changes are measured for size, idle PSS and cold start.

## Decision Gates

| Symptom | First tool | Candidate fix |
|---|---|---|
| Slow rebuild | `cargo build --timings` | `cargo check` in the edit loop; trim features; faster linker |
| Link-heavy debug build | timings bin unit | mold or wild (target is on ext4, kernel 7.2: both safe) |
| Slow clean or CI build | timings, cache hit rate | sccache; registry and target cache keyed as above |
| Large binary | `cargo bloat` | `[profile.release]` lto, codegen-units = 1, strip, panic = "abort" |
| High idle memory | PSS per process | Webview cost dominates; avoid extra windows and polling |
| Slow list or search | `hyperfine` or test timer | SQLite index, `EXPLAIN QUERY PLAN`, fewer clones per render |

## Execution Steps

1. Read `references/baseline.md`; rerun the baseline when it is older than the toolchain.
2. Profile with the gate's first tool; name the dominant cost.
3. Apply one fix; rerun the same measurement 5+ times.
4. Keep it only with a gain beyond noise; record numbers and date.
5. Run `scripts/check.sh --full`.

## Output Contract

Report a before/after table with medians, the command used, and kept or reverted.

## References

- `references/baseline.md` — current measured numbers and environment.
