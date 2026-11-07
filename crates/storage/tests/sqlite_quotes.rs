use std::{borrow::Cow, collections::BTreeSet, env, fs, path::PathBuf};

use cominorsa_domain::{
    BillingRepository, CaseClosure, CaseFile, CaseRepository, CompanyProfile, IgvRate, Lead,
    LeadRepository, Money, NewLead, ProspectRepository, Quote, QuoteContent, QuoteDraft, QuoteLine,
    QuoteRepository, QuoteStatus, SettingsRepository, procedure_template,
};
use cominorsa_storage::SqliteLeads;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use time::{Date, macros::date};
use uuid::Uuid;

const ISSUED_ON: Date = date!(2026 - 11 - 07);
const VALID_UNTIL: Date = date!(2026 - 11 - 22);

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

fn new_lead(service: &str) -> Lead {
    Lead::create(NewLead {
        name: "Minera Peña Blanca".to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: service.to_owned(),
        owner: Some("Ana".to_owned()),
        follow_up_on: ISSUED_ON,
    })
    .unwrap()
}

async fn saved_lead(store: &SqliteLeads) -> Lead {
    let lead = new_lead("REINFO");
    store.save(&lead).await.unwrap();
    lead
}

fn line(description: &str, quantity: u32, cents: i64) -> QuoteLine {
    QuoteLine::new(description, quantity, soles(cents)).unwrap()
}

fn content_on(issued_on: Date, valid_until: Date) -> QuoteContent {
    QuoteContent {
        issued_on,
        valid_until,
        lines: vec![line("Evaluación", 2, 10_000), line("Trámite", 1, 5_000)],
        notes: "Sin viáticos".to_owned(),
    }
}

fn draft_for(lead: &Lead) -> QuoteDraft {
    QuoteDraft::for_lead(lead, content_on(ISSUED_ON, VALID_UNTIL), IgvRate::default()).unwrap()
}

fn company() -> CompanyProfile {
    CompanyProfile {
        name: "Empresa Ejemplo SAC".to_owned(),
        ruc: "20123456786".to_owned(),
        address: "Av. Ejemplo 123".to_owned(),
        phone: "900000000".to_owned(),
        email: "ventas@example.com".to_owned(),
    }
}

async fn sent(store: &SqliteLeads, quote: &Quote) -> Quote {
    let mut quote = quote.clone();
    quote.mark_sent(&company()).unwrap();
    store
        .save_quote_status(&quote, QuoteStatus::Draft)
        .await
        .unwrap();
    quote
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
async fn numbers_are_sequential_within_a_year_and_restart_the_next_year() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let next_year = QuoteDraft::for_lead(
        &lead,
        content_on(date!(2027 - 01 - 02), date!(2027 - 01 - 20)),
        IgvRate::default(),
    )
    .unwrap();

    let first = store.create_quote(&draft_for(&lead)).await.unwrap();
    let second = store.create_quote(&draft_for(&lead)).await.unwrap();
    let other_year = store.create_quote(&next_year).await.unwrap();
    let third = store.create_quote(&draft_for(&lead)).await.unwrap();

    assert_eq!(first.number().to_string(), "COT-2026-0001");
    assert_eq!(second.number().to_string(), "COT-2026-0002");
    assert_eq!(third.number().to_string(), "COT-2026-0003");
    assert_eq!(other_year.number().to_string(), "COT-2027-0001");
    store.close().await;
}

#[tokio::test]
async fn numbers_stay_unique_when_saves_race_on_one_store() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;

    let saves: Vec<_> = (0..20)
        .map(|_| {
            let store = store.clone();
            let draft = draft_for(&lead);
            tokio::spawn(async move { store.create_quote(&draft).await.unwrap() })
        })
        .collect();
    let mut sequences = BTreeSet::new();
    for save in saves {
        sequences.insert(save.await.unwrap().number().sequence());
    }

    assert_eq!(sequences, (1..=20).collect::<BTreeSet<u32>>());
    assert_eq!(store.quotes().await.unwrap().len(), 20);
    store.close().await;
}

