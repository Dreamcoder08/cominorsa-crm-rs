---
name: cominorsa-dioxus-desktop
description: "Trigger: UI, view, component, rsx, Dioxus, desktop window, CSS, UX. Build and test the COMINORSA Dioxus 0.7 desktop UI."
license: Apache-2.0
metadata:
  author: "dreamcoder08"
  version: "1.0"
---

## Activation Contract

Load for any change in `crates/desktop`.

## Hard Rules

- Dioxus stays on 0.7.x; 0.8 is alpha and is adopted only through a planned migration task.
- UI copy is Spanish for the client; identifiers, comments and tests are English.
- Components call domain functions for every business rule; no validation logic in views.
- Formatting and labels live in `presentation.rs` as plain functions so tests reach them without a renderer.
- Every view change gets a test in `crates/desktop/tests` rendering with `dioxus-ssr` and asserting visible text or state.
- Async save actions disable their trigger while pending; never allow duplicate submission.
- Compute "today" when it is used, never once at startup.
- Startup errors are shown to the user with the cause, never swallowed.
- Styles stay in `style.css`; no inline style strings except dynamic values.

## Decision Gates

| Need | Approach |
|---|---|
| Derived value | Memo or plain function over signals, not a second signal kept in sync |
| Persisting data | Call the `LeadRepository` port, never `sqlx` from the UI |
| New dependency for UI | Load `cominorsa-stack-freshness` first; prefer CSS over a crate |
| Visual check needed | Run the release app and confirm with the user; SSR tests do not prove layout |

## Execution Steps

1. Write the SSR or presentation test first and see it fail.
2. Implement in `views.rs` or `presentation.rs`.
3. Run `scripts/check.sh`.
4. For visible changes, ask the user to confirm in the window and record the result in the feature Log.

## Output Contract

Report the tests added, whether the window was exercised, and anything left unverified.

## References

- `../../crates/desktop/tests/board.rs` — SSR test patterns.
- `../cominorsa-stack-freshness/references/stack-watch.md` — Dioxus version notes.
