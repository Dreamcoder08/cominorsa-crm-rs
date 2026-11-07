use std::{
    env, fs,
    path::{Path, PathBuf},
};

use cominorsa_application::{
    AppError, BillingEdit, Clock, Context, ErrorKind, FormValues, Ids, MilestoneValues,
    ProspectConversion, QuoteEdit, QuoteLineValues, QuoteValues, SettingsChange, add_lead,
    advance_lead, agenda, apply_billing, assign_lead, close_case, convert_prospect, create_backup,
    find_case, find_quote, reschedule_step, restore_backup, run_backup, save_quote_edit,
    save_quote_pdf, send_quote, set_step_done, snapshot, status, update_settings,
};
use cominorsa_domain::{
    CaseRepository, DomainError, LeadRepository, Prospect, ProspectKind, ProspectRepository,
    QuoteRepository, QuoteStatus, RepositoryError, SettingsRepository, Stage, TaxRecord,
};
use cominorsa_storage::SqliteLeads;
use time::{Date, macros::date};
use uuid::{Uuid, Version};

const TODAY: Date = date!(2026 - 03 - 02);
const NOW: i64 = 1_772_470_800;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-app-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("crm.db")
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn context(seed: &str) -> Context {
    Context::new(Clock::fixed(Some(TODAY), Some(NOW)), Ids::seeded(seed))
}

fn form(name: &str, service: &str) -> FormValues {
    FormValues {
        name: name.to_owned(),
        phone: "987654321".to_owned(),
        service: service.to_owned(),
        follow_up_on: "2026-03-10".to_owned(),
        ..FormValues::default()
    }
}

fn prospect(name: &str, service: &str, tax: Option<TaxRecord>) -> Prospect {
    Prospect {
        name: name.to_owned(),
        kind: ProspectKind::Company,
        profile: "PERFIL_1_formalizacion".to_owned(),
        concessions: 1,
        hectares: 10.0,
        departments: "PIURA".to_owned(),
        provinces: "SULLANA".to_owned(),
        substances: "M".to_owned(),
        concession_states: "TITULADA".to_owned(),
        potential_service: service.to_owned(),
        needs_review: false,
        source: "Synthetic".to_owned(),
        tax,
    }
}

fn tax(approximate: bool) -> TaxRecord {
    TaxRecord {
        ruc: "20100070970".to_owned(),
        status: "ACTIVO".to_owned(),
        condition: "HABIDO".to_owned(),
        address: "Lima".to_owned(),
        approximate,
    }
}

async fn signed(store: &SqliteLeads, seed: &str, service: &str) -> (Uuid, Uuid) {
    let mut lead_context = context(&format!("{seed}-lead"));
    let lead = add_lead(store, form("Minera Sintética", service), &mut lead_context)
        .await
        .unwrap()
        .lead;
    for stage in 0..3 {
        let mut step_context = context(&format!("{seed}-advance-{stage}"));
        advance_lead(store, lead.id(), &mut step_context)
            .await
            .unwrap();
    }
    let cases = store.active_cases().await.unwrap();
    let file = cases
        .iter()
        .find(|file| file.case.lead_id() == lead.id())
        .unwrap();

    (lead.id(), file.case.id())
}

#[test]
fn a_seed_gives_the_same_ids_and_different_seeds_do_not() {
    let (mut first, mut second, mut other) = (Ids::seeded("a"), Ids::seeded("a"), Ids::seeded("b"));
    let (one, two) = (first.issue().unwrap(), first.issue().unwrap());

    assert_ne!(one, two);
    assert_eq!(second.issue(), Some(one));
    assert_ne!(other.issue(), Some(one));
}

#[test]
fn without_a_seed_ids_stay_with_the_domain() {
    assert_eq!(Ids::generated().issue(), None);
}

#[test]
fn a_fixed_clock_reports_the_injected_day_and_instant() {
    let clock = Clock::fixed(Some(TODAY), Some(NOW));

    assert_eq!(clock.today(), TODAY);
    assert_eq!(clock.now_unix(), NOW);
}

#[test]
fn a_day_alone_fixes_the_instant_at_noon_in_peru() {
    let clock = Clock::fixed(Some(date!(2026 - 03 - 02)), None);

    assert_eq!(clock.file_stamp(), "20260302-120000");
}