#[tokio::test]
async fn numbers_stay_unique_when_two_processes_share_the_file() {
    let database = TempDatabase::new();
    let first = SqliteLeads::open(&database.path()).await.unwrap();
    let second = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&first).await;

    let saves: Vec<_> = (0..20)
        .map(|index| {
            let store = if index % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            };
            let draft = draft_for(&lead);
            tokio::spawn(async move { store.create_quote(&draft).await.unwrap() })
        })
        .collect();
    let mut sequences = BTreeSet::new();
    for save in saves {
        sequences.insert(save.await.unwrap().number().sequence());
    }

    assert_eq!(sequences, (1..=20).collect::<BTreeSet<u32>>());
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn repeating_a_save_returns_the_stored_quote_and_uses_no_number() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let draft = draft_for(&lead);

    let first = store.create_quote(&draft).await.unwrap();
    let repeated = store.create_quote(&draft).await.unwrap();
    let next = store.create_quote(&draft_for(&lead)).await.unwrap();

    assert_eq!(repeated, first);
    assert_eq!(next.number().to_string(), "COT-2026-0002");
    assert_eq!(store.quotes().await.unwrap().len(), 2);
    store.close().await;
    assert_eq!(
        raw_count(&database.path(), "SELECT COUNT(*) FROM quote_lines").await,
        4
    );
}

#[tokio::test]
async fn stores_a_quote_with_its_lines_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let quote = store.create_quote(&draft_for(&lead)).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let stored = reopened.quotes().await.unwrap();

    assert_eq!(stored, vec![quote]);
    assert_eq!(stored[0].lead_id(), lead.id());
    assert_eq!(stored[0].client_name(), "Minera Peña Blanca");
    assert_eq!(stored[0].lines()[0].description(), "Evaluación");
    assert_eq!(stored[0].lines()[1].unit_price(), soles(5_000));
    assert_eq!(stored[0].notes(), "Sin viáticos");
    assert_eq!(stored[0].totals().subtotal, soles(25_000));
    assert_eq!(stored[0].totals().total, soles(29_500));
    reopened.close().await;
}

#[tokio::test]
async fn a_failed_line_rolls_back_the_quote_and_gives_back_the_number() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    raw_execute(
        &database.path(),
        "CREATE TRIGGER refuse_second_line BEFORE INSERT ON quote_lines
            WHEN NEW.position = 2 BEGIN SELECT RAISE(ABORT, 'refused'); END",
    )
    .await;

    let failed = store.create_quote(&draft_for(&lead)).await;

    assert!(failed.is_err());
    assert_eq!(store.quotes().await.unwrap(), Vec::new());
    for table_count in [
        "SELECT COUNT(*) FROM quotes",
        "SELECT COUNT(*) FROM quote_lines",
        "SELECT COUNT(*) FROM quote_numbers",
    ] {
        assert_eq!(raw_count(&database.path(), table_count).await, 0);
    }

    raw_execute(&database.path(), "DROP TRIGGER refuse_second_line").await;
    let retried = store.create_quote(&draft_for(&lead)).await.unwrap();

    assert_eq!(retried.number().to_string(), "COT-2026-0001");
    store.close().await;
}

#[tokio::test]
async fn a_quote_needs_a_lead_that_exists() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();

    assert!(
        store
            .create_quote(&draft_for(&new_lead("REINFO")))
            .await
            .is_err()
    );
    store.close().await;
    assert_eq!(
        raw_count(&database.path(), "SELECT COUNT(*) FROM quote_numbers").await,
        0
    );
}

#[tokio::test]
async fn saves_new_content_of_a_draft_and_replaces_its_lines() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let mut quote = store.create_quote(&draft_for(&lead)).await.unwrap();

    quote
        .revise(
            QuoteContent {
                issued_on: date!(2026 - 11 - 08),
                valid_until: date!(2026 - 11 - 30),
                lines: vec![line("Asesoría", 3, 1_000)],
                notes: "Nueva nota".to_owned(),
            },
            IgvRate::from_basis_points(1000).unwrap(),
        )
        .unwrap();
    store.save_quote_draft(&quote).await.unwrap();

    assert_eq!(store.quotes().await.unwrap(), vec![quote]);
    store.close().await;
}

