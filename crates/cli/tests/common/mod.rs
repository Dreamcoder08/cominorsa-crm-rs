//! Shared fixtures of the black-box contract tests. Old and new schemas are
//! built with the `sqlite3` tool, which the gate already requires.

#![allow(dead_code)] // Each test binary uses part of this module.

use std::io::Write as _;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::Value;
use uuid::Uuid;

pub const TODAY: &str = "2026-03-02";
pub const NO_ID: &str = "00000000-0000-0000-0000-000000000000";

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-contract-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A new, empty database, made the only way the CLI allows.
    pub fn database(&self) -> PathBuf {
        let db = self.file("crm.db");
        let created = run(&db, &["init"]);
        assert!(created.status.success(), "{created:?}");
        db
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A real database at schema `version`, holding one synthetic lead. Its
    /// migration records come from a new database, so `migrate` accepts it.
    pub fn database_at(&self, version: i64, name: &str) -> PathBuf {
        let fresh = self.file("fresh-for-fixture.db");
        let created = run(&fresh, &["init"]);
        assert!(created.status.success(), "{created:?}");
        let record = sqlite(
            &fresh,
            "SELECT sql FROM sqlite_master WHERE name = '_sqlx_migrations'",
        );
        let mut script = format!("{record};\n");

        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../storage/migrations");
        let mut migrations: Vec<PathBuf> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        migrations.sort();
        for migration in migrations {
            let file_name = migration
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let number: i64 = file_name[..4].parse().unwrap();
            if number <= version {
                script.push_str(&fs::read_to_string(&migration).unwrap());
                script.push('\n');
            }
        }
        script.push_str(&format!(
            "ATTACH '{}' AS fresh;
            INSERT INTO _sqlx_migrations
                SELECT * FROM fresh._sqlx_migrations WHERE version <= {version};
            DETACH fresh;
            INSERT INTO leads (id, name, phone, service, stage, follow_up_on)
                VALUES ('11111111-1111-7111-8111-111111111111', 'Lead Viejo',
                    '987650000', 'DAC', 'initial_contact', '2026-03-01');",
            fresh.display()
        ));

        let database = self.file(name);
        sqlite(&database, &script);
        fs::remove_file(&fresh).unwrap();
        for sidecar in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{sidecar}", fresh.display()));
        }
        database
    }

    /// A database that claims a schema newer than this program knows.
    pub fn newer_database(&self, name: &str) -> PathBuf {
        let database = self.database_at(8, name);
        sqlite(
            &database,
            "INSERT INTO _sqlx_migrations
                SELECT 999, description, installed_on, success, checksum, execution_time
                FROM _sqlx_migrations WHERE version = 8;",
        );
        database
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs a script through the `sqlite3` tool and returns its trimmed output.
pub fn sqlite(database: &Path, script: &str) -> String {
    let mut child = Command::new("sqlite3")
        .arg(database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the gate requires sqlite3");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cominorsa"));
    command.env_remove("COMINORSA_DB_PATH");
    command
}

/// Runs against `db` in JSON mode, on `day`.
pub fn run_on(db: &Path, day: &str, args: &[&str]) -> Output {
    binary()
        .arg("--db")
        .arg(db)
        .args(["--json", "--today", day])
        .args(args)
        .output()
        .unwrap()
}

/// Runs against `db` in JSON mode, at the fixed day.
pub fn run(db: &Path, args: &[&str]) -> Output {
    run_on(db, TODAY, args)
}

/// Runs from `folder`, to prove what a relative path would reach.
pub fn run_in(folder: &Path, db: &Path, args: &[&str]) -> Output {
    binary()
        .current_dir(folder)
        .arg("--db")
        .arg(db)
        .args(["--json", "--today", TODAY])
        .args(args)
        .output()
        .unwrap()
}

pub fn seeded(db: &Path, seed: &str, args: &[&str]) -> Output {
    binary()
        .arg("--db")
        .arg(db)
        .args(["--json", "--today", TODAY, "--id-seed", seed])
        .args(args)
        .output()
        .unwrap()
}

/// The `data` of a successful JSON reply, with stderr empty.
pub fn data(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    parsed["data"].clone()
}

/// The code and message of a failure, with stdout empty and this exit code.
pub fn failure(output: &Output, exit: i32) -> (String, String) {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let parsed: Value = serde_json::from_slice(&output.stderr).unwrap();
    (
        parsed["error"]["code"].as_str().unwrap().to_owned(),
        parsed["error"]["message"].as_str().unwrap().to_owned(),
    )
}

pub fn export(db: &Path) -> Value {
    data(&run(db, &["export"]))
}

/// Asserts that a command fails with `code`, leaving the exported state and
/// the file itself as they were.
pub fn refused(db: &Path, output: Output, exit: i32, code: &str, before: &Value) {
    assert_eq!(failure(&output, exit).0, code);
    assert_eq!(&export(db), before);
}

pub fn add_lead(db: &Path, name: &str, phone: &str, service: &str) -> String {
    let added = data(&run(
        db,
        &[
            "lead",
            "add",
            "--name",
            name,
            "--phone",
            phone,
            "--service",
            service,
            "--follow-up",
            "2026-03-20",
        ],
    ));
    added["lead"]["id"].as_str().unwrap().to_owned()
}

/// A lead on the stage before signing, so the next move opens its case.
pub fn lead_about_to_sign(db: &Path, name: &str, phone: &str, service: &str) -> String {
    let id = add_lead(db, name, phone, service);
    data(&run(db, &["lead", "advance", &id]));
    data(&run(db, &["lead", "advance", &id]));
    id
}

/// A signed lead's open case, as the JSON the move returned.
pub fn open_case(db: &Path, name: &str, phone: &str, service: &str) -> Value {
    let id = lead_about_to_sign(db, name, phone, service);
    data(&run(db, &["lead", "advance", &id]))["case"].clone()
}

pub fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
