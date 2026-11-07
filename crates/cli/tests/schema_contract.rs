//! The CLI never migrates implicitly: `init` creates and only `migrate` upgrades.

mod common;

use std::fs;

use common::{NO_ID, Scratch, data, failure, run, sqlite};

/// Every command that writes, with arguments that would pass parsing.
fn writes(scratch: &Scratch) -> Vec<Vec<String>> {
    let csv = scratch.file("input.csv").display().to_string();
    let copy = scratch.file("copy.db").display().to_string();
    let folder = scratch.file("folder").display().to_string();
    let lines = [
        vec![
            "lead",
            "add",
            "--name",
            "",
            "--phone",
            "1",
            "--service",
            "X",
        ],
        vec!["lead", "advance", NO_ID],
        vec!["lead", "assign", NO_ID, "--owner", "Ana"],
        vec!["prospect", "import", "--companies", &csv, "--people", &csv],
        vec!["prospect", "enrich", "--sunat", &csv],
        vec!["prospect", "convert", "--name", "X", "--phone", "1"],
        vec!["case", "close", NO_ID],
        vec!["step", "done", NO_ID],
        vec!["step", "undo", NO_ID],
        vec!["step", "date", NO_ID, "--date", "2026-05-01"],
        vec!["billing", "total", NO_ID, "--amount", "10"],
        vec!["billing", "plan", NO_ID],
        vec![
            "billing",
            "add",
            NO_ID,
            "--title",
            "x",
            "--amount",
            "1",
            "--due",
            "2026-05-01",
        ],
        vec!["billing", "edit", NO_ID, "--title", "x"],
        vec!["billing", "pay", NO_ID],
        vec!["billing", "unpay", NO_ID],
        vec!["billing", "delete", NO_ID],
        vec!["quote", "create", "--lead", NO_ID, "--line", "a|1|1"],
        vec!["quote", "edit", "X", "--notes", "n"],
        vec!["quote", "send", "X"],
        vec!["quote", "accept", "X"],
        vec!["quote", "reject", "X"],
        vec!["quote", "version", "X"],
        vec!["settings", "set", "--theme", "dark"],
        vec!["backup", "create", "--folder", &folder],
        vec!["backup", "restore", "--file", &copy],
    ];

    lines
        .iter()
        .map(|line| line.iter().map(|part| (*part).to_owned()).collect())
        .collect()
}

fn refuses_every_write(scratch: &Scratch, name: &str, code: &str) {
    let db = scratch.file(name);
    let names = scratch.names();
    let bytes = fs::read(&db).ok();

    for command in writes(scratch) {
        let args: Vec<&str> = command.iter().map(String::as_str).collect();
        let found = failure(&run(&db, &args), 3);
        assert_eq!(found.0, code, "{command:?}");
        assert_eq!(fs::read(&db).ok(), bytes, "{command:?}");
        assert_eq!(scratch.names(), names, "{command:?}");
    }
}

#[test]
fn no_write_upgrades_an_older_schema() {
    let scratch = Scratch::new();
    let db = scratch.database_at(5, "old.db");

    refuses_every_write(&scratch, "old.db", "schema_outdated");

    assert_eq!(
        sqlite(&db, "SELECT MAX(version) FROM _sqlx_migrations"),
        "5"
    );
    let (_, message) = failure(&run(&db, &["lead", "advance", NO_ID]), 3);
    assert!(message.contains("cominorsa migrate"), "{message}");
}

#[test]
fn no_write_touches_a_newer_schema() {
    let scratch = Scratch::new();
    let db = scratch.newer_database("new.db");
    let journal = sqlite(&db, "PRAGMA journal_mode");

    refuses_every_write(&scratch, "new.db", "schema_newer");

    assert_eq!(sqlite(&db, "PRAGMA journal_mode"), journal);
}

#[test]
fn no_write_creates_a_database() {
    let scratch = Scratch::new();

    refuses_every_write(&scratch, "absent.db", "database_missing");

    assert!(!scratch.file("absent.db").exists());
}

