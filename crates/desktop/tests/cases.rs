use std::{env, fs, path::PathBuf};

use cominorsa_desktop::{
    Agenda, AgendaGroups, AgendaItem, CaseDetail, Cases, FollowUp, case_progress, parse_step_date,
    responsible_label, save_advance, step_status_label,
};
use cominorsa_domain::{
    Case, CaseFile, CaseRecord, CaseRepository, Lead, LeadRepository, NewLead, Stage,
    procedure_template,
};
use cominorsa_storage::SqliteLeads;
use dioxus::prelude::*;
use time::{Date, macros::date};
use uuid::Uuid;

const TODAY: Date = date!(2026 - 11 - 06);

fn lead(name: &str, service: &str, follow_up_on: Date, advances: usize) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: service.to_owned(),
        owner: Some("Ana".to_owned()),
        follow_up_on,
    })
    .unwrap();
    for _ in 0..advances {
        lead.advance().unwrap();
    }
    lead
}

/// A signed case with one step on each given date.
fn case_with(name: &str, dates: &[Date]) -> CaseFile {
    let signed = lead(name, "DAC", TODAY, 3);
    let mut file = CaseFile::open(&signed, TODAY, &procedure_template("DAC")).unwrap();
    file.steps.truncate(dates.len());
    for (step, date) in file.steps.iter_mut().zip(dates) {
        step.reschedule(*date);
    }
    file
}

