//! Read-only access: nothing here may change the file it looks at.

use std::{borrow::Cow, env, fs, path::PathBuf};

use cominorsa_domain::{
    BillingMilestone, BillingRepository, CaseClosure, CaseFile, CaseRepository, IgvRate, Lead,
    LeadRepository, Money, NewLead, NewMilestone, QuoteContent, QuoteDraft, QuoteLine,
    QuoteRepository, Settings, SettingsRepository, Stage, procedure_template,
};
use cominorsa_storage::{QuoteCounter, SqliteLeads};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use time::{Date, macros::date};
use uuid::Uuid;

const OPENED_ON: Date = date!(2026 - 03 - 02);

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn new() -> Self {
        let dir = env::temp_dir().join(format!("cominorsa-inspect-{}", Uuid::now_v7()));
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

async fn raw_pool(path: &PathBuf) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    SqlitePool::connect_with(options).await.unwrap()
}

/// Leaves a database at `version`, as an older release would have.
async fn database_at(path: &PathBuf, version: i64) {
    let old = raw_pool(path).await;
    let mut schema = sqlx::migrate!("./migrations");
    schema.migrations = Cow::Owned(
        schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= version)
            .cloned()
            .collect(),
    );
    schema.run(&old).await.unwrap();
    old.close().await;
}

fn signed_lead(service: &str) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: "Minera Sintética".to_owned(),
        phone: "987654321".to_owned(),
        ruc: None,
        service: service.to_owned(),
        owner: None,
        follow_up_on: OPENED_ON,
    })
    .unwrap();
    while lead.stage() != Stage::ContractSigned {
        lead.advance().unwrap();
    }
    lead
}

async fn opened(store: &SqliteLeads, service: &str) -> (Lead, CaseFile) {
    let lead = signed_lead(service);
    let file = CaseFile::open(&lead, OPENED_ON, &procedure_template(service)).unwrap();
    store.sign_and_open(&lead, &file).await.unwrap();

    (lead, file)
}

