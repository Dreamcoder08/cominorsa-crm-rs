use std::{env, fs, path::PathBuf};

use cominorsa_desktop::{
    Agenda, AgendaGroups, AgendaItem, BillingChange, BillingEdit, BillingPanel, CaseDetail, Cases,
    FollowUp, MilestoneValues, Workspace, billing_change, close_warning, closing_notice,
    format_soles, milestone_status_label, money_input, renewal_note, save_billing, save_close,
};
use cominorsa_domain::{
    BillingMilestone, Case, CaseFile, CaseRecord, CaseRepository, CaseStatus, Lead, LeadRepository,
    Money, NewLead, NewMilestone, Stage, procedure_template,
};
use cominorsa_storage::SqliteLeads;
use dioxus::prelude::*;
use time::{Date, macros::date};
use uuid::Uuid;

const TODAY: Date = date!(2026 - 11 - 06);

fn soles(cents: i64) -> Money {
    Money::from_cents(cents).unwrap()
}

fn lead(name: &str, service: &str, advances: usize) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: service.to_owned(),
        owner: Some("Ana".to_owned()),
        follow_up_on: TODAY,
    })
    .unwrap();
    for _ in 0..advances {
        lead.advance().unwrap();
    }
    lead
}

fn case_file(name: &str, service: &str) -> CaseFile {
    let signed = lead(name, service, 3);
    let mut file = CaseFile::open(&signed, TODAY, &procedure_template(service)).unwrap();
    file.steps.clear();
    file
}

fn milestone(file: &CaseFile, position: u32, cents: i64, due_on: Date) -> BillingMilestone {
    BillingMilestone::create(NewMilestone {
        case_id: file.case.id(),
        position,
        title: format!("Hito {position}"),
        amount: soles(cents),
        due_on,
    })
    .unwrap()
}

fn closed(file: &CaseFile) -> Case {
    Case::restore(CaseRecord {
        id: file.case.id(),
        lead_id: file.case.lead_id(),
        client_name: file.case.client_name().to_owned(),
        ruc: None,
        service: file.case.service().to_owned(),
        owner: None,
        opened_on: TODAY,
        status: "closed".to_owned(),
        contract_total_cents: None,
    })
    .unwrap()
}