#[tokio::test]
async fn a_failed_draft_save_keeps_the_previous_content() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let quote = store.create_quote(&draft_for(&lead)).await.unwrap();
    let mut revised = quote.clone();
    revised
        .revise(
            QuoteContent {
                notes: "Nueva nota".to_owned(),
                ..content_on(ISSUED_ON, VALID_UNTIL)
            },
            IgvRate::default(),
        )
        .unwrap();
    raw_execute(
        &database.path(),
        "CREATE TRIGGER refuse_second_line BEFORE INSERT ON quote_lines
            WHEN NEW.position = 2 BEGIN SELECT RAISE(ABORT, 'refused'); END",
    )
    .await;

    assert!(store.save_quote_draft(&revised).await.is_err());

    assert_eq!(store.quotes().await.unwrap(), vec![quote]);
    store.close().await;
}

#[tokio::test]
async fn storage_refuses_to_change_the_content_of_a_sent_quote() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let draft = store.create_quote(&draft_for(&lead)).await.unwrap();
    let issued = sent(&store, &draft).await;
    let mut stale = draft.clone();
    stale
        .revise(
            QuoteContent {
                lines: vec![line("Cambiada", 1, 1)],
                ..content_on(ISSUED_ON, VALID_UNTIL)
            },
            IgvRate::default(),
        )
        .unwrap();

    let refused = store.save_quote_draft(&stale).await;

    assert_eq!(
        refused.unwrap_err().to_string(),
        "No se pudo acceder a los datos: La cotización ya no es un borrador"
    );
    assert_eq!(store.quotes().await.unwrap(), vec![issued]);
    store.close().await;
}

#[tokio::test]
async fn persists_status_changes_and_the_frozen_company() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let draft = store.create_quote(&draft_for(&lead)).await.unwrap();

    let mut quote = sent(&store, &draft).await;
    assert_eq!(store.quotes().await.unwrap(), vec![quote.clone()]);
    assert_eq!(store.quotes().await.unwrap()[0].company(), &company());

    quote.mark_accepted().unwrap();
    store
        .save_quote_status(&quote, QuoteStatus::Sent)
        .await
        .unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(
        reopened.quotes().await.unwrap()[0].status(),
        QuoteStatus::Accepted
    );
    reopened.close().await;
}

#[tokio::test]
async fn refuses_a_status_change_when_the_stored_status_already_moved() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let draft = store.create_quote(&draft_for(&lead)).await.unwrap();
    let quote = sent(&store, &draft).await;
    let mut accepted = quote.clone();
    accepted.mark_accepted().unwrap();
    store
        .save_quote_status(&accepted, QuoteStatus::Sent)
        .await
        .unwrap();
    let mut rejected = quote;
    rejected.mark_rejected().unwrap();

    let refused = store.save_quote_status(&rejected, QuoteStatus::Sent).await;

    assert!(refused.is_err());
    assert_eq!(store.quotes().await.unwrap(), vec![accepted]);
    store.close().await;
}

#[tokio::test]
async fn a_new_version_keeps_the_number_and_leaves_the_original_untouched() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let draft = store.create_quote(&draft_for(&lead)).await.unwrap();
    let original = sent(&store, &draft).await;

    let version = store
        .create_quote(
            &original
                .new_version(date!(2026 - 12 - 01), IgvRate::default())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(version.number().to_string(), "COT-2026-0001-V2");
    assert_eq!(version.supersedes(), Some(original.id()));
    assert_eq!(version.status(), QuoteStatus::Draft);
    assert_eq!(
        store.quotes().await.unwrap(),
        vec![original.clone(), version]
    );

    let next = store.create_quote(&draft_for(&lead)).await.unwrap();
    assert_eq!(next.number().to_string(), "COT-2026-0002");
    store.close().await;
}

#[tokio::test]
async fn versioning_the_same_quote_twice_returns_the_stored_version() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let draft = store.create_quote(&draft_for(&lead)).await.unwrap();
    let original = sent(&store, &draft).await;

    let first = store
        .create_quote(&original.new_version(ISSUED_ON, IgvRate::default()).unwrap())
        .await
        .unwrap();
    let second = store
        .create_quote(&original.new_version(ISSUED_ON, IgvRate::default()).unwrap())
        .await
        .unwrap();

    assert_eq!(second, first);
    assert_eq!(store.quotes().await.unwrap().len(), 2);
    store.close().await;
}

#[tokio::test]
async fn the_database_refuses_a_repeated_number() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    store.create_quote(&draft_for(&lead)).await.unwrap();
    store.close().await;

    let pool = raw_pool(&database.path()).await;
    let repeated = sqlx::query(
        "INSERT INTO quotes SELECT 'other', lead_id, year, sequence, version, supersedes,
            client_name, client_ruc, service, issued_on, valid_until, notes, status,
            igv_basis_points, company_name, company_ruc, company_address, company_phone,
            company_email FROM quotes",
    )
    .execute(&pool)
    .await;

    assert!(repeated.is_err());
    pool.close().await;
}

