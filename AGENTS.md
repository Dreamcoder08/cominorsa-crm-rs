# Agent instructions

Read `README.md` before changing behavior. Feature tracking lives in `odd/tasks/`.

- Keep business rules in `crates/domain`, free of I/O and UI dependencies.
- Keep Dioxus types inside `crates/desktop`.
- Never add customer data, credentials, or real exports to this repository.
- Do not add outreach, WhatsApp sending, or SUNAT emission without explicit authorization for the destination, operation, and credential.
- Run the checks listed in `README.md` before committing.
