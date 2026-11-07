use std::{borrow::Cow, env, fs, path::PathBuf};

use cominorsa_domain::{
    CaseFile, CaseRepository, CaseStep, CaseStepRecord, Lead, LeadRepository, NewLead,
    ProspectRepository, RepositoryError, SettingsRepository, Stage, procedure_template,
};
use cominorsa_storage::SqliteLeads;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use time::{Date, macros::date};
use uuid::Uuid;

const SIGNED_ON: Date = date!(2026 - 11 - 07);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn new() -> Self {
        let dir = env::temp_dir().join(format!("cominorsa-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> PathBuf {
        self.0.join("crm.db")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A lead one step away from signing, and its advanced copy with the case.
fn signed(service: &str) -> (Lead, Lead, CaseFile) {
    let mut lead = Lead::create(NewLead {
        name: "Minera Ejemplo".to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: service.to_owned(),
        owner: Some("Ana".to_owned()),
        follow_up_on: date!(2026 - 11 - 10),
    })
    .unwrap();
    lead.advance().unwrap();
    lead.advance().unwrap();
    let quoted = lead.clone();
    lead.advance().unwrap();
    let opening = CaseFile::open(&lead, SIGNED_ON, &procedure_template(service)).unwrap();

    (quoted, lead, opening)
}

async fn raw_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
    .unwrap()
}

async fn raw_execute(path: &std::path::Path, statement: &'static str) {
    let pool = raw_pool(path).await;
    sqlx::query(statement).execute(&pool).await.unwrap();
    pool.close().await;
}

async fn raw_count(path: &std::path::Path, statement: &'static str) -> i64 {
    let pool = raw_pool(path).await;
    let count = sqlx::query_scalar(statement)
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    count
}

#[tokio::test]
async fn opens_the_case_and_saves_the_signed_lead_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (quoted, lead, opening) = signed("IGAFOM");
    store.save(&quoted).await.unwrap();

    let opened = store.sign_and_open(&lead, &opening).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert!(opened);
    assert_eq!(reopened.list().await.unwrap(), vec![lead]);
    assert_eq!(reopened.open_cases().await.unwrap(), vec![opening]);
}

#[tokio::test]
async fn opening_a_new_lead_directly_also_saves_it() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, lead, opening) = signed("DAC");

    store.sign_and_open(&lead, &opening).await.unwrap();

    assert_eq!(
        store.list().await.unwrap()[0].stage(),
        Stage::ContractSigned
    );
}

#[tokio::test]
async fn a_repeat_opens_no_second_case_and_keeps_edited_steps() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, lead, opening) = signed("REINFO");
    store.sign_and_open(&lead, &opening).await.unwrap();
    let mut edited = opening.steps[0].clone();
    edited.mark_done(SIGNED_ON);
    store.save_step(&edited).await.unwrap();

    let repeat = CaseFile::open(&lead, date!(2026 - 12 - 01), &procedure_template("REINFO"));
    let opened = store.sign_and_open(&lead, &repeat.unwrap()).await.unwrap();

    let cases = store.open_cases().await.unwrap();
    assert!(!opened);
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].case, opening.case);
    assert_eq!(cases[0].steps.len(), opening.steps.len());
    assert_eq!(cases[0].steps[0], edited);
}

#[tokio::test]
async fn a_failed_opening_leaves_the_lead_unsigned_and_no_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (quoted, lead, opening) = signed("DAC");
    store.save(&quoted).await.unwrap();
    raw_execute(
        &database.path(),
        "CREATE TRIGGER refuse_steps BEFORE INSERT ON case_steps
            WHEN NEW.position = 2 BEGIN SELECT RAISE(ABORT, 'refused'); END",
    )
    .await;

    let failed = store.sign_and_open(&lead, &opening).await;

    assert!(failed.is_err());
    assert_eq!(store.list().await.unwrap(), vec![quoted]);
    assert_eq!(store.open_cases().await.unwrap(), Vec::new());
    store.close().await;
    let path = database.path();
    assert_eq!(raw_count(&path, "SELECT COUNT(*) FROM cases").await, 0);
    assert_eq!(raw_count(&path, "SELECT COUNT(*) FROM case_steps").await, 0);
}

#[tokio::test]
async fn persists_step_completion_and_due_date_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, lead, opening) = signed("DAC");
    store.sign_and_open(&lead, &opening).await.unwrap();
    let mut done = opening.steps[0].clone();
    done.mark_done(SIGNED_ON);
    let mut moved = opening.steps[1].clone();
    moved.reschedule(date!(2027 - 01 - 15));
    store.save_step(&done).await.unwrap();
    store.save_step(&moved).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let steps = reopened.open_cases().await.unwrap().remove(0).steps;
    assert_eq!(steps[0].done_on(), Some(SIGNED_ON));
    assert_eq!(steps[1].due_on(), date!(2027 - 01 - 15));
    assert_eq!(steps[2..], opening.steps[2..]);

    done.mark_undone();
    reopened.save_step(&done).await.unwrap();
    let steps = reopened.open_cases().await.unwrap().remove(0).steps;
    assert_eq!(steps[0].done_on(), None);
}

#[tokio::test]
async fn refuses_to_save_a_step_that_does_not_exist() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let missing = CaseStep::restore(CaseStepRecord {
        id: Uuid::now_v7(),
        case_id: Uuid::now_v7(),
        position: 1,
        title: "Paso".to_owned(),
        due_on: SIGNED_ON,
        done_on: None,
    })
    .unwrap();

    let saved: Result<(), RepositoryError> = store.save_step(&missing).await;

    assert!(saved.is_err());
}

