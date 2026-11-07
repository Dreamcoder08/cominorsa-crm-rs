use cominorsa_domain::{
    Case, CaseFile, CaseRecord, CaseStatus, CaseStep, CaseStepRecord, DomainError, Lead, NewLead,
    Ruc, procedure_template,
};
use time::{Date, Duration, macros::date};
use uuid::Uuid;

const SIGNED_ON: Date = date!(2026 - 11 - 07);

fn lead(service: &str, advances: usize) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: "Minera Ejemplo".to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: service.to_owned(),
        owner: Some("Ana".to_owned()),
        follow_up_on: date!(2026 - 11 - 10),
    })
    .unwrap();
    for _ in 0..advances {
        lead.advance().unwrap();
    }
    lead
}

fn step_record() -> CaseStepRecord {
    CaseStepRecord {
        id: Uuid::now_v7(),
        case_id: Uuid::now_v7(),
        position: 1,
        title: "Reunir documentos".to_owned(),
        due_on: date!(2026 - 11 - 10),
        done_on: None,
    }
}

fn case_record(status: &str) -> CaseRecord {
    CaseRecord {
        id: Uuid::now_v7(),
        lead_id: Uuid::now_v7(),
        client_name: "Minera Ejemplo".to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        owner: None,
        opened_on: SIGNED_ON,
        status: status.to_owned(),
    }
}

#[test]
fn opens_a_case_from_a_signed_lead_with_its_data_and_responsible_person() {
    let signed = lead("REINFO", 3);
    let template = procedure_template(signed.service());

    let opened = CaseFile::open(&signed, SIGNED_ON, &template).unwrap();

    assert_eq!(opened.case.lead_id(), signed.id());
    assert_eq!(opened.case.client_name(), "Minera Ejemplo");
    assert_eq!(opened.case.ruc().map(Ruc::as_str), Some("20123456786"));
    assert_eq!(opened.case.service(), "REINFO");
    assert_eq!(opened.case.owner(), Some("Ana"));
    assert_eq!(opened.case.opened_on(), SIGNED_ON);
    assert_eq!(opened.case.status(), CaseStatus::Open);
}

#[test]
fn builds_one_ordered_pending_step_per_template_entry_from_the_signing_date() {
    let signed = lead("IGAFOM", 3);
    let template = procedure_template(signed.service());

    let opened = CaseFile::open(&signed, SIGNED_ON, &template).unwrap();

    assert_eq!(opened.steps.len(), template.steps.len());
    for (index, (step, planned)) in opened.steps.iter().zip(template.steps).enumerate() {
        assert_eq!(step.case_id(), opened.case.id());
        assert_eq!(step.position() as usize, index + 1);
        assert_eq!(step.title(), planned.title);
        assert_eq!(
            step.due_on(),
            SIGNED_ON + Duration::days(planned.offset_days)
        );
        assert_eq!(step.done_on(), None);
    }
}

#[test]
fn refuses_to_open_a_case_before_the_contract_is_signed() {
    let unsigned = lead("REINFO", 2);
    let template = procedure_template(unsigned.service());

    assert_eq!(
        CaseFile::open(&unsigned, SIGNED_ON, &template),
        Err(DomainError::ContractNotSigned)
    );
}

#[test]
fn selects_a_template_from_the_service_text() {
    for (service, name) in [
        ("REINFO", "REINFO"),
        ("reinfo persona natural", "REINFO"),
        ("IGAFOM correctivo", "IGAFOM"),
        ("DAC", "DAC"),
        ("Declaración DAC 2026", "DAC"),
        ("ESTAMIN", "ESTAMIN"),
        ("Plan de minado", "Plan de minado"),
        ("PLAN DE MINADO anual", "Plan de minado"),
    ] {
        assert_eq!(procedure_template(service).name, name, "{service}");
    }
}

#[test]
fn falls_back_to_the_generic_template_without_matching_inside_words() {
    for service in ["Asesoría", "Redacción de contrato", ""] {
        assert_eq!(procedure_template(service).name, "General", "{service}");
    }
}

#[test]
fn every_template_offers_ordered_future_suggestions() {
    for service in ["REINFO", "IGAFOM", "DAC", "ESTAMIN", "Plan de minado", "x"] {
        let template = procedure_template(service);

        assert!(!template.steps.is_empty(), "{service}");
        assert!(template.steps.iter().all(|step| step.offset_days > 0));
        assert!(
            template
                .steps
                .windows(2)
                .all(|pair| pair[0].offset_days < pair[1].offset_days),
            "{service}"
        );
    }
}

#[test]
fn marks_a_step_done_and_undone() {
    let mut step = CaseStep::restore(step_record()).unwrap();

    step.mark_done(date!(2026 - 11 - 08));
    assert_eq!(step.done_on(), Some(date!(2026 - 11 - 08)));
    assert!(step.is_done());

    step.mark_undone();
    assert_eq!(step.done_on(), None);
    assert!(!step.is_done());
}

#[test]
fn keeps_the_first_completion_date_when_marked_done_twice() {
    let mut step = CaseStep::restore(step_record()).unwrap();

    step.mark_done(date!(2026 - 11 - 08));
    step.mark_done(date!(2026 - 11 - 09));

    assert_eq!(step.done_on(), Some(date!(2026 - 11 - 08)));
}

#[test]
fn changes_the_due_date_without_touching_completion() {
    let mut step = CaseStep::restore(step_record()).unwrap();
    step.mark_done(date!(2026 - 11 - 08));

    step.reschedule(date!(2026 - 12 - 01));

    assert_eq!(step.due_on(), date!(2026 - 12 - 01));
    assert_eq!(step.done_on(), Some(date!(2026 - 11 - 08)));
}

#[test]
fn restores_persisted_cases_and_steps() {
    let record = case_record("closed");
    let id = record.id;

    let restored = Case::restore(record).unwrap();

    assert_eq!(restored.id(), id);
    assert_eq!(restored.status(), CaseStatus::Closed);
}

#[test]
fn refuses_corrupt_case_and_step_records() {
    let unknown_status = Case::restore(case_record("archived"));
    let blank_client = Case::restore(CaseRecord {
        client_name: " ".to_owned(),
        ..case_record("open")
    });
    let invalid_ruc = Case::restore(CaseRecord {
        ruc: Some("123".to_owned()),
        ..case_record("open")
    });
    let blank_title = CaseStep::restore(CaseStepRecord {
        title: String::new(),
        ..step_record()
    });

    assert_eq!(unknown_status, Err(DomainError::CorruptRecord));
    assert_eq!(blank_client, Err(DomainError::CorruptRecord));
    assert_eq!(invalid_ruc, Err(DomainError::CorruptRecord));
    assert_eq!(blank_title, Err(DomainError::CorruptRecord));
}