fn render(component: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(component);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

fn step_names(group: &[AgendaItem]) -> Vec<String> {
    group
        .iter()
        .map(|item| format!("{}: {}", item.kind_label(), item.name()))
        .collect()
}

#[test]
fn puts_open_steps_in_the_agenda_groups_next_to_follow_ups_in_date_order() {
    let leads = vec![
        lead("Atrasado", "DAC", date!(2026 - 11 - 04), 0),
        lead("Hoy", "DAC", TODAY, 0),
        lead("Semana", "DAC", date!(2026 - 11 - 12), 0),
    ];
    let cases = vec![case_with(
        "Minera",
        &[
            date!(2026 - 11 - 03),
            date!(2026 - 11 - 05),
            TODAY,
            date!(2026 - 11 - 13),
            date!(2026 - 11 - 14),
        ],
    )];

    let groups = AgendaGroups::of(&leads, &cases, TODAY);

    assert_eq!(
        step_names(&groups.overdue),
        [
            "Trámite: Minera",
            "Seguimiento: Atrasado",
            "Trámite: Minera"
        ]
    );
    assert_eq!(
        step_names(&groups.today),
        ["Seguimiento: Hoy", "Trámite: Minera"]
    );
    assert_eq!(
        step_names(&groups.upcoming),
        ["Seguimiento: Semana", "Trámite: Minera"]
    );
    assert!(!groups.is_empty());
}

#[test]
fn leaves_done_steps_and_closed_cases_out_of_the_agenda() {
    let mut done = case_with("Hecho", &[TODAY]);
    done.steps[0].mark_done(TODAY);
    let open = case_with("Cerrado", &[TODAY]);
    let closed = CaseFile {
        case: Case::restore(CaseRecord {
            id: open.case.id(),
            lead_id: open.case.lead_id(),
            client_name: "Cerrado".to_owned(),
            ruc: None,
            service: "DAC".to_owned(),
            owner: None,
            opened_on: TODAY,
            status: "closed".to_owned(),
        })
        .unwrap(),
        steps: open.steps,
    };

    let groups = AgendaGroups::of(&[], &[done, closed], TODAY);

    assert!(groups.is_empty());
}

#[test]
fn keeps_signed_leads_out_of_the_agenda_while_their_steps_appear() {
    let signed = lead("Firmada", "DAC", date!(2026 - 11 - 01), 3);
    let file = case_with("Firmada", &[TODAY]);

    let groups = AgendaGroups::of(std::slice::from_ref(&signed), &[file], TODAY);

    assert_eq!(FollowUp::of(&signed, TODAY), FollowUp::Closed);
    assert_eq!(groups.overdue, Vec::new());
    assert_eq!(step_names(&groups.today), ["Trámite: Firmada"]);
}

fn agenda_with_step() -> Element {
    rsx! {
        Agenda {
            leads: vec![lead("Comercial", "DAC", TODAY, 0)],
            cases: vec![case_with("Minera Sur", &[date!(2026 - 11 - 04)])],
            today: TODAY,
            on_open_pipeline: |_| {},
        }
    }
}

#[test]
fn shows_which_agenda_row_is_a_follow_up_and_which_a_case_step() {
    let html = render(agenda_with_step);

    assert!(html.contains("Vencidos · 1"));
    assert!(html.contains("Hoy · 1"));
    assert!(html.contains(">Seguimiento<"));
    assert!(html.contains(">Trámite<"));
    assert!(html.contains("Minera Sur"));
    assert!(html.contains("Confirmar el cronograma vigente"));
    assert!(html.contains("Fecha sugerida 04/11/2026"));
    assert!(html.contains("Vencido hace 2 días"));
    assert!(html.contains("Ver expediente"));
}

fn cases_list() -> Element {
    let mut file = case_with("Minera Norte", &[TODAY, date!(2026 - 11 - 20)]);
    file.steps[0].mark_done(TODAY);

    rsx! {
        Cases { cases: vec![file], on_open: |_| {} }
    }
}

fn empty_cases() -> Element {
    rsx! {
        Cases { cases: Vec::new(), on_open: |_| {} }
    }
}

#[test]
fn lists_open_cases_and_explains_the_empty_state() {
    let html = render(cases_list);

    assert!(html.contains("Expedientes"));
    assert!(html.contains("Minera Norte"));
    assert!(html.contains("DAC"));
    assert!(html.contains("Ana"));
    assert!(html.contains("1 de 2 pasos"));
    assert!(html.contains("Ver expediente"));

    let empty = render(empty_cases);
    assert!(empty.contains("Sin expedientes"));
    assert!(empty.contains("Contrato firmado"));
}

fn detail_props(saving: bool) -> Element {
    let mut file = case_with("Minera Norte", &[date!(2026 - 11 - 05), TODAY]);
    file.steps[1].mark_done(TODAY);

    rsx! {
        CaseDetail {
            file,
            today: TODAY,
            saving,
            error: if saving { String::new() } else { "No se pudo guardar".to_owned() },
            on_back: |_| {},
            on_toggle: |_| {},
            on_due_date: |_| {},
        }
    }
}

fn case_detail() -> Element {
    detail_props(false)
}

fn saving_case_detail() -> Element {
    detail_props(true)
}

#[test]
fn shows_the_case_data_and_steps_with_suggested_editable_dates() {
    let html = render(case_detail);

    for text in [
        "Minera Norte",
        "20123456786",
        "DAC",
        "Ana",
        "06/11/2026",
        "Confirmar el cronograma vigente",
        "Vencido hace 1 día",
        "Hecho el 06/11/2026",
        "No se pudo guardar",
    ] {
        assert!(html.contains(text), "missing {text}");
    }
    assert_eq!(html.matches("Fecha sugerida, ajústala").count(), 2);
    assert_eq!(html.matches("type=\"checkbox\"").count(), 2);
    assert_eq!(html.matches("type=\"date\"").count(), 2);
    assert!(html.contains("value=\"2026-11-05\""));
    assert_eq!(html.matches("disabled=true").count(), 0);
}

#[test]
fn disables_step_edits_while_a_save_is_pending() {
    let html = render(saving_case_detail);

    assert_eq!(html.matches("disabled=true").count(), 4);
}

#[test]
fn describes_steps_cases_and_owners() {
    let mut file = case_with(
        "Minera",
        &[date!(2026 - 11 - 04), TODAY, date!(2026 - 11 - 09)],
    );
    let [overdue, due_today, upcoming] = [&file.steps[0], &file.steps[1], &file.steps[2]];

    assert_eq!(step_status_label(overdue, TODAY), "Vencido hace 2 días");
    assert_eq!(step_status_label(due_today, TODAY), "Vence hoy");
    assert_eq!(step_status_label(upcoming, TODAY), "Próximo");
    assert_eq!(FollowUp::of_step(upcoming, TODAY).class(), "upcoming");
    assert_eq!(case_progress(&file), "0 de 3 pasos");

    file.steps[0].mark_done(TODAY);
    assert_eq!(FollowUp::of_step(&file.steps[0], TODAY), FollowUp::Closed);
    assert_eq!(
        step_status_label(&file.steps[0], TODAY),
        "Hecho el 06/11/2026"
    );
    assert_eq!(case_progress(&file), "1 de 3 pasos");

    assert_eq!(responsible_label(Some("Ana")), "Ana");
    assert_eq!(responsible_label(None), "Sin responsable");
}

#[test]
fn parses_an_edited_step_date_and_explains_a_blank_one() {
    assert_eq!(parse_step_date("2026-12-01"), Ok(date!(2026 - 12 - 01)));
    assert_eq!(
        parse_step_date(""),
        Err("Ingresa la fecha del paso".to_owned())
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

#[tokio::test]
async fn advancing_to_a_middle_stage_saves_the_lead_and_opens_no_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let initial = lead("Minera", "DAC", date!(2026 - 11 - 10), 0);
    store.save(&initial).await.unwrap();

    save_advance(&store, initial.clone(), TODAY).await.unwrap();

    let saved = store.list().await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id(), initial.id());
    assert_eq!(saved[0].stage(), Stage::CoordinateEvaluation);
    assert_eq!(saved[0].follow_up_on(), initial.follow_up_on());
    assert_eq!(saved[0].owner(), Some("Ana"));
    assert_eq!(store.open_cases().await.unwrap(), Vec::new());
}

#[tokio::test]
async fn advancing_to_the_final_stage_signs_the_lead_and_opens_its_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let quoted = lead("Minera", "Plan de minado", date!(2026 - 11 - 10), 2);
    store.save(&quoted).await.unwrap();

    save_advance(&store, quoted.clone(), TODAY).await.unwrap();

    assert_eq!(
        store.list().await.unwrap()[0].stage(),
        Stage::ContractSigned
    );
    let cases = store.open_cases().await.unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].case.lead_id(), quoted.id());
    assert_eq!(cases[0].case.owner(), Some("Ana"));
    assert_eq!(cases[0].case.opened_on(), TODAY);
    assert_eq!(
        cases[0].steps.len(),
        procedure_template("Plan de minado").steps.len()
    );
}

#[tokio::test]
async fn repeating_the_final_advance_keeps_one_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let quoted = lead("Minera", "DAC", date!(2026 - 11 - 10), 2);
    store.save(&quoted).await.unwrap();

    save_advance(&store, quoted.clone(), TODAY).await.unwrap();
    save_advance(&store, quoted, date!(2026 - 11 - 09))
        .await
        .unwrap();

    let cases = store.open_cases().await.unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].case.opened_on(), TODAY);
}

#[tokio::test]
async fn advancing_a_signed_lead_is_refused_and_opens_no_case() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let signed = lead("Minera", "DAC", date!(2026 - 11 - 10), 3);
    store.save(&signed).await.unwrap();

    let refused = save_advance(&store, signed, TODAY).await;

    assert_eq!(refused, Err("El lead ya está en la etapa final".to_owned()));
    assert_eq!(store.open_cases().await.unwrap(), Vec::new());
}