#[test]
fn a_read_reports_the_same_missing_database() {
    let scratch = Scratch::new();
    let db = scratch.file("absent.db");

    assert_eq!(failure(&run(&db, &["status"]), 3).0, "database_missing");
    assert_eq!(failure(&run(&db, &["migrate"]), 3).0, "database_missing");
}

#[test]
fn init_creates_a_database_at_the_current_schema() {
    let scratch = Scratch::new();
    let db = scratch.file("nested/crm.db");

    let created = data(&run(&db, &["init"]));

    assert_eq!(created["created"], true);
    assert_eq!(created["schema_version"], 9);
    let found = data(&run(&db, &["status"]));
    assert_eq!(found["schema_version"], 9);
    assert_eq!(found["counts"]["leads"], 0);
}

#[test]
fn init_refuses_an_existing_file() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let bytes = fs::read(&db).unwrap();

    let (code, message) = failure(&run(&db, &["init"]), 1);

    assert_eq!(code, "database_exists");
    assert!(message.contains("crm.db"), "{message}");
    assert_eq!(fs::read(&db).unwrap(), bytes);
}

#[test]
fn migrate_upgrades_and_keeps_rows_and_a_copy() {
    let scratch = Scratch::new();
    let db = scratch.database_at(5, "old.db");

    let done = data(&run(&db, &["--now", "1800000000", "migrate"]));

    assert_eq!(done["changed"], true);
    assert_eq!(done["from"], 5);
    assert_eq!(done["to"], 9);
    let copy = scratch.file(
        done["safety_copy"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap(),
    );
    assert_eq!(
        sqlite(&copy, "SELECT MAX(version) FROM _sqlx_migrations"),
        "5"
    );
    assert_eq!(sqlite(&copy, "SELECT COUNT(*) FROM leads"), "1");
    assert_eq!(data(&run(&db, &["status"]))["counts"]["leads"], 1);
    assert!(
        !scratch
            .names()
            .iter()
            .any(|name| name.contains("migrating"))
    );
}

#[test]
fn a_failed_migration_leaves_the_original_and_keeps_the_copy() {
    let scratch = Scratch::new();
    let db = scratch.database_at(5, "old.db");
    sqlite(&db, "CREATE TABLE quote_numbers (blocking TEXT);");
    let bytes = fs::read(&db).unwrap();

    let (code, message) = failure(&run(&db, &["--now", "1800000000", "migrate"]), 3);

    assert_eq!(code, "migration_failed");
    assert_eq!(fs::read(&db).unwrap(), bytes);
    assert_eq!(
        sqlite(&db, "SELECT MAX(version) FROM _sqlx_migrations"),
        "5"
    );
    assert_eq!(failure(&run(&db, &["status"]), 3).0, "schema_outdated");

    let copy = scratch
        .names()
        .into_iter()
        .find(|name| name.contains("antes-de-migrar"))
        .expect("the copy stays beside the database");
    assert!(message.contains(&copy), "{message}");
    assert!(message.contains('5'), "{message}");
    assert_eq!(
        sqlite(&scratch.file(&copy), "SELECT COUNT(*) FROM leads"),
        "1"
    );
    assert!(
        !scratch
            .names()
            .iter()
            .any(|name| name.contains("migrating"))
    );
}

#[test]
fn a_rejected_write_on_a_current_database_leaves_the_file_unchanged() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let bytes = fs::read(&db).unwrap();

    assert_eq!(
        failure(&run(&db, &["lead", "advance", NO_ID]), 1).0,
        "lead_not_found"
    );

    assert_eq!(fs::read(&db).unwrap(), bytes);
    // SQLite may leave an empty WAL beside the file; content would be a write.
    let wal = fs::metadata(scratch.file("crm.db-wal")).map_or(0, |found| found.len());
    assert_eq!(wal, 0);
    assert_eq!(sqlite(&db, "PRAGMA journal_mode"), "wal");
}