#[test]
fn an_instant_alone_fixes_the_day_in_peru() {
    // 2026-03-03 02:00 UTC is still March 2 in Lima.
    let clock = Clock::fixed(None, Some(1_772_503_200));

    assert_eq!(clock.today(), date!(2026 - 03 - 02));
}

#[test]
fn the_system_clock_is_the_production_default() {
    let clock = Context::system().clock;

    assert!(clock.today().year() >= 2026);
    assert!(clock.now_unix() > NOW - 86_400 * 365);
}

#[test]
fn domain_rejections_keep_their_message_and_get_a_stable_code() {
    let error = AppError::from(DomainError::FinalStage);

    assert_eq!(error.kind(), ErrorKind::Rejected);
    assert_eq!(error.code(), "final_stage");
    assert_eq!(error.to_string(), "El lead ya está en la etapa final");
}

#[test]
fn a_corrupt_record_is_a_storage_failure() {
    let error = AppError::from(DomainError::CorruptRecord);

    assert_eq!(error.kind(), ErrorKind::Storage);
    assert_eq!(error.code(), "corrupt_record");
}

#[test]
fn repository_failures_are_storage_failures() {
    let error = AppError::from(RepositoryError("sin disco".to_owned()));

    assert_eq!(error.kind(), ErrorKind::Storage);
    assert_eq!(error.code(), "storage_error");
    assert_eq!(
        error.to_string(),
        "No se pudo acceder a los datos: sin disco"
    );
}

#[tokio::test]
async fn adding_a_lead_uses_the_seeded_id_and_persists_it() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let added = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap();

    let expected = Ids::seeded("one").issue().unwrap();
    assert!(added.created);
    assert_eq!(added.lead.id(), expected);
    assert_eq!(store.list().await.unwrap()[0].id(), expected);
}

#[tokio::test]
async fn without_a_seed_a_new_lead_keeps_a_time_ordered_id() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let mut context = Context::system();

    let added = add_lead(&store, form("Lead Uno", "DAC"), &mut context)
        .await
        .unwrap();

    assert_eq!(added.lead.id().get_version(), Some(Version::SortRand));
}

#[tokio::test]
async fn repeating_a_seeded_lead_changes_nothing() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap();

    let repeated = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap();

    assert!(!repeated.created);
    assert_eq!(store.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_seed_reused_for_other_data_is_refused() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap();

    let refused = add_lead(&store, form("Otro Lead", "DAC"), &mut context("one"))
        .await
        .unwrap_err();

    assert_eq!(refused.code(), "id_conflict");
    assert_eq!(refused.kind(), ErrorKind::Rejected);
    assert_eq!(store.list().await.unwrap()[0].name(), "Lead Uno");
}

#[tokio::test]
async fn advancing_to_the_final_stage_opens_one_case_with_seeded_ids() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let lead = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap()
        .lead;

    advance_lead(&store, lead.id(), &mut context("a"))
        .await
        .unwrap();
    advance_lead(&store, lead.id(), &mut context("b"))
        .await
        .unwrap();
    let last = advance_lead(&store, lead.id(), &mut context("c"))
        .await
        .unwrap();

    let mut ids = Ids::seeded("c");
    let file = last.case.unwrap();
    assert_eq!(last.lead.stage(), Stage::ContractSigned);
    assert_eq!(file.case.id(), ids.issue().unwrap());
    assert_eq!(file.steps[0].id(), ids.issue().unwrap());
    assert_eq!(file.steps[0].case_id(), file.case.id());
    assert_eq!(file.case.opened_on(), TODAY);
    assert_eq!(store.active_cases().await.unwrap().len(), 1);
}

#[tokio::test]
async fn advancing_a_signed_lead_is_refused_and_keeps_one_case() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (lead, _) = signed(&store, "s", "DAC").await;

    let refused = advance_lead(&store, lead, &mut context("x"))
        .await
        .unwrap_err();

    assert_eq!(refused.code(), "final_stage");
    assert_eq!(store.active_cases().await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_unknown_lead_is_a_rejection() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let refused = advance_lead(&store, Uuid::nil(), &mut context("x"))
        .await
        .unwrap_err();

    assert_eq!(refused.code(), "lead_not_found");
    assert_eq!(refused.kind(), ErrorKind::Rejected);
}

