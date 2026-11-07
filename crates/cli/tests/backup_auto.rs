//! Automatic backups through the CLI alone: when a copy is due, and what the
//! pruning may delete.

mod common;

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use common::{Scratch, data, export, failure, refused, run};
use serde_json::{Value, json};

const NOW: i64 = 1_772_470_800;
const DAY: i64 = 86_400;

fn at(db: &Path, now: i64, args: &[&str]) -> std::process::Output {
    let now = now.to_string();
    let mut all = vec!["--now", now.as_str()];
    all.extend_from_slice(args);
    run(db, &all)
}

fn set(db: &Path, args: &[&str]) {
    let mut all = vec!["settings", "set"];
    all.extend_from_slice(args);
    data(&run(db, &all));
}

/// Every file in `folder` with its bytes, to prove what changed.
fn contents(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(folder)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let bytes = fs::read(entry.path()).unwrap_or_default();
            (entry.file_name().to_string_lossy().into_owned(), bytes)
        })
        .collect()
}

fn folder_with_old_copies(scratch: &Scratch) -> PathBuf {
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    for (name, body) in [
        ("crm-20260101-090000.db", "copia vieja"),
        ("crm-20260201-090000.db", "copia reciente"),
        ("notas.txt", "no es una copia"),
        ("crm-viejo.db", "nombre parecido"),
        ("crm-20250101-090000.db.bak", "otra extensión"),
        ("otro-20250101-090000.db", "otro prefijo"),
        ("crm-2025010-090000.db", "fecha corta"),
    ] {
        fs::write(folder.join(name), body).unwrap();
    }
    fs::create_dir(folder.join("crm-20240101-090000.db")).unwrap();
    folder
}

#[test]
fn settings_take_an_interval_and_a_count_within_their_limits() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let shown = data(&run(&db, &["settings", "show"]));
    assert_eq!(shown["backup_interval_days"], 1);
    assert_eq!(shown["backup_keep"], 14);

    set(
        &db,
        &["--backup-interval-days", "30", "--backup-keep", "90"],
    );

    let exported = export(&db);
    assert_eq!(exported["settings"]["backup_interval_days"], 30);
    assert_eq!(exported["settings"]["backup_keep"], 90);
    for (args, code) in [
        (["--backup-interval-days", "0"], "invalid_backup_interval"),
        (["--backup-interval-days", "31"], "invalid_backup_interval"),
        (["--backup-keep", "0"], "invalid_backup_keep"),
        (["--backup-keep", "91"], "invalid_backup_keep"),
    ] {
        let mut all = vec!["settings", "set"];
        all.extend_from_slice(&args);
        refused(&db, run(&db, &all), 1, code, &exported);
    }
}

#[test]
fn without_a_folder_nothing_is_due_and_nothing_is_written() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let before = fs::read(&db).unwrap();
    let names = scratch.names();

    let found = data(&at(&db, NOW, &["backup", "auto"]));

    assert_eq!(found["status"], "not_configured");
    assert_eq!(found["path"], Value::Null);
    assert_eq!(fs::read(&db).unwrap(), before);
    assert_eq!(scratch.names(), names);
}

#[test]
fn a_first_copy_is_made_then_none_until_the_interval_passes() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    set(&db, &["--backup-directory", &folder.display().to_string()]);

    let first = data(&at(&db, NOW, &["backup", "auto"]));
    assert_eq!(first["status"], "created");
    let copy = folder.join("crm-20260302-120000.db");
    assert_eq!(first["path"], copy.display().to_string());
    assert!(copy.is_file());
    assert_eq!(export(&db)["settings"]["last_backup_at"], NOW);

    let before = export(&db);
    let again = data(&at(&db, NOW + DAY, &["backup", "auto"]));
    assert_eq!(again["status"], "not_due");
    assert_eq!(again["next_due_at"], NOW + DAY);
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    assert_eq!(export(&db), before);

    let later = data(&at(&db, NOW + DAY + 1, &["backup", "auto"]));
    assert_eq!(later["status"], "created");
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
}

#[test]
fn pruning_keeps_the_newest_copies_and_never_touches_other_files() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = folder_with_old_copies(&scratch);
    set(
        &db,
        &[
            "--backup-directory",
            &folder.display().to_string(),
            "--backup-keep",
            "2",
        ],
    );
    let mut expected = contents(&folder);
    expected.remove("crm-20260101-090000.db");

    let found = data(&at(&db, NOW, &["backup", "auto"]));

    assert_eq!(found["status"], "created");
    assert_eq!(
        found["pruned"],
        json!([folder.join("crm-20260101-090000.db").display().to_string()])
    );
    let mut after = contents(&folder);
    assert!(after.remove("crm-20260302-120000.db").is_some());
    assert_eq!(after, expected);
    assert!(folder.join("crm-20240101-090000.db").is_dir());
}

#[test]
fn a_dry_run_reports_the_copy_and_the_pruning_without_writing() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = folder_with_old_copies(&scratch);
    set(
        &db,
        &[
            "--backup-directory",
            &folder.display().to_string(),
            "--backup-keep",
            "1",
        ],
    );
    let files = contents(&folder);
    let before = fs::read(&db).unwrap();

    let found = data(&at(&db, NOW, &["backup", "auto", "--dry-run"]));

    assert_eq!(found["status"], "would_create");
    assert_eq!(found["dry_run"], true);
    assert_eq!(
        found["path"],
        folder.join("crm-20260302-120000.db").display().to_string()
    );
    assert_eq!(
        found["pruned"],
        json!([
            folder.join("crm-20260201-090000.db").display().to_string(),
            folder.join("crm-20260101-090000.db").display().to_string(),
        ])
    );
    assert_eq!(contents(&folder), files);
    assert_eq!(fs::read(&db).unwrap(), before);
}

#[test]
fn a_missing_folder_is_an_error_and_creates_nothing() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = scratch.file("no-montada");
    set(&db, &["--backup-directory", &folder.display().to_string()]);
    let before = export(&db);

    for args in [&["backup", "auto"][..], &["backup", "auto", "--dry-run"]] {
        let (code, message) = failure(&at(&db, NOW, args), 3);

        assert_eq!(code, "backup_folder_unavailable");
        assert!(message.contains("no-montada"), "{message}");
        assert!(!folder.exists());
        assert_eq!(export(&db), before);
    }
}

#[test]
fn an_unwritable_folder_is_an_error_and_deletes_nothing() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = folder_with_old_copies(&scratch);
    set(
        &db,
        &[
            "--backup-directory",
            &folder.display().to_string(),
            "--backup-keep",
            "1",
        ],
    );
    let files = contents(&folder);
    let before = export(&db);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();

    let output = at(&db, NOW, &["backup", "auto"]);

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    let (code, _) = failure(&output, 3);
    assert!(!code.is_empty());
    assert_eq!(contents(&folder), files);
    assert_eq!(export(&db), before);
}
