# Lead pipeline slice

Objective: first working desktop flow of the COMINORSA CRM: register lead, move it through the pipeline, see overdue follow-ups.
Branch: `feat/lead-pipeline-slice`. Delivery strategy: `ask-on-risk`. Test runner: `cargo test --workspace`.

## Specs

- S1. New repository with a Rust workspace, separate from `cominorsa-crm`. User: "para el nuevo crm o proyecto parece que debes crear nuevo repo , estrucuta y mas".
- S2. Application code is written in Rust. User: "quiero lo mejor de lo mejor en 100% rust". Assumption: third-party runtime components (SQLite, system webview) may be non-Rust.
- S3. The app runs on the client's laptop. User: "el cliente por el momento me exige que corra en su laptop".
- S4. Desktop style, not a browser tab. User: "no podria ser mejor estilo desktop ??".
- S5. Register a lead with name, phone, optional RUC, requested service and next follow-up date. Assumption: field set pending client validation.
- S6. Reject a RUC that fails format or check-digit validation. Assumption: validation is offline and does not prove the RUC exists at SUNAT.
- S7. Move a lead through pipeline stages. Assumption: stages come from a pasted proposal and need client validation: "1. Contacto inicial -> 2. Evaluación de coordenadas -> 3. Cotización enviada -> 4. Contrato firmado".
- S8. Show leads whose follow-up date is overdue or due today. Assumption: alerts appear inside the open app only.
- S9. Data persists across restarts in a local SQLite file. No customer data enters this repository; fixtures are synthetic.

## Tasks

- [x] T1. S1, S2, S5-S7: workspace and domain model. Route: inline. Commit: 0d167ad.
- [x] T2. S8, S9: SQLite storage adapter. Route: inline. Commit: d998a7b.
- [x] T3. S3, S4, S5, S7, S8: Dioxus desktop UI. Route: inline. Commit: bd93a86.

## Log

- L1 (2026-11-06, original request, verbatim): "bueno quiero lo mejor de lo mejor en 100% rust porfavor objetivo estar en la vanguardia , para el nuevo crm o proyecto parece que debes crear nuevo repo , estrucuta y mas verdad ??"
- L2 (2026-11-06): UI decision: Dioxus desktop, because application UI stays in Rust and the webview renderer is the non-experimental desktop path. Dioxus is 0.7, so it stays isolated in `crates/desktop`.
- L3 (2026-11-06): Concurrent user count is unanswered. The slice assumes one user; storage sits behind a port.
- Forecast: about 700 authored lines. Delivery strategy is unresolved until a push or pull request is requested.
- L4 (2026-11-06): The user's screenshot showed an unset date input with a greyed-out date and the error "Ingresa la fecha de seguimiento". Fixed by starting the form with today's date; the native default menu was removed.
- L5 (2026-11-06): The user asked "no seria mejor el stack rust + go + Mygo ??". Decision: stay on Rust; no second language.
- L6 (2026-11-06): Verification: 17 tests pass, clippy passes with warnings denied, release build 10.4 MB, about 213 MB idle PSS on Linux. The date fix and full click-through in the window are pending user confirmation. Windows is untested.
- Next: confirm the fix in the window, validate S5 and S7 with the client, then add backups.
- L7 (2026-11-06): Native review of 0d167ad..bd93a86 was granted, approved and acknowledged (lineage `review-dcf381afd374b049`). The full branch exceeded the review context budget because of the initial `Cargo.lock`, so domain commit 0d167ad was not reviewed. Advisory findings for later work: `R3-duplicate-submit`, `R3-stale-today`, `R3-startup-error-masked`, `R3-app-flow-unproved`.