#[tokio::test]
async fn assigning_persists_the_responsible_person() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let lead = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap()
        .lead;

    assign_lead(&store, lead.id(), "Ana").await.unwrap();

    assert_eq!(store.list().await.unwrap()[0].owner(), Some("Ana"));
}

#[tokio::test]
async fn converting_a_prospect_prefills_a_lead_due_today() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    store
        .import(&[prospect("Empresa Demo", "DAC", None)])
        .await
        .unwrap();
    store
        .apply_tax_records(&[("Empresa Demo".to_owned(), tax(false))])
        .await
        .unwrap();

    let added = convert_prospect(
        &store,
        ProspectConversion {
            name: "Empresa Demo".to_owned(),
            phone: "987654321".to_owned(),
            ..ProspectConversion::default()
        },
        &mut context("p"),
    )
    .await
    .unwrap();

    assert_eq!(added.lead.name(), "Empresa Demo");
    assert_eq!(added.lead.service(), "DAC");
    assert_eq!(added.lead.follow_up_on(), TODAY);
    assert_eq!(
        added.lead.ruc().map(|ruc| ruc.as_str()),
        Some("20100070970")
    );
}

#[tokio::test]
async fn an_approximate_ruc_never_reaches_the_lead() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    store
        .import(&[prospect("Empresa Demo", "DAC", None)])
        .await
        .unwrap();
    store
        .apply_tax_records(&[("Empresa Demo".to_owned(), tax(true))])
        .await
        .unwrap();

    let added = convert_prospect(
        &store,
        ProspectConversion {
            name: "Empresa Demo".to_owned(),
            phone: "987654321".to_owned(),
            ..ProspectConversion::default()
        },
        &mut context("p"),
    )
    .await
    .unwrap();

    assert_eq!(added.lead.ruc(), None);
}

#[tokio::test]
async fn converting_needs_a_phone_because_sources_have_none() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    store
        .import(&[prospect("Empresa Demo", "DAC", None)])
        .await
        .unwrap();

    let refused = convert_prospect(
        &store,
        ProspectConversion {
            name: "Empresa Demo".to_owned(),
            ..ProspectConversion::default()
        },
        &mut context("p"),
    )
    .await
    .unwrap_err();

    assert_eq!(refused.code(), "missing_field");
    assert!(store.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_review_only_service_is_left_for_the_person_to_type() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    store
        .import(&[prospect("Empresa Demo", "REVISAR_MANUAL", None)])
        .await
        .unwrap();
    let conversion = |service: &str| ProspectConversion {
        name: "Empresa Demo".to_owned(),
        phone: "987654321".to_owned(),
        service: Some(service.to_owned()),
        ..ProspectConversion::default()
    };

    let added = convert_prospect(&store, conversion("IGAFOM"), &mut context("p"))
        .await
        .unwrap();

    assert_eq!(added.lead.service(), "IGAFOM");
}

#[tokio::test]
async fn an_unknown_prospect_is_a_rejection() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let refused = convert_prospect(
        &store,
        ProspectConversion {
            name: "Nadie".to_owned(),
            phone: "987654321".to_owned(),
            ..ProspectConversion::default()
        },
        &mut context("p"),
    )
    .await
    .unwrap_err();

    assert_eq!(refused.code(), "prospect_not_found");
}

#[tokio::test]
async fn a_name_shared_by_a_company_and_a_person_needs_a_kind() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let mut person = prospect("Doble Nombre", "DAC", None);
    person.kind = ProspectKind::Person;
    store
        .import(&[prospect("Doble Nombre", "IGAFOM", None), person])
        .await
        .unwrap();
    let conversion = |kind| ProspectConversion {
        name: "Doble Nombre".to_owned(),
        phone: "987654321".to_owned(),
        kind,
        ..ProspectConversion::default()
    };

    let refused = convert_prospect(&store, conversion(None), &mut context("p"))
        .await
        .unwrap_err();
    let added = convert_prospect(
        &store,
        conversion(Some(ProspectKind::Person)),
        &mut context("p"),
    )
    .await
    .unwrap();

    assert_eq!(refused.code(), "prospect_ambiguous");
    assert_eq!(added.lead.service(), "DAC");
}

