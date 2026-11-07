use std::{borrow::Cow, env, fs, path::PathBuf};

use cominorsa_domain::{
    BillingMilestone, BillingRepository, CaseClosure, CaseFile, CaseRepository, CaseStatus, Lead,
    LeadRepository, Money, NewLead, NewMilestone, ProspectRepository, SettingsRepository, Stage,
    procedure_template,
};
use cominorsa_storage::SqliteLeads;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use time::{Date, macros::date};
use uuid::Uuid;

const SIGNED_ON: Date = date!(2026 - 11 - 07);
const CLOSED_ON: Date = date!(2026 - 12 - 15);

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

fn soles(cents: i64) -> Money {
    Money::from_cents(cents).unwrap()
}

fn signed(service: &str) -> (Lead, CaseFile) {
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
    lead.advance().unwrap();
    let opening = CaseFile::open(&lead, SIGNED_ON, &procedure_template(service)).unwrap();

    (lead, opening)
}

async fn opened(store: &SqliteLeads, service: &str) -> (Lead, CaseFile) {
    let (lead, opening) = signed(service);
    store.sign_and_open(&lead, &opening).await.unwrap();
    (lead, opening)
}

fn milestone(file: &CaseFile, position: u32, cents: i64) -> BillingMilestone {
    BillingMilestone::create(NewMilestone {
        case_id: file.case.id(),
        position,
        title: format!("Hito {position}"),
        amount: soles(cents),
        due_on: SIGNED_ON,
    })
    .unwrap()
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
async fn persists_the_contract_total_in_integer_cents_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    assert_eq!(
        store.open_cases().await.unwrap()[0].case.contract_total(),
        None
    );

    store
        .save_contract_total(file.case.id(), soles(123_450))
        .await
        .unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(
        reopened.open_cases().await.unwrap()[0]
            .case
            .contract_total(),
        Some(soles(123_450))
    );
    reopened.close().await;
    assert_eq!(
        raw_count(
            &database.path(),
            "SELECT contract_total_cents FROM cases WHERE typeof(contract_total_cents) = 'integer'"
        )
        .await,
        123_450
    );
}

#[tokio::test]
async fn refuses_a_total_for_a_case_that_does_not_exist() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();

    let saved = store.save_contract_total(Uuid::now_v7(), soles(100)).await;

    assert!(saved.is_err());
}

#[tokio::test]
async fn adds_milestones_in_order_with_their_case_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    let added = vec![milestone(&file, 2, 700), milestone(&file, 1, 300)];

    store.add_milestones(&added).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let cases = reopened.open_cases().await.unwrap();
    assert_eq!(
        cases[0].milestones,
        vec![added[1].clone(), added[0].clone()]
    );
    assert_eq!(cases[0].steps, file.steps);
}

#[tokio::test]
async fn a_failed_plan_adds_no_milestone() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    let repeated_position = vec![milestone(&file, 1, 300), milestone(&file, 1, 700)];

    let added = store.add_milestones(&repeated_position).await;

    assert!(added.is_err());
    assert_eq!(store.open_cases().await.unwrap()[0].milestones, Vec::new());
}

#[tokio::test]
async fn persists_milestone_edits_and_payment() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    let mut item = milestone(&file, 1, 300);
    store
        .add_milestones(std::slice::from_ref(&item))
        .await
        .unwrap();

    item.rename("Saldo").unwrap();
    item.set_amount(soles(450)).unwrap();
    item.reschedule(date!(2027 - 01 - 15));
    item.mark_paid(SIGNED_ON);
    store.save_milestone(&item).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(
        reopened.open_cases().await.unwrap()[0].milestones,
        vec![item.clone()]
    );

    item.mark_unpaid();
    reopened.save_milestone(&item).await.unwrap();
    assert_eq!(
        reopened.open_cases().await.unwrap()[0].milestones[0].paid_on(),
        None
    );
}

#[tokio::test]
async fn deletes_a_milestone_and_keeps_the_others() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    let first = milestone(&file, 1, 300);
    let second = milestone(&file, 2, 700);
    store
        .add_milestones(&[first.clone(), second.clone()])
        .await
        .unwrap();

    store.delete_milestone(first.id()).await.unwrap();

    assert_eq!(
        store.open_cases().await.unwrap()[0].milestones,
        vec![second]
    );
}

#[tokio::test]
async fn refuses_to_save_or_delete_a_milestone_that_does_not_exist() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    let missing = milestone(&file, 1, 300);

    assert!(store.save_milestone(&missing).await.is_err());
    assert!(store.delete_milestone(missing.id()).await.is_err());
}

#[tokio::test]
async fn refuses_to_return_a_corrupt_milestone() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = opened(&store, "DAC").await;
    store
        .add_milestones(&[milestone(&file, 1, 300)])
        .await
        .unwrap();

    raw_execute(
        &database.path(),
        "UPDATE billing_milestones SET title = ' '",
    )
    .await;

    assert!(store.open_cases().await.is_err());
}