#[tokio::test]
async fn refuses_to_return_a_corrupt_quote() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    store.create_quote(&draft_for(&lead)).await.unwrap();
    raw_execute(&database.path(), "UPDATE quotes SET status = 'archived'").await;

    assert!(store.quotes().await.is_err());
    store.close().await;
}

#[tokio::test]
async fn opening_a_case_stores_the_suggested_contract_total() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut lead = new_lead("DAC");
    for _ in 0..3 {
        lead.advance().unwrap();
    }
    let mut opening = CaseFile::open(&lead, ISSUED_ON, &procedure_template("DAC")).unwrap();
    opening.case.set_contract_total(soles(29_500)).unwrap();

    store.sign_and_open(&lead, &opening).await.unwrap();

    let cases = store.open_cases().await.unwrap();
    assert_eq!(cases[0].case.contract_total(), Some(soles(29_500)));
    store.close().await;
}

#[tokio::test]
async fn persists_the_company_profile_and_igv_rate_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut settings = store.load().await.unwrap();
    settings.set_company(company()).unwrap();
    settings.set_igv_rate(IgvRate::parse("10.5").unwrap());
    store.persist(&settings).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let loaded = reopened.load().await.unwrap();

    assert_eq!(loaded.company(), &company());
    assert_eq!(loaded.igv_rate().basis_points(), 1050);
    reopened.close().await;
}

#[tokio::test]
async fn clearing_a_company_field_removes_the_stored_value() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut settings = store.load().await.unwrap();
    settings.set_company(company()).unwrap();
    store.persist(&settings).await.unwrap();

    settings
        .set_company(CompanyProfile {
            phone: String::new(),
            ..company()
        })
        .unwrap();
    store.persist(&settings).await.unwrap();

    assert_eq!(store.load().await.unwrap().company().phone, "");
    store.close().await;
    assert_eq!(
        raw_count(
            &database.path(),
            "SELECT COUNT(*) FROM settings WHERE key = 'company_phone'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn loads_old_company_and_igv_values_without_blocking() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    raw_execute(
        &database.path(),
        "INSERT INTO settings VALUES ('company_ruc', '123'), ('igv_basis_points', 'abc')",
    )
    .await;

    let settings = store.load().await.unwrap();

    assert_eq!(settings.company().ruc, "123");
    assert_eq!(settings.igv_rate(), IgvRate::default());
    store.close().await;
}

#[tokio::test]
async fn restores_quotes_and_keeps_numbering_from_a_backup() {
    let database = TempDatabase::new();
    let folder = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let quote = store.create_quote(&draft_for(&lead)).await.unwrap();
    let backup = folder.0.join("copia.db");
    store.backup_to(&backup).await.unwrap();
    store.create_quote(&draft_for(&lead)).await.unwrap();

    let restored = store
        .restore(&database.path(), &backup, &folder.0.join("safety.db"))
        .await
        .unwrap();

    assert_eq!(restored.quotes().await.unwrap(), vec![quote]);
    let next = restored.create_quote(&draft_for(&lead)).await.unwrap();
    assert_eq!(next.number().to_string(), "COT-2026-0002");
    restored.close().await;
}

#[tokio::test]
async fn refuses_to_restore_a_backup_with_a_corrupt_quote() {
    let database = TempDatabase::new();
    let folder = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let lead = saved_lead(&store).await;
    let quote = store.create_quote(&draft_for(&lead)).await.unwrap();
    let backup = folder.0.join("copia.db");
    store.backup_to(&backup).await.unwrap();
    raw_execute(&backup, "UPDATE quotes SET status = 'archived'").await;
    let safety = folder.0.join("safety.db");

    let reopened = match store
        .clone()
        .restore(&database.path(), &backup, &safety)
        .await
    {
        Ok(_) => panic!("the corrupt backup must be refused"),
        Err(_) => store,
    };

    assert!(!safety.exists());
    assert_eq!(reopened.quotes().await.unwrap(), vec![quote]);
    reopened.close().await;
}

