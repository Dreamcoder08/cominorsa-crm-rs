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
- S10. Improve the desktop UI and UX. User: "no hay forma de mejora el diseño ui & Ux ?". Assumption: the slice keeps the existing lead fields and stages.
- S11. Load the existing spreadsheet data into the software. User: "los datos de clientes ya estaban en un excel buscalo y pasalos al softwart". Evidence: the source holds INGEMMET concession titleholders, without phone, RUC, or email, so records load as prospects and never as leads with invented contact data. Customer files stay outside this repository.
- S12. Improve the design system with light and dark themes. User: "mejora el system desing del proyecto inspirate en el mejor theme light , theme dark".
- S13. Add a settings section to the sidebar. User: "le falta su seccion de configuraciones en su sidebar".
- S14. The sidebar and layout are responsive. User: "tambien debe ser responsive".
- S15. Add features that satisfy the client, inspired by global and regional leaders. User: "mas funcionalidades que le falta al proyecto para que el cliente este satisfecho". Assumption: delivered incrementally, starting with the agenda, team list, and manual backup; WhatsApp stays out until authorized.
- S16. Add a WhatsApp button that opens the chat with a prepared message; a person reviews and sends it. Without API. The user authorized it: "yes te autorizo". The prepared message is editable in Settings.
- S17. Show the app icon. User: "me gustaria que se muestre el icon de la app actualmente no sale si necesitas crear imagen o algo usa a codex cli". Assumption: the mark is the existing COMINORSA logo, redrawn in high resolution, not a new design.
- S18. Backups go to a folder the user chooses, show the last backup, and can be restored. Assumption: restore validates the file and first saves a safety copy.
- S19. The team list is used: each lead has a responsible person, and it can change.
- S20. Settings validate visibly and share one save model. Assumption: valid changes save automatically and show "Guardado".
- S21. Fix the date tooltip, theme flash, and non-text contrast.
- S22. Redesign Settings as a settings list with a WhatsApp preview.
- S23. Enrich prospect companies with the official SUNAT roll: RUC, status, condition, and fiscal address. User: "yes te autorizo todo has lo que sea necesario solo necesito los datos". Exclusions: natural persons are not searched; no phone or email exists in the source; ambiguous names stay without a RUC.
- S24. "Expediente automático al firmar contrato. Es el corazón de la fusión." Advancing a lead to Contrato firmado atomically opens exactly one case with procedure steps and responsible person.
- S25. "Plazos por trámite en la Agenda, junto con los seguimientos comerciales." Open case steps appear in the Agenda next to follow-ups.
- S26. "Hitos de cobro por expediente." Each case has billing milestones with amount, due date, and paid state; amounts use integer cents; no tax invoices.
- S27. "Cotización en PDF, que cierra el tramo inicial." A lead can produce a numbered quote as a PDF, with correct Spanish text.
- S28. "Renovaciones, que convierten un cliente atendido en una nueva oportunidad." Closing a recurring case creates a new follow-up lead.
- Constraint for S24-S28: legal deadlines are never hardcoded. DAC dates change by yearly resolution and last RUC digit (RD 0144-2026-MINEM/DGM), so step dates are editable suggestions, labeled as such, pending client validation.

## Tasks

