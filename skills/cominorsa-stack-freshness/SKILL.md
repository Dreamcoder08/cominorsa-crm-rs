---
name: cominorsa-stack-freshness
description: "Trigger: upgrade, new version, dependency, toolchain, is this current, vanguard, latest, advisory. Challenge stack choices with dated sources from the current month."
license: Apache-2.0
metadata:
  author: "dreamcoder08"
  version: "1.0"
---

## Activation Contract

Load before adding or upgrading a dependency or toolchain, before recommending a tool, and when the user asks whether something is current.

## Hard Rules

- Never state that a version, tool, or practice is current from memory; verify with a source dated within 30 days, or say it is unverified.
- Prefer primary sources: official release notes, changelogs, crates.io, RustSec.
- Record every verified fact in `references/stack-watch.md` with date and source; delete facts older than 90 days or replace them.
- Challenge the request when fresh evidence contradicts it; give the evidence and a recommendation.
- Pre-1.0 crates (Dioxus) are pinned to a minor; upgrade on purpose with a migration task, never via a blind `cargo update`.
- Run advisories from two databases: `cargo deny check advisories` (RustSec) plus Dependabot or OSV; one quiet tool is not a clean tree.
- Toolchain changes run `scripts/check.sh --full` on the new version before being committed.

## Decision Gates

| Situation | Action |
|---|---|
| Fact in stack-watch under 30 days old | Use it, cite its date |
| Older or missing | Search the web for this month; update stack-watch |
| New stable Rust | Test on beta first when a release note mentions cargo layout or lints |
| Alpha or beta of a dependency | Track only; do not adopt for customer delivery |
| Open advisory in the graph | Check whether the affected feature is compiled; record the verdict |

## Execution Steps

1. Read `references/stack-watch.md`.
2. Refresh stale entries with 2-4 dated web queries.
3. Compare against `Cargo.toml`, `Cargo.lock` and `rustc --version`.
4. Write the delta and a recommendation; update stack-watch.
5. Hand any upgrade to `cominorsa-quality-gate`.

## Output Contract

Report a table: component, ours, latest verified, date, source, recommendation.

## References

- `references/stack-watch.md` — dated facts about the stack.
