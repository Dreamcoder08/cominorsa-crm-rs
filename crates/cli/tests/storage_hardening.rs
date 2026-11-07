//! Storage hardening, proven through the CLI with a second process: `migrate`
//! never races another connection, its working copy never collides with a
//! user file, one corrupt consent row is tolerated, and an ordinary open keeps
//! the journal mode.

mod common;

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

use common::{NO_ID, Scratch, add_lead, data, export, failure, run, sqlite};
use serde_json::Value;

/// A `sqlite3` process that keeps the database open between statements, as
/// another program would.
struct Shell {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    errors: PathBuf,
    seen: usize,
}

impl Shell {
    fn open(database: &Path, errors: PathBuf) -> Self {
        let mut child = Command::new("sqlite3")
            .arg(database)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(&errors).unwrap())
            .spawn()
            .expect("the gate requires sqlite3");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            errors,
            seen: 0,
        }
    }

    /// Runs `sql` and waits until it finished; an error is what `sqlite3` said.
    fn run(&mut self, sql: &str) -> Result<(), String> {
        writeln!(self.stdin, "{sql}\nSELECT 'listo';").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(
                self.stdout.read_line(&mut line).unwrap(),
                0,
                "sqlite3 ended"
            );
            if line.trim() == "listo" {
                break;
            }
        }
        let errors = fs::read_to_string(&self.errors).unwrap();
        let new = errors[self.seen..].to_owned();
        self.seen = errors.len();
        if new.is_empty() { Ok(()) } else { Err(new) }
    }

    /// Ends the process; a refused statement already showed in `run`.
    fn quit(mut self) {
        drop(self.stdin);
        self.child.wait().unwrap();
    }
}

const NEW_LEAD: &str = "INSERT INTO leads (id, name, phone, service, stage, follow_up_on)
    VALUES ('22222222-2222-7222-8222-222222222222', 'Escrito por otro', '987650001', 'DAC',
    'initial_contact', '2026-03-05');";

/// An older database in the journal mode the app uses.
fn older_wal_database(scratch: &Scratch) -> PathBuf {
    let db = scratch.database_at(8, "old.db");
    assert_eq!(sqlite(&db, "PRAGMA journal_mode = WAL;"), "wal");
    db
}

/// A fingerprint of the file's bytes, so a failure does not print them all.
fn digest(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    fs::read(path).unwrap().hash(&mut hasher);
    hasher.finish()
}

