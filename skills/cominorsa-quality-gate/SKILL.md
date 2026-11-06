---
name: cominorsa-quality-gate
description: "Trigger: commit, before finishing, quality check, clippy, tests, COMINORSA CRM change. Enforce the workspace quality gate and layering rules."
license: Apache-2.0
metadata:
  author: "dreamcoder08"
  version: "1.1"
---

## Activation Contract

Load before any commit or before reporting a code change as done in this repository.

## Hard Rules

- Run `scripts/check.sh` before every commit; never hand-type fmt, clippy and test separately.
- Run `scripts/check.sh --full` before a push, a dependency change, or a release build.
- Write a failing test first for every behavior change; observe RED before GREEN.
- `crates/domain` imports no I/O, async runtime, SQL, or UI crate.
- Dioxus types never leave `crates/desktop`.
- `unsafe_code` stays forbidden; never add `#[allow]` to silence clippy without a one-line reason.
- Fixtures are synthetic; never commit customer data, credentials, `.db`, or `.csv` files.
- A skipped check is reported as SKIPPED with the reason, never as passed.
- Other sessions may edit this tree: stage only paths you changed, by name; never `git add -A` or `git commit -a`.

## Decision Gates

| Change | Extra proof |
|---|---|
| Domain rule | Test in `crates/domain/tests` covering the rule's examples, boundaries and errors |
| Storage or migration | Load `cominorsa-sqlite-storage` |
| UI | Load `cominorsa-dioxus-desktop`; SSR test in `crates/desktop/tests` |
| Dependency or toolchain | Load `cominorsa-stack-freshness` |
| Performance claim | Load `cominorsa-perf-cache`; measured numbers only |

## Execution Steps

1. Write or update the failing test; run it and see it fail.
2. Implement the smallest change that passes.
3. Run `scripts/check.sh`; fix every failure at the root.
4. Update README or help text when a command, option, or message changed.
5. Load `cominorsa-self-improve` and record any repeated action.

## Output Contract

Report: tests run with pass counts, clippy result, any SKIPPED check, and `Risk: none` or `Risk: item N (reason)`.

## References

- `../../README.md` — rules and structure.
- `../../scripts/check.sh` — the gate itself.
