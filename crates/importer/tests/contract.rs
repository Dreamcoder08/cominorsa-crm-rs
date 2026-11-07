//! `cominorsa-import` follows the CLI contract: it never creates or upgrades a
//! database, and it checks its input files first. Runs the binary itself.

use std::{
    env, fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use cominorsa_domain::ProspectRepository;
use cominorsa_storage::SqliteLeads;
use uuid::Uuid;

const HEADER: &str = "titular,perfil_icp,n_concesiones,hectareas_totales,departamentos,provincias,sustancias,estados_concesion,servicio_potencial,revisar_manual,fuente_dato";
const ROW: &str = "Empresa Demo,PERFIL_1_formalizacion,3,120.5,PIURA,SULLANA,M,TITULADA,IGAFOM + REINFO,FALSE,Synthetic";
const TAX: &str = "titular,match,ruc,razon_social,estado,condicion,ubigeo,domicilio\nEmpresa Demo,unico,20123456786,EMPRESA DEMO,ACTIVO,HABIDO,200101,CAL UNO\n";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-import-contract-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("ok.csv"), format!("{HEADER}\n{ROW}\n")).unwrap();
        fs::write(
            path.join("people.csv"),
            format!("{HEADER}\n{}\n", ROW.replace("Empresa", "Persona")),
        )
        .unwrap();
        fs::write(path.join("tax.csv"), TAX).unwrap();
        fs::write(path.join("broken.csv"), "nombre\nA\n").unwrap();
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

    fn import(&self, db: &Path) -> Output {
        self.import_files(db, "ok.csv", "people.csv")
    }

    fn import_files(&self, db: &Path, companies: &str, people: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cominorsa-import"))
            .arg("--db")
            .arg(db)
            .arg(self.file(companies))
            .arg(self.file(people))
            .output()
            .unwrap()
    }

    fn enrich(&self, db: &Path, tax: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cominorsa-import"))
            .arg("--db")
            .arg(db)
            .arg("--sunat")
            .arg(self.file(tax))
            .output()
            .unwrap()
    }

    /// A new database in the rollback journal, so any write changes its bytes.
    async fn database(&self, name: &str) -> PathBuf {
        let db = self.file(name);
        SqliteLeads::open(&db).await.unwrap().close().await;
        sqlite(&db, "PRAGMA journal_mode = DELETE;");
        db
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

fn refusal(output: &Output, exit: i32) -> String {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn stdout(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

async fn prospects(db: &Path) -> usize {
    let store = SqliteLeads::open_read_only(db).await.unwrap();
    let count = store.count().await.unwrap();
    store.close().await;
    count
}

#[test]
fn an_absent_database_is_refused_with_init_guidance_and_stays_absent() {
    let scratch = Scratch::new();
    let db = scratch.file("absent.db");
    let names = scratch.names();

    for output in [scratch.import(&db), scratch.enrich(&db, "tax.csv")] {
        let message = refusal(&output, 3);

        assert!(message.contains("absent.db"), "{message}");
        assert!(message.contains("cominorsa init"), "{message}");
        assert_eq!(scratch.names(), names);
    }
}

#[tokio::test]
async fn an_older_schema_is_refused_with_the_file_unchanged() {
    let scratch = Scratch::new();
    let db = scratch.database("old.db").await;
    sqlite(
        &db,
        "ALTER TABLE leads DROP COLUMN origin; ALTER TABLE leads DROP COLUMN consent_given;
        ALTER TABLE leads DROP COLUMN consent_on; ALTER TABLE leads DROP COLUMN consent_source;
        DELETE FROM _sqlx_migrations WHERE version = 9;",
    );
    let bytes = fs::read(&db).unwrap();
    let names = scratch.names();

    for output in [scratch.import(&db), scratch.enrich(&db, "tax.csv")] {
        let message = refusal(&output, 3);

        assert!(message.contains("versión 8"), "{message}");
        assert!(message.contains("la 9"), "{message}");
        assert!(message.contains("cominorsa migrate"), "{message}");
        assert_eq!(fs::read(&db).unwrap(), bytes);
        assert_eq!(scratch.names(), names);
    }
}

#[tokio::test]
async fn a_newer_schema_is_refused_with_the_file_and_journal_unchanged() {
    let scratch = Scratch::new();
    let db = scratch.database("new.db").await;
    sqlite(
        &db,
        "INSERT INTO _sqlx_migrations
            SELECT 999, description, installed_on, success, checksum, execution_time
            FROM _sqlx_migrations WHERE version = 8;",
    );
    let bytes = fs::read(&db).unwrap();
    let names = scratch.names();

    for output in [scratch.import(&db), scratch.enrich(&db, "tax.csv")] {
        let message = refusal(&output, 3);

        assert!(message.contains("más nueva"), "{message}");
        assert!(message.contains("Actualiza el programa"), "{message}");
        assert_eq!(fs::read(&db).unwrap(), bytes);
        assert_eq!(scratch.names(), names);
        assert_eq!(sqlite(&db, "PRAGMA journal_mode"), "delete");
    }
}

#[tokio::test]
async fn invalid_input_wins_over_any_database_and_leaves_it_alone() {
    let scratch = Scratch::new();
    let absent = scratch.file("absent.db");
    let current = scratch.database("current.db").await;
    let bytes = fs::read(&current).unwrap();
    let names = scratch.names();

    for db in [&absent, &current] {
        let message = refusal(&scratch.import_files(db, "ok.csv", "broken.csv"), 1);
        assert_eq!(message, "Falta la columna titular\n");

        let message = refusal(&scratch.enrich(db, "broken.csv"), 1);
        assert_eq!(message, "Falta la columna titular\n");
    }

    assert_eq!(fs::read(&current).unwrap(), bytes);
    assert_eq!(scratch.names(), names);
}

#[tokio::test]
async fn a_current_database_imports_and_enriches_with_the_same_output() {
    let scratch = Scratch::new();
    let db = scratch.database("crm.db").await;

    let first = stdout(&scratch.import(&db));
    let mut lines = first.lines();
    assert_eq!(lines.next(), Some("Prospectos nuevos: 2"));
    assert_eq!(lines.next(), Some("Prospectos totales: 2"));
    let copy = lines
        .next()
        .and_then(|line| line.strip_prefix("Copia previa: "))
        .expect("the import reports its copy");
    assert!(Path::new(copy).exists(), "{copy}");
    assert_eq!(
        sqlite(Path::new(copy), "SELECT COUNT(*) FROM prospects"),
        "0"
    );
    assert_eq!(lines.next(), None);
    assert_eq!(prospects(&db).await, 2);

    let enriched = stdout(&scratch.enrich(&db, "tax.csv"));
    assert!(
        enriched.starts_with("Empresas actualizadas: 1\nCopia previa: "),
        "{enriched}"
    );
    assert_eq!(
        sqlite(&db, "SELECT ruc FROM prospects WHERE kind = 'company'"),
        "20123456786"
    );
}
