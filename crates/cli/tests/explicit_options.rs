//! An explicit option takes effect or the command fails.

mod common;

use std::fs;

use common::{NO_ID, Scratch, add_lead, data, export, failure, refused, run, text};

#[test]
fn removing_someone_who_is_not_on_the_team_is_refused() {
    let scratch = Scratch::new();
    let db = scratch.database();
    data(&run(&db, &["settings", "set", "--add-member", "Ana"]));
    let before = export(&db);

    let output = run(
        &db,
        &[
            "settings",
            "set",
            "--theme",
            "dark",
            "--remove-member",
            "Nadie Existe",
        ],
    );

    refused(&db, output, 1, "member_not_found", &before);
}

#[test]
fn removing_a_member_still_works() {
    let scratch = Scratch::new();
    let db = scratch.database();
    data(&run(&db, &["settings", "set", "--add-member", "Ana"]));

    let saved = data(&run(&db, &["settings", "set", "--remove-member", "Ana"]));

    assert_eq!(saved["settings"]["team"], serde_json::json!([]));
}

#[test]
fn assigning_someone_outside_the_team_is_refused() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Uno", "987654321", "DAC");
    let before = export(&db);

    let output = run(&db, &["lead", "assign", &lead, "--owner", "Fantasma"]);

    refused(&db, output, 1, "owner_not_in_team", &before);
}

#[test]
fn assigning_a_member_and_clearing_still_work() {
    let scratch = Scratch::new();
    let db = scratch.database();
    data(&run(&db, &["settings", "set", "--add-member", "Ana"]));
    let lead = add_lead(&db, "Uno", "987654321", "DAC");

    let assigned = data(&run(&db, &["lead", "assign", &lead, "--owner", "Ana"]));
    assert_eq!(assigned["lead"]["owner"], "Ana");

    let cleared = data(&run(&db, &["lead", "assign", &lead, "--owner", ""]));
    assert_eq!(cleared["lead"]["owner"], serde_json::Value::Null);
}

#[test]
fn a_new_lead_takes_the_same_owner_rule() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let before = export(&db);

    let output = run(
        &db,
        &[
            "lead",
            "add",
            "--name",
            "Uno",
            "--phone",
            "987654321",
            "--service",
            "DAC",
            "--owner",
            "Fantasma",
        ],
    );

    refused(&db, output, 1, "owner_not_in_team", &before);
}

#[test]
fn a_milestone_edit_needs_a_field() {
    let scratch = Scratch::new();
    let db = scratch.database();

    let (code, _) = failure(&run(&db, &["billing", "edit", NO_ID]), 2);

    assert_eq!(code, "usage");
}

fn quote(db: &std::path::Path) -> String {
    let lead = add_lead(db, "Uno", "987654321", "DAC");
    let made = data(&run(
        db,
        &["quote", "create", "--lead", &lead, "--line", "a|1|100"],
    ));
    text(&made["quote"]["number"]).to_owned()
}

#[test]
fn a_quote_pdf_refuses_an_existing_file() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    let target = scratch.file("victim.csv");
    fs::write(&target, "keep me").unwrap();
    let out = target.display().to_string();

    let (code, _) = failure(&run(&db, &["quote", "pdf", &number, "--out", &out]), 1);

    assert_eq!(code, "pdf_target_exists");
    assert_eq!(fs::read(&target).unwrap(), b"keep me");
}

#[test]
fn force_overwrites_a_quote_pdf() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    let target = scratch.file("old.pdf");
    fs::write(&target, "old").unwrap();
    let out = target.display().to_string();

    data(&run(
        &db,
        &["quote", "pdf", &number, "--out", &out, "--force"],
    ));

    assert!(fs::read(&target).unwrap().starts_with(b"%PDF"));
}

#[test]
fn a_quote_pdf_without_extension_checks_the_file_it_would_write() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    fs::write(scratch.file("doc.pdf"), "keep me").unwrap();
    let out = scratch.file("doc").display().to_string();

    let (code, _) = failure(&run(&db, &["quote", "pdf", &number, "--out", &out]), 1);

    assert_eq!(code, "pdf_target_exists");
    assert_eq!(fs::read(scratch.file("doc.pdf")).unwrap(), b"keep me");
}

#[test]
fn a_quote_pdf_never_replaces_the_database() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    let before = export(&db);

    for name in ["crm.db", "crm.db-wal", "crm.db-shm", "crm.db-journal"] {
        let out = scratch.file(name).display().to_string();
        let output = run(&db, &["quote", "pdf", &number, "--out", &out, "--force"]);
        refused(&db, output, 1, "pdf_target_is_database", &before);
    }
}

#[test]
fn a_quote_pdf_never_replaces_a_hard_link_to_the_database() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    let before = export(&db);
    let bytes = fs::read(&db).unwrap();
    let link = scratch.file("link.pdf");
    fs::hard_link(&db, &link).unwrap();
    let out = link.display().to_string();

    let forced = run(&db, &["quote", "pdf", &number, "--out", &out, "--force"]);
    refused(&db, forced, 1, "pdf_target_is_database", &before);
    let plain = run(&db, &["quote", "pdf", &number, "--out", &out]);
    refused(&db, plain, 1, "pdf_target_is_database", &before);

    assert_eq!(fs::read(&db).unwrap(), bytes);
}

#[test]
fn a_quote_pdf_never_replaces_a_hard_link_to_a_database_sidecar() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    let before = export(&db);
    let sidecar = scratch.file("crm.db-journal");
    fs::write(&sidecar, b"").unwrap();
    let link = scratch.file("journal.pdf");
    fs::hard_link(&sidecar, &link).unwrap();
    let out = link.display().to_string();

    let output = run(&db, &["quote", "pdf", &number, "--out", &out, "--force"]);

    refused(&db, output, 1, "pdf_target_is_database", &before);
    assert!(fs::read(&sidecar).unwrap().is_empty());
}

#[test]
fn a_plain_copy_of_the_database_can_still_be_replaced() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let number = quote(&db);
    let copy = scratch.file("copy.pdf");
    fs::copy(&db, &copy).unwrap();
    let out = copy.display().to_string();

    data(&run(
        &db,
        &["quote", "pdf", &number, "--out", &out, "--force"],
    ));

    assert!(fs::read(&copy).unwrap().starts_with(b"%PDF-"));
}
