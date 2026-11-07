---
name: cominorsa-self-improve
description: "Trigger: end of task, retro, repeated action, automate, improve skills, rule of two. Automate repeats and fold lessons back into the project skills."
license: Apache-2.0
metadata:
  author: "dreamcoder08"
  version: "1.0"
---

## Activation Contract

Load at the end of every task in this repository, and whenever an action is about to run for the third time.

## Hard Rules

- Rule of two: an action done twice is tallied in `odd/automation-ledger.md`; the third time it is automated before the task continues.
- Automate with the lightest tool that works: cargo alias in `.cargo/config.toml`, then a `scripts/*.sh` with `set -euo pipefail`, then a skill step.
- Every lesson that would have saved time goes into the skill that owns it, as one imperative rule, and one line in the ledger's self-improvement log.
- Edit skills only through evidence from this session: a failure, a repeat, a measurement, or a dated source. No speculative rules.
- Keep each `SKILL.md` body under 700 tokens; move detail to `references/`.
- Never let a skill contradict `README.md` or `AGENTS.md`; fix the source of truth first.

## Decision Gates

| Signal | Action |
|---|---|
| Same command typed 3 times | Script or alias it; update ledger |
| Same mistake twice | Add a Hard Rule to the owning skill |
| A check was forgotten | Add it to `scripts/check.sh` |
| A skill rule proved wrong or stale | Rewrite it; log the evidence |
| A skill was never loaded in 5 tasks | Merge it into another or delete it |

## Execution Steps

1. List the commands and manual steps this task used; tally repeats in the ledger.
2. Automate every action at count 3; run the new automation once to prove it.
3. Update the owning skill with each lesson; bump its `metadata.version`.
4. Append dated lines to the self-improvement log.
5. Refresh the skill registry with `/skill-registry:refresh` when a skill was added or renamed.

## Output Contract

Report: automations added, skills changed with their new version, and ledger lines appended. Report "no changes" when nothing qualified.

## References

- `../../odd/automation-ledger.md` — repeat tally and lesson log.
