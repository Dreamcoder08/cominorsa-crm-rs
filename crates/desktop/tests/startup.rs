//! Startup upgrades through the path `cominorsa migrate` proves: a copy first,
//! a migrated working copy, and a rename, so a failure leaves the file intact.
//! Older databases are built with the `sqlite3` tool, as in the CLI contract.

use std::{
    env, fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use cominorsa_desktop::{initial_settings, open_database};
use cominorsa_domain::{LeadRepository, ThemePreference};
use uuid::Uuid;

const CURRENT_SCHEMA: &str = "9";
const SCHEMA_VERSION: &str = "SELECT MAX(version) FROM _sqlx_migrations";
const COPY_MARK: &str = "antes-de-migrar";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-startup-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn copies(&self) -> Vec<PathBuf> {
        self.names()
            .iter()
            .filter(|name| name.contains(COPY_MARK))
            .map(|name| self.file(name))
            .collect()
    }

    /// A database at schema `version` holding one synthetic lead and the dark
    /// theme. Its migration records come from a new database.
    async fn database_at(&self, version: i64) -> PathBuf {
        let fresh = self.file("fresh-for-fixture.db");
        open_database(&fresh).await.unwrap().close().await;
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
            let name = migration.file_name().unwrap().to_string_lossy();
            if name[..4].parse::<i64>().unwrap() <= version {
                script.push_str(&fs::read_to_string(&migration).unwrap());
                script.push('\n');
            }
        }
        script.push_str(&format!(
            "ATTACH '{}' AS fresh;
            INSERT INTO _sqlx_migrations
                SELECT * FROM fresh._sqlx_migrations WHERE version <= {version};
            DETACH fresh;
            INSERT INTO settings (key, value) VALUES ('theme', 'dark');
            INSERT INTO leads (id, name, phone, service, stage, follow_up_on)
                VALUES ('11111111-1111-7111-8111-111111111111', 'Lead Viejo',
                    '987650000', 'DAC', 'initial_contact', '2026-03-01');",
            fresh.display()
        ));

        let database = self.file("crm.db");
        sqlite(&database, &script);
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", fresh.display()));
        }
        database
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sqlite(database: &Path, script: &str) -> String {
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

#[tokio::test]
async fn an_older_database_is_upgraded_keeping_a_copy_and_its_rows() {
    let scratch = Scratch::new();
    let db = scratch.database_at(5).await;

    let store = open_database(&db).await.unwrap();
    let leads = store.list().await.unwrap();
    store.close().await;

    assert_eq!(leads.len(), 1);
    assert_eq!(leads[0].name(), "Lead Viejo");
    assert_eq!(sqlite(&db, SCHEMA_VERSION), CURRENT_SCHEMA);
    let copies = scratch.copies();
    assert_eq!(copies.len(), 1, "{:?}", scratch.names());
    assert_eq!(sqlite(&copies[0], SCHEMA_VERSION), "5");
    assert_eq!(sqlite(&copies[0], "SELECT COUNT(*) FROM leads"), "1");
    assert!(
        !scratch
            .names()
            .iter()
            .any(|name| name.contains("migrating"))
    );
}

#[tokio::test]
async fn a_failed_upgrade_leaves_the_database_and_names_its_copy() {
    let scratch = Scratch::new();
    let db = scratch.database_at(5).await;
    sqlite(&db, "CREATE TABLE quote_numbers (blocking TEXT);");
    let bytes = fs::read(&db).unwrap();

    let message = open_database(&db)
        .await
        .err()
        .expect("the blocked migration must fail");

    assert_eq!(fs::read(&db).unwrap(), bytes);
    assert_eq!(sqlite(&db, SCHEMA_VERSION), "5");
    let copies = scratch.copies();
    assert_eq!(copies.len(), 1, "{:?}", scratch.names());
    assert!(
        message.contains(&copies[0].display().to_string()),
        "{message}"
    );
    assert!(message.contains("versión 5"), "{message}");
    assert!(message.contains("quote_numbers"), "{message}");
    assert_eq!(sqlite(&copies[0], "SELECT COUNT(*) FROM leads"), "1");
    assert!(
        !scratch
            .names()
            .iter()
            .any(|name| name.contains("migrating"))
    );
}

#[tokio::test]
async fn a_first_launch_creates_a_new_database() {
    let scratch = Scratch::new();
    let db = scratch.file("nested/data/crm.db");

    let store = open_database(&db).await.unwrap();
    let leads = store.list().await.unwrap();
    store.close().await;

    assert!(leads.is_empty());
    assert_eq!(sqlite(&db, SCHEMA_VERSION), CURRENT_SCHEMA);
    assert!(scratch.copies().is_empty());
}

#[tokio::test]
async fn a_current_database_opens_without_a_copy() {
    let scratch = Scratch::new();
    let db = scratch.database_at(9).await;

    let store = open_database(&db).await.unwrap();
    assert_eq!(store.list().await.unwrap().len(), 1);
    store.close().await;

    assert!(scratch.copies().is_empty());
}

#[tokio::test]
async fn a_newer_database_is_refused_untouched() {
    let scratch = Scratch::new();
    let db = scratch.database_at(8).await;
    sqlite(
        &db,
        "INSERT INTO _sqlx_migrations
            SELECT 999, description, installed_on, success, checksum, execution_time
            FROM _sqlx_migrations WHERE version = 8;",
    );
    let bytes = fs::read(&db).unwrap();

    let message = open_database(&db)
        .await
        .err()
        .expect("a newer schema must be refused");

    assert!(message.contains("Actualiza el programa"), "{message}");
    assert_eq!(fs::read(&db).unwrap(), bytes);
    assert!(scratch.copies().is_empty());
}

#[tokio::test]
async fn the_theme_read_before_the_window_never_upgrades() {
    let scratch = Scratch::new();
    let db = scratch.database_at(5).await;
    let bytes = fs::read(&db).unwrap();

    let path = db.clone();
    let settings = tokio::task::spawn_blocking(move || initial_settings(&path))
        .await
        .unwrap();

    assert_eq!(settings.theme, ThemePreference::Dark);
    assert_eq!(fs::read(&db).unwrap(), bytes);
    assert!(scratch.copies().is_empty());
}

#[tokio::test]
async fn the_theme_read_before_the_window_creates_nothing() {
    let scratch = Scratch::new();
    let db = scratch.file("crm.db");

    let path = db.clone();
    let settings = tokio::task::spawn_blocking(move || initial_settings(&path))
        .await
        .unwrap();

    assert_eq!(settings.theme, ThemePreference::default());
    assert!(!db.exists());
}