#[tokio::test]
async fn marking_a_step_done_twice_keeps_the_first_date() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    let step = find_case(&store, case).await.unwrap().steps[0].id();

    let first = set_step_done(&store, step, true, TODAY).await.unwrap();
    let repeat = set_step_done(&store, step, true, date!(2026 - 04 - 01))
        .await
        .unwrap();

    let saved = find_case(&store, case).await.unwrap();
    assert!(first);
    assert!(!repeat);
    assert_eq!(saved.steps[0].done_on(), Some(TODAY));
}

#[tokio::test]
async fn undoing_a_step_clears_its_date() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    let step = find_case(&store, case).await.unwrap().steps[0].id();
    set_step_done(&store, step, true, TODAY).await.unwrap();

    set_step_done(&store, step, false, TODAY).await.unwrap();

    assert_eq!(
        find_case(&store, case).await.unwrap().steps[0].done_on(),
        None
    );
}

#[tokio::test]
async fn rescheduling_validates_the_typed_date() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    let step = find_case(&store, case).await.unwrap().steps[0].id();

    let refused = reschedule_step(&store, step, "mañana").await.unwrap_err();
    reschedule_step(&store, step, "2026-05-01").await.unwrap();

    assert_eq!(refused.code(), "invalid_date");
    assert_eq!(refused.to_string(), "Ingresa la fecha del paso");
    assert_eq!(
        find_case(&store, case).await.unwrap().steps[0].due_on(),
        date!(2026 - 05 - 01)
    );
}

#[tokio::test]
async fn an_unknown_step_is_a_rejection() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let refused = set_step_done(&store, Uuid::nil(), true, TODAY)
        .await
        .unwrap_err();

    assert_eq!(refused.code(), "step_not_found");
}

#[tokio::test]
async fn the_suggested_plan_uses_seeded_ids() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    let file = find_case(&store, case).await.unwrap();
    let draft = MilestoneValues::default();
    apply_billing(
        &store,
        &file,
        BillingEdit::Total("1000".to_owned()),
        &draft,
        &mut context("t"),
    )
    .await
    .unwrap();
    let file = find_case(&store, case).await.unwrap();

    apply_billing(
        &store,
        &file,
        BillingEdit::Suggest,
        &draft,
        &mut context("plan"),
    )
    .await
    .unwrap();

    let saved = find_case(&store, case).await.unwrap();
    let mut ids = Ids::seeded("plan");
    assert_eq!(saved.milestones.len(), 3);
    assert!(saved.milestones.iter().all(|m| m.case_id() == case));
    let expected: Vec<Uuid> = (0..3).map(|_| ids.issue().unwrap()).collect();
    let mut found: Vec<Uuid> = saved.milestones.iter().map(|m| m.id()).collect();
    found.sort();
    let mut wanted = expected;
    wanted.sort();
    assert_eq!(found, wanted);
}

#[tokio::test]
async fn repeating_a_seeded_milestone_adds_nothing() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    let draft = MilestoneValues {
        title: "Adelanto".to_owned(),
        amount: "100".to_owned(),
        due_on: "2026-03-20".to_owned(),
    };
    for _ in 0..2 {
        let file = find_case(&store, case).await.unwrap();
        apply_billing(&store, &file, BillingEdit::Add, &draft, &mut context("m"))
            .await
            .unwrap();
    }

    assert_eq!(find_case(&store, case).await.unwrap().milestones.len(), 1);
}

#[tokio::test]
async fn closing_a_recurring_case_twice_creates_one_seeded_renewal() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;

    let first = close_case(&store, case, &mut context("close"))
        .await
        .unwrap();
    let repeat = close_case(&store, case, &mut context("again")).await;

    let renewal = first.renewal.unwrap();
    assert_eq!(renewal.id(), Ids::seeded("close").issue().unwrap());
    assert_eq!(renewal.stage(), Stage::InitialContact);
    assert_eq!(renewal.renewal_of(), Some(case));
    assert_eq!(repeat.unwrap_err().code(), "case_not_active");
    let leads = store.list().await.unwrap();
    assert_eq!(
        leads
            .iter()
            .filter(|lead| lead.renewal_of().is_some())
            .count(),
        1
    );
}

