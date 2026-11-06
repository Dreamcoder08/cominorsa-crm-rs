# Automation ledger

Rule of two: when the same manual action runs a third time, automate it before continuing. Tally each repeat here; the third repeat must land as a script, cargo alias, or skill step.

| Action | Count | Automated by | Date |
|---|---|---|---|
| fmt + clippy + test before commit | 3+ | `scripts/check.sh` | 2026-11-06 |
| Check dependency and toolchain freshness | 1 | `skills/cominorsa-stack-freshness` | 2026-11-06 |
| Measure release size and idle memory | 1 | `scripts/check.sh --full` (size only) | 2026-11-06 |

## Self-improvement log

Append one line per lesson that changed a skill: date, skill, what changed, and why.

- 2026-11-06: created project skills; baseline from `odd/tasks/lead-pipeline-slice.md` L6.