#[tokio::test]
async fn lists_only_open_cases_with_their_own_steps() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, first, first_opening) = signed("DAC");
    let (_, second, second_opening) = signed("ESTAMIN");
    store.sign_and_open(&first, &first_opening).await.unwrap();
    store.sign_and_open(&second, &second_opening).await.unwrap();
    assert_eq!(
        store.open_cases().await.unwrap(),
        vec![first_opening.clone(), second_opening]
    );

    raw_execute(
        &database.path(),
        "UPDATE cases SET status = 'closed' WHERE service = 'ESTAMIN'",
    )
    .await;

    assert_eq!(store.open_cases().await.unwrap(), vec![first_opening]);
}

#[tokio::test]
async fn refuses_to_return_a_corrupt_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, lead, opening) = signed("DAC");
    store.sign_and_open(&lead, &opening).await.unwrap();

    raw_execute(&database.path(), "UPDATE cases SET ruc = '123'").await;

    assert!(store.open_cases().await.is_err());
}

#[tokio::test]
async fn keeps_everything_when_upgrading_from_the_fifth_schema() {
    let database = TempDatabase::new();
    let old = raw_pool(&database.path()).await;
    let mut fifth_schema = sqlx::migrate!("./migrations");
    fifth_schema.migrations = Cow::Owned(
        fifth_schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= 5)
            .cloned()
            .collect(),
    );
    fifth_schema.run(&old).await.unwrap();
    for statement in [
        "INSERT INTO leads VALUES ('00000000-0000-7000-8000-000000000001', 'Firmado',
            '900000000', NULL, 'DAC', 'contract_signed', '2026-11-01', 'Ana')",
        "INSERT INTO leads VALUES ('00000000-0000-7000-8000-000000000002', 'Cotizado',
            '900000001', '20123456786', 'REINFO', 'quote_sent', '2026-11-02', NULL)",
        "INSERT INTO prospects (kind, name, profile, concessions, hectares, departments,
            provinces, substances, concession_states, potential_service, needs_review, source,
            ruc, tax_status, tax_condition, fiscal_address, tax_match_approximate)
            VALUES ('company', 'Titular', 'OTRO_estado', 1, 2.5, 'PIURA', 'SULLANA', 'M',
            'TITULADA', 'DAC', 0, 'Synthetic', '20123456786', 'ACTIVO', 'HABIDO', 'Calle 1', 0)",
        "INSERT INTO settings VALUES ('follow_up_days', '9')",
        "INSERT INTO team_members VALUES (0, 'Ana')",
    ] {
        sqlx::query(statement).execute(&old).await.unwrap();
    }
    old.close().await;

    let upgraded = SqliteLeads::open(&database.path()).await.unwrap();

    let leads = upgraded.list().await.unwrap();
    assert_eq!(leads.len(), 2);
    assert_eq!(leads[0].name(), "Firmado");
    assert_eq!(leads[0].stage(), Stage::ContractSigned);
    assert_eq!(leads[0].owner(), Some("Ana"));
    assert_eq!(leads[1].stage(), Stage::QuoteSent);
    assert_eq!(leads[1].ruc().map(|ruc| ruc.as_str()), Some("20123456786"));
    let prospects = upgraded.search("", 10).await.unwrap();
    assert_eq!(prospects.len(), 1);
    assert_eq!(prospects[0].tax.as_ref().unwrap().status, "ACTIVO");
    let settings = upgraded.load().await.unwrap();
    assert_eq!(settings.follow_up_days(), 9);
    assert_eq!(settings.team(), ["Ana"]);
    assert_eq!(upgraded.open_cases().await.unwrap(), Vec::new());

    let mut quoted = leads[1].clone();
    quoted.advance().unwrap();
    let opening = CaseFile::open(&quoted, SIGNED_ON, &procedure_template("REINFO")).unwrap();
    assert!(upgraded.sign_and_open(&quoted, &opening).await.unwrap());
    assert_eq!(upgraded.open_cases().await.unwrap(), vec![opening]);
}

#[tokio::test]
async fn restores_cases_from_a_backup() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, lead, opening) = signed("DAC");
    store.sign_and_open(&lead, &opening).await.unwrap();
    let backup = database.0.join("copia.db");
    let safety = database.0.join("seguridad.db");
    store.backup_to(&backup).await.unwrap();
    raw_execute(&database.path(), "DELETE FROM case_steps").await;
    raw_execute(&database.path(), "DELETE FROM cases").await;

    let restored = store
        .restore(&database.path(), &backup, &safety)
        .await
        .unwrap();

    assert_eq!(restored.open_cases().await.unwrap(), vec![opening]);
}

#[tokio::test]
async fn refuses_to_restore_a_backup_with_a_corrupt_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, lead, opening) = signed("DAC");
    store.sign_and_open(&lead, &opening).await.unwrap();
    let backup = database.0.join("copia.db");
    let safety = database.0.join("seguridad.db");
    store.backup_to(&backup).await.unwrap();
    raw_execute(&backup, "UPDATE cases SET ruc = '123'").await;
    let current = store.clone();

    let restored = store.restore(&database.path(), &backup, &safety).await;

    assert!(restored.is_err());
    assert!(!safety.exists());
    assert_eq!(current.open_cases().await.unwrap(), vec![opening]);
}