#[tokio::test]
async fn keeps_everything_when_upgrading_from_the_seventh_schema() {
    let database = TempDatabase::new();
    let old = raw_pool(&database.path()).await;
    let mut seventh_schema = sqlx::migrate!("./migrations");
    seventh_schema.migrations = Cow::Owned(
        seventh_schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= 7)
            .cloned()
            .collect(),
    );
    seventh_schema.run(&old).await.unwrap();
    let lead_id = "00000000-0000-7000-8000-000000000001";
    let case_id = "00000000-0000-7000-8000-00000000000a";
    for statement in [
        "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on, owner)
            VALUES ('00000000-0000-7000-8000-000000000001', 'Firmado', '900000000', NULL, 'DAC',
            'contract_signed', '2026-11-01', 'Ana')",
        "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on, owner,
            renewal_of_case)
            VALUES ('00000000-0000-7000-8000-000000000002', 'Renovación: Otro', '900000001',
            '20123456786', 'DAC', 'quote_sent', '2026-11-02', NULL, NULL)",
        "INSERT INTO prospects (kind, name, profile, concessions, hectares, departments,
            provinces, substances, concession_states, potential_service, needs_review, source)
            VALUES ('company', 'Titular', 'OTRO_estado', 1, 2.5, 'PIURA', 'SULLANA', 'M',
            'TITULADA', 'DAC', 0, 'Synthetic')",
        "INSERT INTO settings VALUES ('follow_up_days', '9'),
            ('whatsapp_template', 'Hola {nombre}')",
        "INSERT INTO team_members VALUES (0, 'Ana')",
        "INSERT INTO cases (id, lead_id, client_name, ruc, service, owner, opened_on, status,
            contract_total_cents)
            VALUES ('00000000-0000-7000-8000-00000000000a',
            '00000000-0000-7000-8000-000000000001', 'Firmado', NULL, 'DAC', 'Ana', '2026-11-01',
            'open', 150000)",
        "INSERT INTO case_steps VALUES ('00000000-0000-7000-8000-00000000000b',
            '00000000-0000-7000-8000-00000000000a', 1, 'Paso uno', '2026-11-20', NULL)",
        "INSERT INTO billing_milestones VALUES ('00000000-0000-7000-8000-00000000000c',
            '00000000-0000-7000-8000-00000000000a', 1, 'Adelanto', 45000, '2026-11-02',
            '2026-11-03')",
    ] {
        sqlx::query(statement).execute(&old).await.unwrap();
    }
    old.close().await;

    let upgraded = SqliteLeads::open(&database.path()).await.unwrap();

    let leads = upgraded.list().await.unwrap();
    assert_eq!(leads.len(), 2);
    assert_eq!(leads[0].id().to_string(), lead_id);
    assert_eq!(leads[0].owner(), Some("Ana"));
    assert_eq!(leads[1].name(), "Renovación: Otro");
    let prospects = upgraded.search("", 10).await.unwrap();
    assert_eq!(prospects.len(), 1);
    let settings = upgraded.load().await.unwrap();
    assert_eq!(settings.follow_up_days(), 9);
    assert_eq!(settings.whatsapp_template(), "Hola {nombre}");
    assert_eq!(settings.team(), ["Ana"]);
    assert_eq!(settings.company(), &CompanyProfile::default());
    assert_eq!(settings.igv_rate(), IgvRate::default());
    let cases = upgraded.open_cases().await.unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].case.id().to_string(), case_id);
    assert_eq!(cases[0].case.contract_total(), Some(soles(150_000)));
    assert_eq!(cases[0].steps[0].title(), "Paso uno");
    assert_eq!(cases[0].milestones[0].amount(), soles(45_000));
    assert_eq!(
        cases[0].milestones[0].paid_on(),
        Some(date!(2026 - 11 - 03))
    );
    assert_eq!(upgraded.quotes().await.unwrap(), Vec::new());

    let quote = upgraded.create_quote(&draft_for(&leads[0])).await.unwrap();
    assert_eq!(quote.number().to_string(), "COT-2026-0001");
    upgraded
        .save_contract_total(cases[0].case.id(), soles(200_000))
        .await
        .unwrap();
    let closure = CaseClosure::of(cases[0].case.clone(), &leads[0], ISSUED_ON);
    assert!(upgraded.close_case(&closure).await.unwrap());
    upgraded.close().await;
}