#[tokio::test]
async fn closing_a_recurring_case_creates_one_linked_renewal_lead() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "DAC").await;
    let closure = CaseClosure::of(file.case.clone(), &lead, CLOSED_ON);

    let closed = store.close_case(&closure).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let leads = reopened.list().await.unwrap();
    let renewal = leads.iter().find(|item| item.id() != lead.id()).unwrap();
    assert!(closed);
    assert_eq!(leads.len(), 2);
    assert_eq!(renewal, closure.renewal.as_ref().unwrap());
    assert_eq!(renewal.renewal_of(), Some(file.case.id()));
    assert_eq!(renewal.stage(), Stage::InitialContact);
    assert_eq!(reopened.open_cases().await.unwrap(), Vec::new());
    reopened.close().await;
    assert_eq!(
        raw_count(
            &database.path(),
            "SELECT COUNT(*) FROM cases WHERE status = 'closed'"
        )
        .await,
        1
    );
    assert_eq!(
        raw_count(
            &database.path(),
            "SELECT COUNT(*) FROM leads AS lead JOIN cases AS source
                ON source.id = lead.renewal_of_case"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_repeated_close_creates_no_second_lead() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "DAC").await;

    let first = store
        .close_case(&CaseClosure::of(file.case.clone(), &lead, CLOSED_ON))
        .await
        .unwrap();
    let second = store
        .close_case(&CaseClosure::of(file.case, &lead, date!(2027 - 03 - 01)))
        .await
        .unwrap();

    assert!(first);
    assert!(!second);
    let leads = store.list().await.unwrap();
    assert_eq!(leads.len(), 2);
    assert!(
        leads
            .iter()
            .any(|item| item.follow_up_on() == date!(2027 - 12 - 15))
    );
}

#[tokio::test]
async fn the_database_refuses_a_second_renewal_of_one_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "DAC").await;
    store
        .close_case(&CaseClosure::of(file.case.clone(), &lead, CLOSED_ON))
        .await
        .unwrap();
    let another = CaseClosure::of(file.case, &lead, CLOSED_ON)
        .renewal
        .unwrap();

    assert!(store.save(&another).await.is_err());
    assert_eq!(store.list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn saving_a_renewal_lead_again_keeps_its_link() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "DAC").await;
    let closure = CaseClosure::of(file.case.clone(), &lead, CLOSED_ON);
    store.close_case(&closure).await.unwrap();
    let mut renewal = closure.renewal.unwrap();

    renewal.advance().unwrap();
    store.save(&renewal).await.unwrap();

    let leads = store.list().await.unwrap();
    let saved = leads.iter().find(|item| item.id() == renewal.id()).unwrap();
    assert_eq!(saved.renewal_of(), Some(file.case.id()));
    assert_eq!(saved.stage(), Stage::CoordinateEvaluation);
}

#[tokio::test]
async fn closing_a_case_that_does_not_recur_creates_no_lead() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "REINFO").await;

    let closed = store
        .close_case(&CaseClosure::of(file.case, &lead, CLOSED_ON))
        .await
        .unwrap();

    assert!(closed);
    assert_eq!(store.list().await.unwrap(), vec![lead]);
    assert_eq!(store.open_cases().await.unwrap(), Vec::new());
}

#[tokio::test]
async fn a_failed_renewal_leaves_the_case_open_and_no_lead() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "DAC").await;
    raw_execute(
        &database.path(),
        "CREATE TRIGGER refuse_renewals BEFORE INSERT ON leads
            WHEN NEW.renewal_of_case IS NOT NULL BEGIN SELECT RAISE(ABORT, 'refused'); END",
    )
    .await;

    let failed = store
        .close_case(&CaseClosure::of(file.case.clone(), &lead, CLOSED_ON))
        .await;

    assert!(failed.is_err());
    assert_eq!(store.list().await.unwrap(), vec![lead]);
    let cases = store.open_cases().await.unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].case.status(), CaseStatus::Open);
    store.close().await;
    assert_eq!(
        raw_count(
            &database.path(),
            "SELECT COUNT(*) FROM cases WHERE status = 'open'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn keeps_a_closed_case_active_only_while_a_milestone_is_unpaid() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "REINFO").await;
    let (_, other) = opened(&store, "ESTAMIN").await;
    let mut item = milestone(&file, 1, 300);
    store
        .add_milestones(std::slice::from_ref(&item))
        .await
        .unwrap();
    store
        .close_case(&CaseClosure::of(file.case.clone(), &lead, CLOSED_ON))
        .await
        .unwrap();

    let active = store.active_cases().await.unwrap();
    assert_eq!(active.len(), 2);
    assert_eq!(active[0].case.status(), CaseStatus::Closed);
    assert_eq!(active[0].steps, file.steps);
    assert_eq!(active[0].milestones, vec![item.clone()]);
    assert_eq!(active[1].case, other.case);

    item.mark_paid(CLOSED_ON);
    store.save_milestone(&item).await.unwrap();

    let active = store.active_cases().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].case, other.case);
}