fn quote_draft(lead: &Lead, issued_on: Date) -> QuoteDraft {
    QuoteDraft::for_lead(
        lead,
        QuoteContent {
            issued_on,
            valid_until: issued_on,
            lines: vec![QuoteLine::new("Estudio", 1, Money::from_cents(100).unwrap()).unwrap()],
            notes: String::new(),
        },
        IgvRate::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_current_database_reports_the_required_schema_version() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();

    assert_eq!(store.schema_version().await.unwrap(), 9);
    assert_eq!(SqliteLeads::required_schema_version(), 9);
}

#[tokio::test]
async fn opening_to_read_leaves_an_older_schema_as_it_is() {
    let database = TempDatabase::new();
    database_at(&database.path(), 5).await;
    let before = fs::read(database.path()).unwrap();

    let reader = SqliteLeads::open_read_only(&database.path()).await.unwrap();
    let version = reader.schema_version().await.unwrap();
    reader.close().await;

    assert_eq!(version, 5);
    assert_eq!(fs::read(database.path()).unwrap(), before);
}

#[tokio::test]
async fn opening_to_read_never_creates_a_file() {
    let database = TempDatabase::new();

    assert!(SqliteLeads::open_read_only(&database.path()).await.is_err());
    assert!(!database.path().exists());
}

#[tokio::test]
async fn a_read_only_store_refuses_every_write() {
    let database = TempDatabase::new();
    SqliteLeads::open(&database.path())
        .await
        .unwrap()
        .close()
        .await;
    let before = fs::read(database.path()).unwrap();

    let reader = SqliteLeads::open_read_only(&database.path()).await.unwrap();
    let written = reader.persist(&Settings::default()).await;
    reader.close().await;

    assert!(written.is_err());
    assert_eq!(fs::read(database.path()).unwrap(), before);
}

#[tokio::test]
async fn a_read_only_store_reads_what_a_running_writer_has_not_checkpointed() {
    let database = TempDatabase::new();
    let writer = SqliteLeads::open(&database.path()).await.unwrap();
    writer.save(&signed_lead("DAC")).await.unwrap();

    let reader = SqliteLeads::open_read_only(&database.path()).await.unwrap();
    let leads = reader.list().await.unwrap();
    reader.close().await;
    writer.close().await;

    assert_eq!(leads.len(), 1);
}

#[tokio::test]
async fn a_read_only_store_can_copy_the_database() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    store.save(&signed_lead("DAC")).await.unwrap();
    store.close().await;
    let copy = database.0.join("copia.db");

    let reader = SqliteLeads::open_read_only(&database.path()).await.unwrap();
    reader.backup_to(&copy).await.unwrap();
    reader.close().await;

    let restored = SqliteLeads::open(&copy).await.unwrap();
    assert_eq!(restored.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_healthy_database_passes_the_integrity_check() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();

    assert_eq!(store.integrity_check().await.unwrap(), "ok");
}

#[tokio::test]
async fn a_damaged_database_reports_what_is_wrong() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    store.close().await;
    let raw = raw_pool(&database.path()).await;
    let mut connection = raw.acquire().await.unwrap();
    for statement in [
        "PRAGMA writable_schema = ON",
        "DELETE FROM sqlite_master WHERE name = 'quotes_lead'",
    ] {
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    drop(connection);
    raw.close().await;

    let reader = SqliteLeads::open_read_only(&database.path()).await.unwrap();
    let found = reader.integrity_check().await.unwrap();
    reader.close().await;

    assert_ne!(found, "ok");
}

#[tokio::test]
async fn all_cases_include_settled_ones_with_steps_and_milestones() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "REINFO").await;
    let mut paid = BillingMilestone::create(NewMilestone {
        case_id: file.case.id(),
        position: 1,
        title: "Total".to_owned(),
        amount: Money::from_cents(100).unwrap(),
        due_on: OPENED_ON,
    })
    .unwrap();
    paid.mark_paid(OPENED_ON);
    store
        .add_milestones(std::slice::from_ref(&paid))
        .await
        .unwrap();
    store.save_milestone(&paid).await.unwrap();
    store
        .close_case(&CaseClosure::of(file.case.clone(), &lead, OPENED_ON))
        .await
        .unwrap();
    let (_, open) = opened(&store, "DAC").await;

    let found = store.all_cases().await.unwrap();

    assert!(
        store
            .active_cases()
            .await
            .unwrap()
            .iter()
            .all(|item| item.case.id() != file.case.id())
    );
    assert_eq!(found.len(), 2);
    let settled = found
        .iter()
        .find(|item| item.case.id() == file.case.id())
        .unwrap();
    assert_eq!(settled.case.status().code(), "closed");
    assert_eq!(settled.steps.len(), file.steps.len());
    assert_eq!(settled.milestones, [paid]);
    assert!(found.iter().any(|item| item.case.id() == open.case.id()));
}

#[tokio::test]
async fn quote_counters_list_every_year_in_order() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, _) = opened(&store, "DAC").await;
    assert_eq!(store.quote_counters().await.unwrap(), []);

    for issued_on in [
        date!(2026 - 03 - 02),
        date!(2025 - 03 - 02),
        date!(2026 - 04 - 01),
    ] {
        store
            .create_quote(&quote_draft(&lead, issued_on))
            .await
            .unwrap();
    }

    assert_eq!(
        store.quote_counters().await.unwrap(),
        [
            QuoteCounter {
                year: 2025,
                last_sequence: 1
            },
            QuoteCounter {
                year: 2026,
                last_sequence: 2
            },
        ]
    );
}

#[tokio::test]
async fn opening_an_existing_database_does_not_create_one() {
    let scratch = TempDatabase::new();

    assert!(SqliteLeads::open_existing(&scratch.path()).await.is_err());

    assert!(!scratch.path().exists());
}

#[tokio::test]
async fn a_failed_upgrade_leaves_the_database_and_removes_its_working_copy() {
    let scratch = TempDatabase::new();
    let database = scratch.path();
    let store = SqliteLeads::open(&database).await.unwrap();
    store.close().await;
    let before = fs::read(&database).unwrap();
    let not_a_database = scratch.0.join("copy.db");
    fs::write(&not_a_database, "not a database").unwrap();

    assert!(
        SqliteLeads::upgrade(&database, &not_a_database)
            .await
            .is_err()
    );

    assert_eq!(fs::read(&database).unwrap(), before);
    assert!(!database.with_extension("migrating.db").exists());
}