fn lead_names(db: &Path) -> Vec<String> {
    export(db)["leads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|lead| lead["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn migrate_refuses_while_another_program_has_the_database_open() {
    // WAL: an idle connection holds the file. Rollback journal: only a
    // transaction does, so the other program is inside one.
    for (mode, start) in [
        ("wal", "SELECT COUNT(*) FROM leads;"),
        ("delete", "BEGIN IMMEDIATE;"),
    ] {
        let scratch = Scratch::new();
        let db = scratch.database_at(8, "old.db");
        assert_eq!(sqlite(&db, &format!("PRAGMA journal_mode = {mode};")), mode);
        let mut other = Shell::open(&db, scratch.file("other.err"));
        other.run(start).unwrap();
        let bytes = digest(&db);
        let names = scratch.names();

        let (code, message) = failure(&run(&db, &["migrate"]), 3);

        assert_eq!(code, "database_busy", "{mode}");
        assert!(message.contains("cominorsa migrate"), "{message}");
        assert_eq!(digest(&db), bytes, "{mode}");
        assert_eq!(scratch.names(), names, "{mode}");

        other.run(NEW_LEAD).unwrap();
        if mode == "delete" {
            other.run("COMMIT;").unwrap();
        }
        other.quit();
        assert_eq!(data(&run(&db, &["migrate"]))["changed"], true, "{mode}");
        assert_eq!(
            lead_names(&db),
            ["Lead Viejo", "Escrito por otro"],
            "{mode}"
        );
    }
}

#[test]
fn a_write_waiting_on_migrate_is_refused_or_kept() {
    let scratch = Scratch::new();
    let db = scratch.database_at(8, "old.db");
    // A rollback-journal connection that is idle holds no lock, so `migrate`
    // cannot see it; its next write must not reach a replaced file.
    let mut other = Shell::open(&db, scratch.file("other.err"));
    other.run("SELECT COUNT(*) FROM leads;").unwrap();

    let migrated = run(&db, &["migrate"]);
    let written = other.run(NEW_LEAD);
    other.quit();

    match migrated.status.code() {
        Some(0) => {
            let refusal = written.expect_err("a write to the replaced file must fail");
            assert!(refusal.contains("readonly"), "{refusal}");
            assert_eq!(lead_names(&db), ["Lead Viejo"]);
        }
        _ => {
            assert_eq!(failure(&migrated, 3).0, "database_busy");
            written.unwrap();
            assert_eq!(data(&run(&db, &["migrate"]))["changed"], true);
            assert_eq!(lead_names(&db), ["Lead Viejo", "Escrito por otro"]);
        }
    }
}

#[test]
fn writes_racing_migrate_are_refused_or_kept() {
    let scratch = Scratch::new();
    let db = older_wal_database(&scratch);
    let binary = env!("CARGO_BIN_EXE_cominorsa");
    let mut migrate = Command::new(binary)
        .env_remove("COMINORSA_DB_PATH")
        .arg("--db")
        .arg(&db)
        .args(["--json", "--today", common::TODAY, "migrate"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Each write is its own process with a short wait, so some land before,
    // some during, and some after the upgrade.
    let mut kept = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut index = 0;
    while migrate.try_wait().unwrap().is_none() || index < 5 {
        assert!(Instant::now() < deadline, "migrate did not end");
        let id = format!("33333333-3333-7333-8333-{index:012}");
        let insert = Command::new("sqlite3")
            .args(["-cmd", ".timeout 20"])
            .arg(&db)
            .arg(format!(
                "INSERT INTO leads (id, name, phone, service, stage, follow_up_on)
                VALUES ('{id}', 'Carrera {index}', '987650002', 'DAC', 'initial_contact',
                '2026-03-05');"
            ))
            .output()
            .unwrap();
        if insert.status.success() && insert.stderr.is_empty() {
            kept.push(format!("Carrera {index}"));
        }
        index += 1;
    }
    let migrated = migrate.wait_with_output().unwrap();

    if migrated.status.success() {
        assert_eq!(data(&migrated)["changed"], true);
    } else {
        assert_eq!(failure(&migrated, 3).0, "database_busy");
        data(&run(&db, &["migrate"]));
    }
    let names = lead_names(&db);
    for name in &kept {
        assert!(names.contains(name), "{name} was written and then lost");
    }
    assert!(names.contains(&"Lead Viejo".to_owned()));
}

#[test]
fn migrate_never_touches_a_file_with_the_old_working_name() {
    let scratch = Scratch::new();
    let db = scratch.database_at(8, "old.db");
    let user_files = ["old.migrating.db", "old.migrating.db-wal"];
    for name in user_files {
        fs::write(scratch.file(name), format!("archivo del usuario: {name}")).unwrap();
    }

    let done = data(&run(&db, &["--now", "1800000000", "migrate"]));

    assert_eq!(done["changed"], true);
    for name in user_files {
        assert_eq!(
            fs::read_to_string(scratch.file(name)).unwrap(),
            format!("archivo del usuario: {name}")
        );
    }
    let copy = Path::new(done["safety_copy"].as_str().unwrap())
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut expected = vec!["old.db".to_owned(), copy];
    expected.extend(user_files.map(str::to_owned));
    expected.sort();
    assert_eq!(scratch.names(), expected);
}

#[test]
fn a_failed_migration_never_touches_a_file_with_the_old_working_name() {
    let scratch = Scratch::new();
    let db = scratch.database_at(8, "old.db");
    sqlite(&db, "ALTER TABLE leads ADD COLUMN origin TEXT;");
    fs::write(scratch.file("old.migrating.db"), "archivo del usuario").unwrap();
    let bytes = digest(&db);

    assert_eq!(failure(&run(&db, &["migrate"]), 3).0, "migration_failed");

    assert_eq!(digest(&db), bytes);
    assert_eq!(
        fs::read_to_string(scratch.file("old.migrating.db")).unwrap(),
        "archivo del usuario"
    );
    let names = scratch.names();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names.iter().any(|name| name.contains("antes-de-migrar")));
}

#[test]
fn one_corrupt_consent_row_leaves_every_lead_readable() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let broken = add_lead(&db, "Consentimiento roto", "987650003", "DAC");
    let fine = add_lead(&db, "Lead sano", "987650004", "DAC");
    data(&run(
        &db,
        &["lead", "consent", &fine, "--source", "Aceptó por llamada"],
    ));
    sqlite(
        &db,
        &format!(
            "UPDATE leads SET consent_given = 1, consent_on = 'fecha-secreta-rota',
                consent_source = 'fuente' WHERE id = '{broken}';"
        ),
    );

    let listed = run(&db, &["lead", "list"]);
    let leads = data(&listed)["leads"].as_array().unwrap().clone();
    assert_eq!(leads.len(), 2);
    let shown = data(&run(&db, &["lead", "show", &broken]));
    assert_eq!(shown["lead"]["consent"], Value::Null);
    let healthy = data(&run(&db, &["lead", "show", &fine]));
    assert_eq!(healthy["lead"]["consent"]["source"], "Aceptó por llamada");

    let status = run(&db, &["status"]);
    assert_eq!(data(&status)["unreadable_rows"]["lead_consents"], 1);
    let exported = export(&db);
    assert_eq!(exported["unreadable_rows"]["lead_consents"], 1);
    assert_eq!(exported["leads"].as_array().unwrap().len(), 2);
    let human = human_output(&db, &["status"]);
    assert!(human.contains("Consentimientos ilegibles: 1"), "{human}");
    for output in [&listed.stdout, &status.stdout, human.as_bytes()] {
        assert!(!String::from_utf8_lossy(output).contains("fecha-secreta-rota"));
    }

    let clean = scratch.file("clean.db");
    data(&run(&clean, &["init"]));
    assert_eq!(
        data(&run(&clean, &["status"]))["unreadable_rows"]["lead_consents"],
        0
    );
}

fn human_output(db: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_cominorsa"))
        .env_remove("COMINORSA_DB_PATH")
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn an_ordinary_open_keeps_the_rollback_journal() {
    let scratch = Scratch::new();
    let db = scratch.database();
    assert_eq!(sqlite(&db, "PRAGMA journal_mode = DELETE;"), "delete");
    let bytes = digest(&db);
    let names = scratch.names();

    assert_eq!(
        failure(&run(&db, &["lead", "advance", NO_ID]), 1).0,
        "lead_not_found"
    );

    assert_eq!(digest(&db), bytes);
    assert_eq!(scratch.names(), names);
    assert_eq!(sqlite(&db, "PRAGMA journal_mode"), "delete");

    add_lead(&db, "Lead en modo clásico", "987650005", "DAC");
    assert_eq!(sqlite(&db, "PRAGMA journal_mode"), "delete");
    assert_eq!(lead_names(&db), ["Lead en modo clásico"]);
}

#[test]
fn migrate_moves_an_older_rollback_journal_database_to_wal() {
    let scratch = Scratch::new();
    let db = scratch.database_at(8, "old.db");
    assert_eq!(sqlite(&db, "PRAGMA journal_mode"), "delete");

    data(&run(&db, &["migrate"]));

    assert_eq!(sqlite(&db, "PRAGMA journal_mode"), "wal");
}

#[test]
fn restore_never_touches_a_file_with_the_old_working_name() {
    let scratch = Scratch::new();
    let db = scratch.database();
    add_lead(&db, "Lead respaldado", "987650006", "DAC");
    let folder = scratch.file("copias");
    fs::create_dir_all(&folder).unwrap();
    let backup = data(&run(
        &db,
        &["backup", "create", "--folder", folder.to_str().unwrap()],
    ))["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let junk = scratch.file("junk.db");
    fs::write(&junk, "no soy una copia").unwrap();
    let user_files = ["crm.restoring.db", "crm.restoring.db-wal"];
    for name in user_files {
        fs::write(scratch.file(name), format!("archivo del usuario: {name}")).unwrap();
    }

    let refused = run(
        &db,
        &["backup", "restore", "--file", junk.to_str().unwrap()],
    );
    assert_eq!(failure(&refused, 3).0, "storage_error");
    let restored = data(&run(&db, &["backup", "restore", "--file", &backup]));

    for name in user_files {
        assert_eq!(
            fs::read_to_string(scratch.file(name)).unwrap(),
            format!("archivo del usuario: {name}")
        );
    }
    assert!(Path::new(restored["safety_copy"].as_str().unwrap()).exists());
    assert_eq!(lead_names(&db), ["Lead respaldado"]);
    assert!(
        !scratch
            .names()
            .iter()
            .any(|name| name.contains("restaurando") || name.contains("migrando")),
        "{:?}",
        scratch.names()
    );
}