fn render(component: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(component);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

fn labels(group: &[AgendaItem]) -> Vec<String> {
    group
        .iter()
        .map(|item| format!("{}: {}", item.kind_label(), item.name()))
        .collect()
}

#[test]
fn formats_soles_with_grouping_and_two_decimals() {
    for (cents, text) in [
        (123_450, "S/ 1,234.50"),
        (0, "S/ 0.00"),
        (5, "S/ 0.05"),
        (100, "S/ 1.00"),
        (99_999, "S/ 999.99"),
        (100_000_000, "S/ 1,000,000.00"),
    ] {
        assert_eq!(format_soles(soles(cents)), text);
    }
}

#[test]
fn shows_an_editable_amount_that_reads_back_exactly() {
    assert_eq!(money_input(soles(123_450)), "1234.50");
    assert_eq!(Money::parse(&money_input(soles(5))), Ok(soles(5)));
}

#[test]
fn puts_unpaid_milestones_in_the_agenda_groups_in_date_order() {
    let leads = vec![lead("Atrasado", "DAC", 0)];
    let mut file = case_file("Minera", "DAC");
    let mut paid = milestone(&file, 1, 100, TODAY);
    paid.mark_paid(TODAY);
    file.milestones = vec![
        paid,
        milestone(&file, 2, 100, date!(2026 - 11 - 04)),
        milestone(&file, 3, 100, TODAY),
        milestone(&file, 4, 100, date!(2026 - 11 - 13)),
        milestone(&file, 5, 100, date!(2026 - 11 - 14)),
    ];

    let groups = AgendaGroups::of(&leads, &[file], TODAY);

    assert_eq!(labels(&groups.overdue), ["Cobro: Minera"]);
    assert_eq!(
        labels(&groups.today),
        ["Seguimiento: Atrasado", "Cobro: Minera"]
    );
    assert_eq!(labels(&groups.upcoming), ["Cobro: Minera"]);
}

#[test]
fn money_owed_on_a_closed_case_stays_in_the_agenda_without_its_steps() {
    let open = case_file("Minera", "REINFO");
    let mut file = CaseFile {
        case: closed(&open),
        steps: CaseFile::open(
            &lead("Minera", "REINFO", 3),
            TODAY,
            &procedure_template("REINFO"),
        )
        .unwrap()
        .steps,
        milestones: Vec::new(),
    };
    file.milestones = vec![milestone(&file, 1, 100, TODAY)];

    let groups = AgendaGroups::of(&[], &[file], TODAY);

    assert_eq!(labels(&groups.today), ["Cobro: Minera"]);
    assert!(groups.overdue.is_empty() && groups.upcoming.is_empty());
}

fn agenda_with_milestone() -> Element {
    let mut file = case_file("Minera Sur", "DAC");
    file.milestones = vec![milestone(&file, 1, 30_000, date!(2026 - 11 - 04))];

    rsx! {
        Agenda {
            leads: vec![lead("Comercial", "DAC", 0)],
            cases: vec![file],
            today: TODAY,
            on_open_pipeline: |_| {},
        }
    }
}

#[test]
fn tells_a_collection_apart_from_follow_ups_in_the_agenda() {
    let html = render(agenda_with_milestone);

    for text in [
        "agenda-kind billing",
        ">Cobro<",
        ">Seguimiento<",
        "Minera Sur",
        "Hito 1 · S/ 300.00 · Vence 04/11/2026",
        "Vencido hace 2 días",
        "Ver expediente",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
}

#[test]
fn describes_a_milestone_by_its_payment_and_due_date() {
    let file = case_file("Minera", "DAC");
    let mut item = milestone(&file, 1, 100, date!(2026 - 11 - 05));

    assert_eq!(milestone_status_label(&item, TODAY), "Vencido hace 1 día");
    assert_eq!(FollowUp::of_milestone(&item, TODAY).class(), "overdue");

    item.reschedule(TODAY);
    assert_eq!(milestone_status_label(&item, TODAY), "Vence hoy");

    item.reschedule(date!(2026 - 11 - 20));
    assert_eq!(milestone_status_label(&item, TODAY), "Próximo");

    item.mark_paid(TODAY);
    assert_eq!(FollowUp::of_milestone(&item, TODAY), FollowUp::Closed);
    assert_eq!(milestone_status_label(&item, TODAY), "Pagado el 06/11/2026");
}

fn billing_panel(file: CaseFile, saving: bool) -> Element {
    rsx! {
        BillingPanel {
            file,
            today: TODAY,
            saving,
            draft: MilestoneValues::starting_on(TODAY),
            on_draft: |_| {},
            on_edit: |_| {},
        }
    }
}

fn panel_with_milestones() -> Element {
    let mut file = case_file("Minera", "DAC");
    file.case.set_contract_total(soles(1_000_000)).unwrap();
    let mut paid = milestone(&file, 1, 300_000, TODAY);
    paid.mark_paid(TODAY);
    file.milestones = vec![paid, milestone(&file, 2, 700_050, date!(2026 - 12 - 01))];
    billing_panel(file, false)
}

fn panel_saving() -> Element {
    let mut file = case_file("Minera", "DAC");
    file.case.set_contract_total(soles(1_000_000)).unwrap();
    file.milestones = vec![milestone(&file, 1, 300_000, TODAY)];
    billing_panel(file, true)
}

fn panel_ready_for_a_plan() -> Element {
    let mut file = case_file("Minera", "DAC");
    file.case.set_contract_total(soles(1_000_000)).unwrap();
    billing_panel(file, false)
}

fn panel_without_total() -> Element {
    billing_panel(case_file("Minera", "DAC"), false)
}

#[test]
fn shows_total_collected_and_pending_and_every_milestone() {
    let html = render(panel_with_milestones);

    for text in [
        "Total del contrato",
        "S/ 10,000.00",
        "Cobrado",
        "S/ 3,000.00",
        "Pendiente",
        "S/ 7,000.00",
        "value=\"Hito 2\"",
        "value=\"7000.50\"",
        "value=\"2026-12-01\"",
        "Pagado el 06/11/2026",
        "Fecha sugerida, ajústala",
        "Eliminar Hito 2",
        "Agregar hito",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
}

#[test]
fn says_plainly_that_it_tracks_collections_and_issues_no_invoices() {
    let html = render(panel_without_total);

    assert!(html.contains("Registra cobros; no emite facturas ni comprobantes."));
}

#[test]
fn offers_the_plan_as_an_editable_suggestion_only_when_it_can_apply() {
    let ready = render(panel_ready_for_a_plan);
    assert!(ready.contains("Aplicar plan sugerido 30 / 40 / 30"));
    assert!(ready.contains("Sugerencia editable: el reparto viene de un ejemplo sin validar."));

    assert!(!render(panel_with_milestones).contains("Aplicar plan"));

    let without_total = render(panel_without_total);
    assert!(!without_total.contains("Aplicar plan"));
    assert!(without_total.contains("Sin definir"));
}

#[test]
fn disables_every_billing_control_while_a_save_is_pending() {
    let html = render(panel_saving);

    // Total, plan-free row (paid, title, amount, date, delete), and the new form.
    assert_eq!(html.matches("disabled=true").count(), 10);
    assert_eq!(
        render(panel_with_milestones)
            .matches("disabled=true")
            .count(),
        0
    );
}

fn detail(file: CaseFile, confirming_close: bool, saving: bool) -> Element {
    rsx! {
        CaseDetail {
            file,
            today: TODAY,
            saving,
            error: String::new(),
            notice: "Expediente cerrado.".to_owned(),
            confirming_close,
            on_close: |_| {},
            draft: MilestoneValues::starting_on(TODAY),
            on_back: |_| {},
            on_toggle: |_| {},
            on_due_date: |_| {},
        }
    }
}

fn open_detail() -> Element {
    detail(case_file("Minera", "DAC"), false, false)
}

fn confirming_recurring() -> Element {
    detail(case_file("Minera", "DAC"), true, false)
}

fn confirming_while_saving() -> Element {
    detail(case_file("Minera", "DAC"), true, true)
}

fn confirming_once_with_money_owed() -> Element {
    let mut file = case_file("Minera", "REINFO");
    file.milestones = vec![milestone(&file, 1, 123_450, TODAY)];
    detail(file, true, false)
}

fn closed_detail() -> Element {
    let open = case_file("Minera", "DAC");
    let mut file = CaseFile {
        case: closed(&open),
        steps: Vec::new(),
        milestones: Vec::new(),
    };
    file.milestones = vec![milestone(&file, 1, 100, TODAY)];
    detail(file, false, false)
}

#[test]
fn closing_a_case_is_an_explicit_action_that_needs_confirmation() {
    let html = render(open_detail);

    assert!(html.contains("Cerrar expediente"));
    assert!(!html.contains("Sí, cerrar expediente"));
    assert!(html.contains("Expediente cerrado."));
}

#[test]
fn the_confirmation_explains_the_renewal_and_that_nothing_is_sent() {
    let html = render(confirming_recurring);

    for text in [
        "¿Cerrar el expediente de Minera?",
        "Se creará un lead de renovación con seguimiento sugerido el 06/11/2027.",
        "Ningún mensaje se envía solo.",
        "Sí, cerrar expediente",
        "Cancelar",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
    assert!(!html.contains(">Cerrar expediente<"));
}

#[test]
fn the_confirmation_warns_about_money_owed_and_a_service_that_does_not_recur() {
    let html = render(confirming_once_with_money_owed);

    assert!(html.contains("Este servicio no genera renovación."));
    assert!(html.contains("Quedan S/ 1,234.50 por cobrar; seguirán en la Agenda."));
    assert!(!html.contains("Se creará un lead"));
}

#[test]
fn disables_the_confirmation_while_closing() {
    let html = render(confirming_while_saving);

    assert!(
        html.contains("disabled=true class=\"danger\"")
            || html.contains("class=\"danger\" disabled=true")
    );
    assert!(
        html.contains("disabled=true class=\"secondary\"")
            || html.contains("class=\"secondary\" disabled=true")
    );
}

#[test]
fn a_closed_case_offers_no_closing_and_shows_its_state() {
    let html = render(closed_detail);

    assert!(!html.contains(">Cerrar expediente<"));
    assert!(html.contains("Cerrado, con cobros pendientes"));
}

fn cases_with_closed() -> Element {
    let open = case_file("Minera", "DAC");
    let mut file = CaseFile {
        case: closed(&open),
        steps: Vec::new(),
        milestones: Vec::new(),
    };
    file.milestones = vec![milestone(&file, 1, 100, TODAY)];

    rsx! {
        Cases {
            cases: vec![file],
            notice: "Expediente cerrado.".to_owned(),
            on_open: |_| {},
        }
    }
}

#[test]
fn lists_a_closed_case_that_still_owes_money_and_the_closing_notice() {
    let html = render(cases_with_closed);

    assert!(html.contains("Cerrado, con cobros pendientes"));
    assert!(html.contains("Expediente cerrado."));
}

#[test]
fn explains_what_closing_does_for_each_service() {
    let recurring = case_file("Minera", "DAC");
    let once = case_file("Minera", "REINFO");

    assert_eq!(
        renewal_note(&recurring.case, TODAY),
        "Se creará un lead de renovación con seguimiento sugerido el 06/11/2027. \
         Ningún mensaje se envía solo."
    );
    assert_eq!(
        renewal_note(&once.case, TODAY),
        "Este servicio no genera renovación."
    );
    assert_eq!(close_warning(&recurring), None);
}

#[test]
fn reports_the_outcome_of_a_closing() {
    let source = lead("Minera", "DAC", 3);
    let file = CaseFile::open(&source, TODAY, &procedure_template("DAC")).unwrap();
    let renewal = cominorsa_domain::CaseClosure::of(file.case, &source, TODAY).renewal;

    assert_eq!(closing_notice(None), "Expediente cerrado.");
    assert_eq!(
        closing_notice(renewal.as_ref()),
        "Expediente cerrado. Se creó «Renovación: Minera» en el Embudo."
    );
}

fn workspace_with_renewal() -> Element {
    let source = lead("Minera", "DAC", 3);
    let file = CaseFile::open(&source, TODAY, &procedure_template("DAC")).unwrap();
    let renewal = cominorsa_domain::CaseClosure::of(file.case, &source, TODAY)
        .renewal
        .unwrap();

    rsx! {
        Workspace {
            leads: vec![renewal, lead("Nueva", "DAC", 0)],
            today: TODAY,
            query: String::new(),
            whatsapp_template: "Hola {{nombre}}".replace("{{", "{").replace("}}", "}"),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

#[test]
fn marks_a_renewal_as_an_existing_client_and_keeps_messages_human() {
    let html = render(workspace_with_renewal);

    assert_eq!(html.matches("Cliente existente").count(), 1);
    assert!(html.contains("Renovación: Minera"));
    assert!(html.contains("Hola%20Minera"));
    assert_eq!(html.matches(">WhatsApp<").count(), 2);
}

fn draft(title: &str, amount: &str, due_on: &str) -> MilestoneValues {
    MilestoneValues {
        title: title.to_owned(),
        amount: amount.to_owned(),
        due_on: due_on.to_owned(),
    }
}

fn blank() -> MilestoneValues {
    MilestoneValues::starting_on(TODAY)
}

#[test]
fn starts_a_new_milestone_with_a_real_date() {
    assert_eq!(blank(), draft("", "", "2026-11-06"));
}

#[test]
fn reads_the_contract_total_and_explains_a_bad_one() {
    let file = case_file("Minera", "DAC");

    assert_eq!(
        billing_change(
            &file,
            BillingEdit::Total("1,234.50".to_owned()),
            &blank(),
            TODAY
        ),
        Ok(BillingChange::Total(file.case.id(), soles(123_450)))
    );
    assert_eq!(
        billing_change(
            &file,
            BillingEdit::Total("1234,50".to_owned()),
            &blank(),
            TODAY
        )
        .map_err(|error| error.to_string()),
        Err("Ingresa un monto mayor que cero, por ejemplo 1,234.50".to_owned())
    );
}

#[test]
fn applies_the_suggested_plan_and_explains_a_missing_total() {
    let mut file = case_file("Minera", "DAC");

    assert_eq!(
        billing_change(&file, BillingEdit::Suggest, &blank(), TODAY)
            .map_err(|error| error.to_string()),
        Err("Ingresa primero el total del contrato".to_owned())
    );

    file.case.set_contract_total(soles(100_001)).unwrap();
    let Ok(BillingChange::Add(plan)) = billing_change(&file, BillingEdit::Suggest, &blank(), TODAY)
    else {
        panic!("expected a plan");
    };
    let cents: Vec<i64> = plan.iter().map(|item| item.amount().cents()).collect();
    assert_eq!(cents, [30_000, 40_000, 30_001]);
}

#[test]
fn adds_a_milestone_from_the_form_after_the_last_position() {
    let mut file = case_file("Minera", "DAC");
    file.milestones = vec![milestone(&file, 3, 100, TODAY)];
    let values = draft(" Saldo ", "1,500", "2026-12-01");

    let Ok(BillingChange::Add(added)) = billing_change(&file, BillingEdit::Add, &values, TODAY)
    else {
        panic!("expected a milestone");
    };

    assert_eq!(added.len(), 1);
    assert_eq!(added[0].case_id(), file.case.id());
    assert_eq!(added[0].position(), 4);
    assert_eq!(added[0].title(), "Saldo");
    assert_eq!(added[0].amount(), soles(150_000));
    assert_eq!(added[0].due_on(), date!(2026 - 12 - 01));
}

#[test]
fn explains_every_invalid_new_milestone() {
    let file = case_file("Minera", "DAC");
    let refused = |values: MilestoneValues| {
        billing_change(&file, BillingEdit::Add, &values, TODAY).unwrap_err()
    };

    assert_eq!(
        refused(draft("", "100", "2026-12-01")),
        "El campo concepto es obligatorio"
    );
    assert_eq!(
        refused(draft("Saldo", "", "2026-12-01")),
        "Ingresa un monto mayor que cero, por ejemplo 1,234.50"
    );
    assert_eq!(
        refused(draft("Saldo", "100", "")),
        "Ingresa la fecha del hito"
    );
}

#[test]
fn edits_a_milestone_one_field_at_a_time() {
    let file = case_file("Minera", "DAC");
    let item = milestone(&file, 1, 100, TODAY);
    let edited = |edit: BillingEdit| {
        let Ok(BillingChange::Save(saved)) = billing_change(&file, edit, &blank(), TODAY) else {
            panic!("expected a save");
        };
        saved
    };

    let renamed = edited(BillingEdit::Title(item.clone(), "Saldo".to_owned()));
    assert_eq!(renamed.title(), "Saldo");
    let priced = edited(BillingEdit::Amount(item.clone(), "12.50".to_owned()));
    assert_eq!(priced.amount(), soles(1_250));
    let moved = edited(BillingEdit::DueDate(item.clone(), "2027-01-31".to_owned()));
    assert_eq!(moved.due_on(), date!(2027 - 01 - 31));

    let paid = edited(BillingEdit::TogglePaid(item.clone()));
    assert_eq!(paid.paid_on(), Some(TODAY));
    let unpaid = edited(BillingEdit::TogglePaid(paid));
    assert_eq!(unpaid.paid_on(), None);

    assert_eq!(
        billing_change(&file, BillingEdit::Delete(item.id()), &blank(), TODAY),
        Ok(BillingChange::Delete(item.id()))
    );
    assert_eq!(
        billing_change(
            &file,
            BillingEdit::Title(item.clone(), " ".to_owned()),
            &blank(),
            TODAY
        )
        .map_err(|error| error.to_string()),
        Err("El campo concepto es obligatorio".to_owned())
    );
    assert_eq!(
        billing_change(
            &file,
            BillingEdit::DueDate(item, String::new()),
            &blank(),
            TODAY
        )
        .map_err(|error| error.to_string()),
        Err("Ingresa la fecha del hito".to_owned())
    );
}

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

async fn signed_in(store: &SqliteLeads, service: &str) -> (Lead, CaseFile) {
    let source = lead("Minera", service, 3);
    let file = CaseFile::open(&source, TODAY, &procedure_template(service)).unwrap();
    store.sign_and_open(&source, &file).await.unwrap();
    (source, file)
}

#[tokio::test]
async fn saves_every_billing_change_through_the_port() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = signed_in(&store, "DAC").await;
    let first = milestone(&file, 1, 100, TODAY);
    let second = milestone(&file, 2, 200, TODAY);

    save_billing(&store, BillingChange::Total(file.case.id(), soles(300)))
        .await
        .unwrap();
    save_billing(
        &store,
        BillingChange::Add(vec![first.clone(), second.clone()]),
    )
    .await
    .unwrap();
    let mut paid = second.clone();
    paid.mark_paid(TODAY);
    save_billing(&store, BillingChange::Save(paid.clone()))
        .await
        .unwrap();
    save_billing(&store, BillingChange::Delete(first.id()))
        .await
        .unwrap();

    let cases = store.open_cases().await.unwrap();
    assert_eq!(cases[0].case.contract_total(), Some(soles(300)));
    assert_eq!(cases[0].milestones, vec![paid]);
}

#[tokio::test]
async fn reports_a_billing_save_failure() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();

    let failed = save_billing(&store, BillingChange::Delete(Uuid::now_v7())).await;

    assert_eq!(
        failed.map_err(|error| error.to_string()),
        Err("No se pudo acceder a los datos: El hito de cobro ya no existe".to_owned())
    );
}

#[tokio::test]
async fn closing_a_recurring_case_puts_its_renewal_in_the_first_stage() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (source, file) = signed_in(&store, "DAC").await;

    let renewal = save_close(&store, file.case.clone(), TODAY)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(renewal.name(), "Renovación: Minera");
    assert_eq!(renewal.phone(), source.phone());
    assert_eq!(renewal.stage(), Stage::InitialContact);
    assert_eq!(renewal.follow_up_on(), date!(2027 - 11 - 06));
    assert_eq!(renewal.renewal_of(), Some(file.case.id()));
    assert_eq!(store.list().await.unwrap().len(), 2);
    assert_eq!(store.open_cases().await.unwrap(), Vec::new());
}

#[tokio::test]
async fn repeating_a_close_reports_no_new_renewal() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (_, file) = signed_in(&store, "DAC").await;

    save_close(&store, file.case.clone(), TODAY).await.unwrap();
    let repeat = save_close(&store, file.case, TODAY).await.unwrap();

    assert_eq!(repeat, None);
    assert_eq!(store.list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn closing_a_one_off_case_creates_no_lead() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let (source, file) = signed_in(&store, "IGAFOM + REINFO").await;

    let renewal = save_close(&store, file.case, TODAY).await.unwrap();

    assert_eq!(renewal, None);
    assert_eq!(store.list().await.unwrap(), vec![source]);
    assert_eq!(store.open_cases().await.unwrap(), Vec::new());
}

#[tokio::test]
async fn refuses_to_close_a_case_whose_lead_is_missing() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let file = case_file("Minera", "DAC");

    let refused = save_close(&store, file.case.clone(), TODAY).await;

    assert_eq!(
        refused.map_err(|error| error.to_string()),
        Err("No se encontró el lead del expediente".to_owned())
    );
    assert_eq!(file.case.status(), CaseStatus::Open);
}
