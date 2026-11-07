//! Automatic backups through the CLI alone: when a copy is due, and what the
//! pruning may delete.

mod common;

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use common::{Scratch, data, export, failure, refused, run, run_in, run_on};
use serde_json::{Value, json};

const NOW: i64 = 1_772_470_800;
const HOUR: i64 = 3_600;

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
    // 23:00 in Peru on the same day: the copy becomes due at midnight.
    let again = data(&at(&db, NOW + 11 * HOUR, &["backup", "auto"]));
    assert_eq!(again["status"], "not_due");
    assert_eq!(again["next_due_at"], NOW + 12 * HOUR);
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    assert_eq!(export(&db), before);

    let later = data(&run_on(&db, "2026-03-03", &["backup", "auto"]));
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
    let database = fs::read(&db).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();

    let real = at(&db, NOW, &["backup", "auto"]);
    let dry = at(&db, NOW, &["backup", "auto", "--dry-run"]);
    let manual = at(&db, NOW, &["backup", "create"]);

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    for output in [&real, &dry] {
        let (code, message) = failure(output, 3);
        assert_eq!(code, "backup_folder_unavailable");
        assert!(message.contains("copias"), "{message}");
    }
    // `backup create` keeps reporting its own failure as before.
    assert_eq!(failure(&manual, 3).0, "storage_error");
    assert_eq!(contents(&folder), files);
    assert_eq!(fs::read(&db).unwrap(), database);
    assert_eq!(export(&db), before);
}

/// The names in `folder`, sorted, to compare before and after.
fn names(folder: &Path) -> Vec<String> {
    contents(folder).into_keys().collect()
}

fn hash(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

fn keep_one_in(db: &Path, folder: &Path) {
    set(
        db,
        &[
            "--backup-directory",
            &folder.display().to_string(),
            "--backup-keep",
            "1",
        ],
    );
}

#[test]
fn pruning_never_deletes_the_live_database_named_like_a_copy() {
    let scratch = Scratch::new();
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    let db = folder.join("crm-20200101-000000.db");
    data(&run(&db, &["init"]));
    keep_one_in(&db, &folder);
    let before = names(&folder);

    let dry = data(&at(&db, NOW, &["backup", "auto", "--dry-run"]));
    let found = data(&at(&db, NOW, &["backup", "auto"]));

    assert_eq!(found["status"], "created");
    assert_eq!(found["pruned"], json!([]));
    assert_eq!(dry["pruned"], found["pruned"]);
    assert_eq!(dry["path"], found["path"]);
    let mut expected = before;
    expected.push("crm-20260302-120000.db".to_owned());
    expected.sort();
    assert_eq!(names(&folder), expected);
    assert_eq!(export(&db)["settings"]["last_backup_at"], NOW);
}

#[test]
fn pruning_never_deletes_a_hard_link_to_the_live_database() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    let link = folder.join("crm-20200102-000000.db");
    fs::write(folder.join("crm-20200101-000000.db"), "copia vieja").unwrap();
    keep_one_in(&db, &folder);
    fs::hard_link(&db, &link).unwrap();

    let dry = data(&at(&db, NOW, &["backup", "auto", "--dry-run"]));
    let found = data(&at(&db, NOW, &["backup", "auto"]));

    let old = json!([folder.join("crm-20200101-000000.db").display().to_string()]);
    assert_eq!(found["pruned"], old);
    assert_eq!(dry["pruned"], old);
    assert_eq!(
        names(&folder),
        ["crm-20200102-000000.db", "crm-20260302-120000.db"]
    );
    assert_eq!(hash(&link), hash(&db));
    assert_eq!(export(&db)["settings"]["last_backup_at"], NOW);
}

#[test]
fn a_copy_made_in_this_run_survives_copies_stamped_in_the_future() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    for name in [
        "crm-20990101-000000.db",
        "crm-20260201-090000.db",
        // Not a real instant, so never one of this app's copies.
        "crm-20250195-000095.db",
    ] {
        fs::write(folder.join(name), name).unwrap();
    }
    set(
        &db,
        &[
            "--backup-directory",
            &folder.display().to_string(),
            "--backup-keep",
            "1",
        ],
    );
    let before = contents(&folder);
    let database = hash(&db);
    let state = export(&db);

    let dry = data(&at(&db, NOW, &["backup", "auto", "--dry-run"]));

    assert_eq!(contents(&folder), before);
    assert_eq!(hash(&db), database);
    assert_eq!(export(&db), state);
    let found = data(&at(&db, NOW, &["backup", "auto"]));

    let copy = folder.join("crm-20260302-120000.db");
    assert_eq!(found["path"], copy.display().to_string());
    assert_eq!(dry["path"], found["path"]);
    assert_eq!(
        found["pruned"],
        json!([folder.join("crm-20260201-090000.db").display().to_string()])
    );
    assert_eq!(dry["pruned"], found["pruned"]);
    let mut expected = before;
    expected.remove("crm-20260201-090000.db");
    let mut after = contents(&folder);
    assert!(after.remove("crm-20260302-120000.db").is_some());
    assert_eq!(after, expected);
    assert!(copy.is_file());
}

#[test]
fn a_daily_opening_backs_up_every_calendar_day_in_peru() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    set(&db, &["--backup-directory", &folder.display().to_string()]);

    for (day, status) in [
        ("2026-03-01", "created"),
        ("2026-03-01", "not_due"),
        ("2026-03-02", "created"),
        ("2026-03-04", "created"),
    ] {
        let found = data(&run_on(&db, day, &["backup", "auto"]));
        assert_eq!(found["status"], status, "{day}");
    }
    assert_eq!(
        names(&folder),
        [
            "crm-20260301-120000.db",
            "crm-20260302-120000.db",
            "crm-20260304-120000.db"
        ]
    );

    // 18:00 one evening, then 09:00 the next morning: fifteen hours, a new day.
    let evening = "1772751600";
    let morning = "1772805600";
    let late = data(&run_on(
        &db,
        "2026-03-05",
        &["--now", evening, "backup", "auto"],
    ));
    assert_eq!(late["status"], "created");
    let next = data(&run_on(
        &db,
        "2026-03-06",
        &["--now", morning, "backup", "auto", "--dry-run"],
    ));
    assert_eq!(next["status"], "would_create");
}

#[test]
fn a_relative_folder_is_refused_and_never_resolved_against_the_working_folder() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let working = scratch.file("trabajo");
    let reached = working.join("copias");
    fs::create_dir_all(&reached).unwrap();
    for name in ["crm-20000101-000000.db", "crm-20000102-000000.db"] {
        fs::write(reached.join(name), name).unwrap();
    }
    set(&db, &["--backup-directory", "copias", "--backup-keep", "1"]);
    let files = contents(&reached);
    let state = export(&db);
    let database = hash(&db);

    for args in [&["backup", "auto"][..], &["backup", "auto", "--dry-run"]] {
        let (code, message) = failure(&run_in(&working, &db, args), 1);

        assert_eq!(code, "backup_folder_not_absolute");
        assert!(message.contains("copias"), "{message}");
        assert_eq!(contents(&reached), files);
        assert_eq!(hash(&db), database);
        assert_eq!(export(&db), state);
    }
}