#[tokio::test]
async fn closing_a_case_that_still_owes_money_twice_changes_nothing() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    let draft = MilestoneValues {
        title: "Saldo".to_owned(),
        amount: "100".to_owned(),
        due_on: "2026-03-20".to_owned(),
    };
    let file = find_case(&store, case).await.unwrap();
    apply_billing(&store, &file, BillingEdit::Add, &draft, &mut context("m"))
        .await
        .unwrap();

    close_case(&store, case, &mut context("close"))
        .await
        .unwrap();
    let repeat = close_case(&store, case, &mut context("again"))
        .await
        .unwrap();

    assert!(!repeat.closed);
    assert_eq!(repeat.renewal, None);
    assert_eq!(store.list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn the_agenda_reads_follow_ups_for_the_injected_day() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap();

    let before = agenda(&store, date!(2026 - 03 - 01)).await.unwrap();
    let same_day = agenda(&store, date!(2026 - 03 - 10)).await.unwrap();
    let after = agenda(&store, date!(2026 - 03 - 11)).await.unwrap();

    assert!(before.is_empty(), "nine days out stays beyond the agenda");
    assert_eq!(same_day.today.len(), 1);
    assert_eq!(after.overdue.len(), 1);
}

fn quote_values() -> QuoteValues {
    QuoteValues {
        lines: vec![QuoteLineValues {
            description: "Estudio".to_owned(),
            quantity: "2".to_owned(),
            unit_price: "100.00".to_owned(),
        }],
        ..QuoteValues::starting_on(TODAY)
    }
}

#[tokio::test]
async fn a_quote_is_found_by_number_or_id() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let lead = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap()
        .lead;
    let quote = save_quote_edit(&store, &lead, QuoteEdit::Save, &quote_values(), TODAY)
        .await
        .unwrap()
        .quote;

    let by_number = find_quote(&store, "COT-2026-0001").await.unwrap();
    let by_id = find_quote(&store, &quote.id().to_string()).await.unwrap();
    let unknown = find_quote(&store, "COT-2026-0099").await.unwrap_err();

    assert_eq!(by_number.id(), quote.id());
    assert_eq!(by_id.id(), quote.id());
    assert_eq!(unknown.code(), "quote_not_found");
}

#[tokio::test]
async fn sending_without_a_company_name_needs_an_explicit_choice() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let lead = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap()
        .lead;
    let quote = save_quote_edit(&store, &lead, QuoteEdit::Save, &quote_values(), TODAY)
        .await
        .unwrap()
        .quote;

    let refused = send_quote(&store, &lead, quote.clone(), false, TODAY)
        .await
        .unwrap_err();
    let unchanged = find_quote(&store, "COT-2026-0001").await.unwrap();
    let sent = send_quote(&store, &lead, quote, true, TODAY).await.unwrap();

    assert_eq!(refused.code(), "company_name_missing");
    assert_eq!(unchanged.status(), QuoteStatus::Draft);
    assert_eq!(sent.quote.status(), QuoteStatus::Sent);
}

#[tokio::test]
async fn a_saved_quote_pdf_lands_at_the_chosen_path() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let lead = add_lead(&store, form("Lead Uno", "DAC"), &mut context("one"))
        .await
        .unwrap()
        .lead;
    let quote = save_quote_edit(&store, &lead, QuoteEdit::Save, &quote_values(), TODAY)
        .await
        .unwrap()
        .quote;

    let path = save_quote_pdf(&store, &quote, &scratch.path().join("cotizacion"))
        .await
        .unwrap();

    assert_eq!(path.extension().unwrap(), "pdf");
    assert!(fs::read(path).unwrap().starts_with(b"%PDF-"));
}

#[tokio::test]
async fn settings_change_as_one_unit_or_not_at_all() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let refused = update_settings(
        &store,
        SettingsChange {
            follow_up_days: Some(30),
            igv: Some("abc".to_owned()),
            ..SettingsChange::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code(), "invalid_igv_rate");
    assert_eq!(store.load().await.unwrap().follow_up_days(), 3);

    update_settings(
        &store,
        SettingsChange {
            follow_up_days: Some(30),
            igv: Some("10.5".to_owned()),
            theme: Some("dark".to_owned()),
            company_name: Some("COMINORSA".to_owned()),
            add_members: vec!["Ana".to_owned()],
            ..SettingsChange::default()
        },
    )
    .await
    .unwrap();

    let saved = store.load().await.unwrap();
    assert_eq!(saved.follow_up_days(), 30);
    assert_eq!(saved.igv_rate().basis_points(), 1050);
    assert_eq!(saved.theme.code(), "dark");
    assert_eq!(saved.company().name, "COMINORSA");
    assert_eq!(saved.team(), ["Ana"]);
}