#[tokio::test]
async fn restores_milestones_totals_and_renewal_links_from_a_backup() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "DAC").await;
    let item = milestone(&file, 1, 300);
    store
        .add_milestones(std::slice::from_ref(&item))
        .await
        .unwrap();
    store
        .save_contract_total(file.case.id(), soles(1_000))
        .await
        .unwrap();
    let closure = CaseClosure::of(file.case.clone(), &lead, CLOSED_ON);
    store.close_case(&closure).await.unwrap();
    let backup = database.0.join("copia.db");
    let safety = database.0.join("seguridad.db");
    store.backup_to(&backup).await.unwrap();
    raw_execute(&database.path(), "DELETE FROM billing_milestones").await;

    let restored = store
        .restore(&database.path(), &backup, &safety)
        .await
        .unwrap();

    let active = restored.active_cases().await.unwrap();
    assert_eq!(active[0].milestones, vec![item]);
    assert_eq!(active[0].case.contract_total(), Some(soles(1_000)));
    assert_eq!(restored.list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn refuses_to_restore_a_backup_with_a_corrupt_milestone_of_a_closed_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (lead, file) = opened(&store, "REINFO").await;
    store
        .add_milestones(&[milestone(&file, 1, 300)])
        .await
        .unwrap();
    store
        .close_case(&CaseClosure::of(file.case, &lead, CLOSED_ON))
        .await
        .unwrap();
    let backup = database.0.join("copia.db");
    let safety = database.0.join("seguridad.db");
    store.backup_to(&backup).await.unwrap();
    raw_execute(&backup, "UPDATE billing_milestones SET title = ' '").await;
    let current = store.clone();

    let restored = store.restore(&database.path(), &backup, &safety).await;

    assert!(restored.is_err());
    assert!(!safety.exists());
    assert_eq!(current.active_cases().await.unwrap().len(), 1);
}

#[tokio::test]
async fn keeps_everything_when_upgrading_from_the_sixth_schema() {
    let database = TempDatabase::new();
    let old = raw_pool(&database.path()).await;
    let mut sixth_schema = sqlx::migrate!("./migrations");
    sixth_schema.migrations = Cow::Owned(
        sixth_schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= 6)
            .cloned()
            .collect(),
    );
    sixth_schema.run(&old).await.unwrap();
    let case_id = "00000000-0000-7000-8000-00000000000a";
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
        "INSERT INTO cases VALUES ('00000000-0000-7000-8000-00000000000a',
            '00000000-0000-7000-8000-000000000001', 'Firmado', NULL, 'DAC', 'Ana', '2026-11-01',
            'open')",
        "INSERT INTO case_steps VALUES ('00000000-0000-7000-8000-00000000000b',
            '00000000-0000-7000-8000-00000000000a', 1, 'Paso uno', '2026-11-20', '2026-11-05')",
        "INSERT INTO case_steps VALUES ('00000000-0000-7000-8000-00000000000c',
            '00000000-0000-7000-8000-00000000000a', 2, 'Paso dos', '2026-12-01', NULL)",
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
    assert_eq!(leads[0].renewal_of(), None);
    assert_eq!(leads[1].ruc().map(|ruc| ruc.as_str()), Some("20123456786"));
    let prospects = upgraded.search("", 10).await.unwrap();
    assert_eq!(prospects.len(), 1);
    assert_eq!(prospects[0].tax.as_ref().unwrap().status, "ACTIVO");
    let settings = upgraded.load().await.unwrap();
    assert_eq!(settings.follow_up_days(), 9);
    assert_eq!(settings.team(), ["Ana"]);
    let cases = upgraded.open_cases().await.unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].case.id().to_string(), case_id);
    assert_eq!(cases[0].case.client_name(), "Firmado");
    assert_eq!(cases[0].case.contract_total(), None);
    assert_eq!(cases[0].milestones, Vec::new());
    assert_eq!(cases[0].steps.len(), 2);
    assert_eq!(cases[0].steps[0].title(), "Paso uno");
    assert_eq!(cases[0].steps[0].done_on(), Some(date!(2026 - 11 - 05)));
    assert_eq!(cases[0].steps[1].due_on(), date!(2026 - 12 - 01));

    let closure = CaseClosure::of(cases[0].case.clone(), &leads[0], CLOSED_ON);
    assert!(upgraded.close_case(&closure).await.unwrap());
    assert_eq!(upgraded.list().await.unwrap().len(), 3);
}