- [x] T1. S1, S2, S5-S7: workspace and domain model. Route: inline. Commit: 0d167ad.
- [x] T2. S8, S9: SQLite storage adapter. Route: inline. Commit: d998a7b.
- [x] T3. S3, S4, S5, S7, S8: Dioxus desktop UI. Route: inline. Commit: bd93a86.
- [x] T4. S10: redesigned workspace, metrics, search, follow-up status and lead panel. Route: inline. Commit: 1671ed5.
- [x] T5. S11: prospect storage, importer, and desktop section with conversion to lead. Route: inline. Commits: 156eae8, 207fb47, 5418b3e.
- [x] T6. S13, S15: settings persistence with theme, team, follow-up days, and manual backup. Route: inline. Risk: item 1. Commit: f2cab15.
- [x] T7. S12-S15: design system, responsive shell, settings view, and agenda. Route: inline. Commit: e49e7b2.
- [x] T8. S16: WhatsApp chat link with an editable message template. Route: inline. Commit: 2550d2a.
- [x] T9. S17: icon in window, sidebar, Windows executable, and Linux launcher. Route: inline. Commit: 67087e5.
- [x] T10. S18, S19: owner migration, backup settings, restore, and template rules. Route: inline. Risk: item 1. Commit: 71f2dcd.
- [x] T11. S18-S22: desktop behavior and redesign. Route: inline. Commit: 1cbbe48.
- [x] T12. S23: tax data migration, importer enrichment, and prospect view. Route: inline. Risk: item 1. Commits: 825f906, 949fe9b.
- [x] T13. S24, S25: cases, steps, and Agenda deadlines. Route: delegated, context backstop. Risk: item 1. Commits: bce58f7, 4cff2d5.
- [x] T14. S26, S28: billing milestones and renewals. Route: delegated, context backstop. Risk: item 1. Commits: 0a41496, 80d1a9f.
- [ ] T15. S27: quote PDF. Route: delegated, context backstop. Risk: item 1. Commit: pending.

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
- L8 (2026-11-06, user answer, verbatim): "lo va a usar el dueño que es mi prima y su novio que tambien es su socio". Two users are confirmed; whether they share one device is unanswered.
- L9 (2026-11-06, user answer, verbatim): "si por el momento los 2 usan la misma laptop". No network mode or login is needed for now.
- L10 (2026-11-06): Fixed `R3-duplicate-submit` in 9aed357. 18 tests pass and clippy is clean; the fix was not exercised in the live window.
- L11 (2026-11-06, user request, verbatim): "perfecto solo eso ?? y mas automatizacion o mas ? no hay forma de mejora el diseño ui & Ux ?".
- L12 (2026-11-06): Redesign committed in 1671ed5. 23 tests pass. A real-window capture with synthetic data confirmed the workspace, metrics, status chips and removed menu. The new-lead panel, Escape, and search were not exercised in the live window.
- L13 (2026-11-06, user request, verbatim): "bien pero los datos de clientes ya estaban en un excel buscalo y pasalos al softwart".
- L14 (2026-11-06): Source profile: 799 companies and 1222 people, with 0 phones, 0 RUCs, and 0 emails. Imported 2021 prospects into the local database; a repeated import added 0; integrity check ok. Not imported: 94 titleholders excluded earlier as medium or large mining.
- L15 (2026-11-06): Risk: item 1 (new table). An independent verifier was launched. Live-window use of the Prospects section and conversion remains unconfirmed.
- L16 (2026-11-06): The public repository `Dreamcoder08/cominorsa-web` tracks the source spreadsheet and CSV files. Cleanup awaits a user decision.
- L17 (2026-11-06, user request, verbatim): "hey bro mejora el system desing del proyecto inspirate en el mejor theme light , theme dark , ademas que le falta su seccion de configuraciones en su sidebar , que tambien debe ser responsive y mas funcionalidades que le falta al proyecto para que el cliente este satisfecho inspriate en las funcionalidades de los mejores del mundo , competencia local , regional , y mas".
- L18 (2026-11-06): Windows Actions run 42311118079 did not start because of an account billing block. Cross-compilation with zig produced PE32+ binaries; they have not been run on Windows.
- L19 (2026-11-06): Verifier passed S11 with no blockers. Advisories kept: backups hold the post-migration schema, reruns leave extra backups, conversion does not mark the prospect, and a failed startup may leave Prospects loading.
- L20 (2026-11-06): Real-window captures confirmed light theme, dark theme, and the icon rail. Bottom navigation, settings clicks, and agenda were not exercised in the live window. Native review of commits after 4728027 is pending.
- L21 (2026-11-06, user answer, verbatim): "yes te autorizo". Authorized scope: open `wa.me` in the external browser or app with prepared text. No credential, API, or automatic send.
- L22 (2026-11-06, user requests, verbatim): "me gustaria que se muestre el icon de la app actualmente no sale si necesitas crear imagen o algo usa a codex cli" and "usa hc de herdr para abrir codex cli si es necesario". The icon was redrawn by Codex CLI from the existing logo, before the second message arrived.
- L23 (2026-11-06, user request, verbatim): "si tienes problemas de github ci y mas has el repo publico por el momento". `cominorsa-crm-rs` is public after a history audit; `cominorsa-web` stays private.
- L24 (2026-11-06, user answer, verbatim): "yes confirmo y autorizo todo". Scope: the five points of the Settings audit.
- L25 (2026-11-06): Commit 71f2dcd went in while the gate failed on a desktop test helper, because the shell condition read `tail`, not the gate. The next desktop commit fixes it. Settings were checked in a temporary preview in both themes; folder dialogs and live restore clicks were not exercised.
- L26 (2026-11-06): The verifier reported two blockers on d010951: empty or unrelated files passed restore validation, and stored templates with unknown placeholders blocked startup. Fixed in c758cdc with tests that fail on the previous code. A rename failure now keeps the database in place and names the safety copy.
- L27 (2026-11-06): Verifier recheck at ed0e7d6 passed both blockers. Advisories kept: a rename failure leaves `crm.restoring.db`, the app needs a restart after a post-close failure, and a reopen failure reports an error although the backup was applied.
- L28 (2026-11-06): SUNAT roll published 2026-11-06 has no phone or email columns. Companies: 436 exact, 30 approximate, 9 ambiguous, 324 without match; 376 active. The output file stays outside the repository.
- L29 (2026-11-07, user request, verbatim): "imnplementa esto ademas si tienes dudas mira a la competencia aqui en piura o peru o en laitnoametica o como lo hacen los mejores del mundo china , usa y mas". Research found the case-centered pattern in Clio; no usable evidence on Feishu, DingTalk, or Peruvian mining-consultancy software.
- Delivery forecast: about 1500 more authored lines. Commits stay on the feature branch; no pull request was requested.
- L30 (2026-11-07): T13 done in bce58f7 and 4cff2d5. `scripts/check.sh` passed with 121 tests. RED: compile failures, then assertion failures against stubbed and mutated code (no transaction, no idempotence, no status filter, no restore validation). Migration 0006 survived a schema-5 database with synthetic rows, in tests and in one app run. Importer output matches 4cb0726 except the backup timestamp. Window clicks were not exercised. Open points: leads signed before T13 have no case; the case owner is a snapshot, so a later change on the lead card does not reach it; a service naming two procedures, such as "IGAFOM + REINFO", uses the first template (REINFO). Template titles and offsets await client validation.
- L31 (2026-11-07): T14 done in 0a41496 and 80d1a9f, covering S26, S28, and the T13 gap: `procedure_template` now combines every named service in text order without repeating a step title. `scripts/check.sh` passed with 199 tests. RED: the combining tests failed on the T13 code; 18 storage and 31 desktop tests failed against stubs; domain billing and renewal rules were written with their code, so their RED came from mutations (24 failures across two passes). Migration 0007 survived a schema-6 database with synthetic rows, in tests and in one app run: 2 leads, 1 case, 1 step, settings kept, integrity check ok. Window clicks were not exercised. Decisions: unpaid milestones of a closed case stay listed and in the Agenda until paid, through `active_cases`, because closing must not hide money owed; the closing confirmation warns about them. Renewal names carry "Renovación: ", and WhatsApp text and new cases use the client name without it. Pending never drops below zero; without a contract total it is the unpaid sum. Open points: a service naming DAC with others renews as a whole; milestones may stop summing to the total after edits, with no warning; closing has no date stored; the 30, 40, 30 split, its 0, 30, 60 day dates, and the 12-month DAC interval await client validation.
