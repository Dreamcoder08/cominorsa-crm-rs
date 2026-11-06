use cominorsa_desktop::{Board, FormValues, parse_form};
use cominorsa_domain::{Lead, NewLead};
use dioxus::prelude::*;
use time::macros::date;

fn sample(name: &str, follow_up_on: time::Date, advances: usize) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        follow_up_on,
    })
    .unwrap();
    for _ in 0..advances {
        lead.advance().unwrap();
    }
    lead
}

fn harness() -> Element {
    let leads = vec![
        sample("Atrasada", date!(2026 - 11 - 01), 0),
        sample("Futura", date!(2026 - 12 - 01), 1),
        sample("Cerrada", date!(2026 - 11 - 01), 3),
    ];

    rsx! {
        Board { leads, today: date!(2026 - 11 - 06), on_advance: |_| {} }
    }
}

fn render_board() -> String {
    let mut dom = VirtualDom::new(harness);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

#[test]
fn renders_every_pipeline_stage_with_its_leads() {
    let html = render_board();

    for label in [
        "Contacto inicial",
        "Evaluación de coordenadas",
        "Cotización enviada",
        "Contrato firmado",
    ] {
        assert!(html.contains(label), "missing stage {label}");
    }
    assert!(html.contains("Futura"));
}

#[test]
fn reports_only_open_overdue_leads() {
    let html = render_board();

    assert!(html.contains("1 seguimiento pendiente"));
    assert_eq!(html.matches("Vence: 2026-11-01").count(), 1);
}

#[test]
fn offers_no_advance_action_for_signed_contracts() {
    assert_eq!(render_board().matches("Avanzar").count(), 2);
}

fn form(date: &str, ruc: &str) -> FormValues {
    FormValues {
        name: "Minera".to_owned(),
        phone: "900000000".to_owned(),
        ruc: ruc.to_owned(),
        service: "REINFO".to_owned(),
        follow_up_on: date.to_owned(),
    }
}

#[test]
fn parses_form_values_into_a_lead() {
    let lead = parse_form(form("2026-11-10", "20123456786")).unwrap();

    assert_eq!(lead.name(), "Minera");
    assert_eq!(lead.follow_up_on(), date!(2026 - 11 - 10));
}

#[test]
fn explains_invalid_form_values() {
    assert_eq!(
        parse_form(form("", "")).unwrap_err(),
        "Ingresa la fecha de seguimiento"
    );
    assert_eq!(
        parse_form(form("2026-11-10", "123")).unwrap_err(),
        "El RUC no es válido"
    );
}

#[test]
fn starts_the_form_with_a_real_follow_up_date() {
    let values = FormValues::starting_on(date!(2026 - 11 - 06));

    assert_eq!(values.follow_up_on, "2026-11-06");
    assert!(values.name.is_empty());
}