#[tokio::test]
async fn an_unknown_theme_is_refused() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let refused = update_settings(
        &store,
        SettingsChange {
            theme: Some("neon".to_owned()),
            ..SettingsChange::default()
        },
    )
    .await
    .unwrap_err();

    assert_eq!(refused.code(), "invalid_theme");
}

#[tokio::test]
async fn a_backup_needs_a_folder() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();

    let refused = run_backup(&store, None, &context("b")).await.unwrap_err();

    assert_eq!(refused.code(), "backup_folder_missing");
}

#[tokio::test]
async fn a_backup_lands_in_the_saved_folder_and_records_the_injected_instant() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let folder = scratch.path().join("copias");
    update_settings(
        &store,
        SettingsChange {
            backup_directory: Some(folder.display().to_string()),
            ..SettingsChange::default()
        },
    )
    .await
    .unwrap();

    let path = run_backup(&store, None, &context("b")).await.unwrap();

    assert_eq!(path, folder.join("crm-20260302-120000.db"));
    assert!(path.exists());
    assert_eq!(store.load().await.unwrap().last_backup_at(), Some(NOW));
}

#[tokio::test]
async fn a_restore_brings_the_backed_up_state_back_and_keeps_a_safety_copy() {
    let scratch = Scratch::new();
    let database = scratch.database();
    let store = SqliteLeads::open(&database).await.unwrap();
    let clock = Clock::fixed(Some(TODAY), Some(NOW));
    add_lead(&store, form("Antes", "DAC"), &mut context("one"))
        .await
        .unwrap();
    let backup = create_backup(
        &store,
        &scratch.path().join("copias").display().to_string(),
        &clock,
    )
    .await
    .unwrap();
    add_lead(&store, form("Después", "DAC"), &mut context("two"))
        .await
        .unwrap();

    let restored = restore_backup(store, &database, &backup, &clock)
        .await
        .unwrap();

    let leads = restored.store.list().await.unwrap();
    assert_eq!(leads.len(), 1);
    assert_eq!(leads[0].name(), "Antes");
    assert!(
        database
            .with_extension("antes-de-restaurar-20260302-120000.db")
            .exists()
    );
}

#[tokio::test]
async fn the_snapshot_orders_prospects_by_kind_and_name() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let mut person = prospect("Beta", "DAC", None);
    person.kind = ProspectKind::Person;
    store
        .import(&[
            prospect("Zeta", "DAC", None),
            person,
            prospect("Alfa", "DAC", None),
        ])
        .await
        .unwrap();

    let state = snapshot(&store).await.unwrap();

    let names: Vec<&str> = state.prospects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Alfa", "Zeta", "Beta"]);
}

#[tokio::test]
async fn the_snapshot_keeps_every_active_record() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (lead, _) = signed(&store, "s", "DAC").await;
    let found = store.list().await.unwrap();
    let signed_lead = found.iter().find(|item| item.id() == lead).unwrap();
    save_quote_edit(&store, signed_lead, QuoteEdit::Save, &quote_values(), TODAY)
        .await
        .unwrap();

    let state = snapshot(&store).await.unwrap();

    assert_eq!(state.leads.len(), 1);
    assert_eq!(state.cases.len(), 1);
    assert_eq!(state.quotes.len(), 1);
    assert_eq!(state.quotes[0].number().sequence(), 1);
    assert_eq!(store.quotes().await.unwrap().len(), 1);
}

#[tokio::test]
async fn status_counts_what_is_stored() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (_, case) = signed(&store, "s", "DAC").await;
    store
        .import(&[prospect("Empresa", "DAC", None)])
        .await
        .unwrap();
    update_settings(
        &store,
        SettingsChange {
            add_members: vec!["Ana".to_owned()],
            ..SettingsChange::default()
        },
    )
    .await
    .unwrap();

    let found = status(&store).await.unwrap();

    let file = find_case(&store, case).await.unwrap();
    assert_eq!(found.leads, 1);
    assert_eq!(found.active_cases, 1);
    assert_eq!(found.case_steps, file.steps.len());
    assert_eq!(found.billing_milestones, 0);
    assert_eq!(found.quotes, 0);
    assert_eq!(found.prospects, 1);
    assert_eq!(found.team_members, 1);
}
