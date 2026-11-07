//! Rules the desktop shows are reachable without a window.

mod common;

use std::{collections::BTreeSet, fs, path::Path};

use common::{Scratch, add_lead, data, export, failure, refused, run, run_on, sqlite, text};

#[test]
fn quote_preview_shows_the_totals_at_the_stored_rate() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let before = fs::read(&db).unwrap();

    let found = data(&run(
        &db,
        &[
            "quote",
            "preview",
            "--line",
            "a|2|100",
            "--line",
            "b|1|50.50",
        ],
    ));

    assert_eq!(found["subtotal_cents"], 25050);
    assert_eq!(found["igv_cents"], 4509);
    assert_eq!(found["total_cents"], 29559);
    assert_eq!(fs::read(&db).unwrap(), before);
}

#[test]
fn quote_preview_reports_an_unreadable_line() {
    let scratch = Scratch::new();
    let db = scratch.database();

    let (code, _) = failure(&run(&db, &["quote", "preview", "--line", "a|0|100"]), 1);

    assert_eq!(code, "invalid_quantity");
}

#[test]
fn the_whatsapp_preview_renders_the_stored_template() {
    let scratch = Scratch::new();
    let db = scratch.database();
    data(&run(
        &db,
        &[
            "settings",
            "set",
            "--whatsapp-template",
            "Hola {nombre}: {servicio}",
        ],
    ));

    let found = data(&run(&db, &["settings", "whatsapp-preview"]));

    assert_eq!(found["message"], "Hola Minera Ejemplo: REINFO");
}

#[test]
fn the_whatsapp_preview_tries_a_template_without_saving_it() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let before = export(&db);

    let found = data(&run(
        &db,
        &[
            "settings",
            "whatsapp-preview",
            "--template",
            "Sobre {servicio}",
        ],
    ));

    assert_eq!(found["message"], "Sobre REINFO");
    assert_eq!(export(&db), before);
}

#[test]
fn the_whatsapp_preview_refuses_an_unknown_placeholder() {
    let scratch = Scratch::new();
    let db = scratch.database();

    let (code, _) = failure(
        &run(&db, &["settings", "whatsapp-preview", "--template", "{x}"]),
        1,
    );

    assert_eq!(code, "unknown_placeholder");
}

#[test]
fn lead_show_reports_its_follow_up_status_for_the_day() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let id = add_lead(&db, "Uno", "987654321", "DAC");

    let status = |day: &str| {
        let found = data(&run_on(&db, day, &["lead", "show", &id]));
        found["follow_up"]["status"].clone()
    };

    assert_eq!(status("2026-03-21"), "overdue");
    assert_eq!(status("2026-03-20"), "today");
    assert_eq!(status("2026-03-19"), "upcoming");
}

#[test]
fn export_stays_free_of_the_day() {
    let scratch = Scratch::new();
    let db = scratch.database();
    add_lead(&db, "Uno", "987654321", "DAC");

    let leads = export(&db);

    assert!(leads["leads"][0].get("follow_up").is_none());
}

#[test]
fn a_quote_change_carries_its_notice_under_json() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Uno", "987654321", "DAC");

    let made = data(&run(
        &db,
        &["quote", "create", "--lead", &lead, "--line", "a|1|100"],
    ));

    let number = text(&made["quote"]["number"]);
    assert_eq!(made["notice"], format!("{number} guardada como borrador."));
    let sent = data(&run(
        &db,
        &["quote", "send", number, "--allow-blank-company"],
    ));
    assert_eq!(sent["notice"], format!("{number} marcada como enviada."));
}

#[test]
fn a_refused_preview_changes_nothing() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let before = export(&db);

    let output = run(&db, &["quote", "preview", "--line", "|1|100"]);

    refused(&db, output, 1, "missing_field", &before);
    assert_eq!(sqlite(&db, "SELECT COUNT(*) FROM quotes"), "0");
}

/// Every quoted code that follows `marker` in the Rust files under `folder`.
fn codes_after(folder: &Path, marker: &str, found: &mut BTreeSet<String>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            codes_after(&path, marker, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            for part in source.split(marker).skip(1) {
                let rest = part.trim_start();
                if let Some(rest) = rest.strip_prefix('"') {
                    let code = rest.split('"').next().unwrap();
                    if !code.is_empty() && code.chars().all(|c| c == '_' || c.is_ascii_lowercase())
                    {
                        found.insert(code.to_owned());
                    }
                }
            }
        }
    }
}

#[test]
fn the_readme_lists_every_error_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let mut codes = BTreeSet::new();

    for folder in ["crates/application/src", "crates/cli/src"] {
        for marker in ["rejected(", "storage(", "code: ", "=> ", "fail("] {
            codes_after(&root.join(folder), marker, &mut codes);
        }
    }
    codes.retain(|code| code.contains('_') || code == "usage");

    assert!(codes.len() > 30, "{codes:?}");
    let missing: Vec<&String> = codes
        .iter()
        .filter(|code| !readme.contains(&format!("`{code}`")))
        .collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn lead_list_returns_the_follow_up_of_lead_show() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let id = add_lead(&db, "Ana", "900000001", "DAC");

    for (day, status) in [
        ("2026-03-02", "upcoming"),
        ("2026-03-20", "today"),
        ("2026-03-21", "overdue"),
    ] {
        let shown = data(&run_on(&db, day, &["lead", "show", &id]));
        let listed = data(&run_on(&db, day, &["lead", "list"]));

        assert_eq!(shown["follow_up"]["status"], status);
        assert_eq!(listed["leads"][0]["follow_up"], shown["follow_up"]);
        assert_eq!(listed["leads"][0]["id"], shown["lead"]["id"]);
    }
}
