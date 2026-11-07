use cominorsa_domain::{
    Case, CaseClosure, CaseFile, CaseRecord, CaseStatus, CaseStep, CaseStepRecord, DomainError,
    Lead, LeadRecord, Money, NewLead, Ruc, Stage, procedure_template, renewal_date,
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
        contract_total_cents: None,
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
fn combines_every_named_service_in_the_order_of_the_text() {
    let template = procedure_template("IGAFOM + REINFO");
    let igafom = procedure_template("IGAFOM");
    let reinfo = procedure_template("REINFO");

    assert_eq!(template.name, "IGAFOM + REINFO");
    assert_eq!(
        template.steps.len(),
        igafom.steps.len() + reinfo.steps.len()
    );
    assert_eq!(
        procedure_template("reinfo y plan de minado").name,
        "REINFO + Plan de minado"
    );
}

#[test]
fn keeps_each_filing_when_combining_procedures() {
    for service in ["DAC + ESTAMIN", "ESTAMIN + DAC"] {
        let labels = procedure_template(service).step_titles();

        assert!(labels.contains(&"DAC: Presentar la declaración".to_owned()));
        assert!(labels.contains(&"ESTAMIN: Presentar la declaración".to_owned()));
    }
}

#[test]
fn orders_combined_steps_by_their_suggested_date() {
    let offsets: Vec<i64> = procedure_template("ESTAMIN + DAC")
        .steps
        .iter()
        .map(|step| step.offset_days)
        .collect();
    let mut sorted = offsets.clone();
    sorted.sort_unstable();

    assert_eq!(offsets, sorted);
}

#[test]
fn keeps_plain_titles_for_a_single_procedure() {
    let template = procedure_template("DAC");

    assert!(
        template
            .step_titles()
            .contains(&"Presentar la declaración".to_owned())
    );
}

#[test]
fn combining_keeps_whole_word_matching() {
    assert_eq!(procedure_template("Redacción + REINFO").name, "REINFO");
    assert_eq!(procedure_template("plan de trabajo").name, "General");
}

#[test]
fn a_combined_case_opens_one_step_per_combined_entry() {
    let signed = lead("IGAFOM + REINFO", 3);
    let template = procedure_template(signed.service());

    let opened = CaseFile::open(&signed, SIGNED_ON, &template).unwrap();

    assert_eq!(opened.steps.len(), template.steps.len());
    assert!(
        opened.steps[0].title().starts_with("IGAFOM: ")
            || opened.steps[0].title().starts_with("REINFO: ")
    );
    assert_eq!(opened.steps[0].position(), 1);
    assert!(
        opened
            .steps
            .windows(2)
            .all(|pair| pair[0].due_on() <= pair[1].due_on())
    );
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

#[test]
fn only_dac_suggests_a_renewal_interval() {
    assert_eq!(procedure_template("DAC").renewal_months, Some(12));
    for service in ["REINFO", "IGAFOM", "ESTAMIN", "Plan de minado", "Asesoría"] {
        assert_eq!(
            procedure_template(service).renewal_months,
            None,
            "{service}"
        );
    }
}

#[test]
fn a_service_recurs_when_any_of_its_named_procedures_does() {
    assert_eq!(procedure_template("IGAFOM + DAC").renewal_months, Some(12));
    assert_eq!(procedure_template("IGAFOM + REINFO").renewal_months, None);
}

#[test]
fn suggests_a_renewal_date_interval_months_after_closing() {
    assert_eq!(
        renewal_date("DAC", date!(2026 - 11 - 07)),
        Some(date!(2027 - 11 - 07))
    );
    assert_eq!(
        renewal_date("DAC", date!(2028 - 02 - 29)),
        Some(date!(2029 - 02 - 28))
    );
    assert_eq!(renewal_date("REINFO", date!(2026 - 11 - 07)), None);
}

fn restored_lead(name: &str, renewal_of: Option<Uuid>) -> Lead {
    Lead::restore(LeadRecord {
        id: Uuid::now_v7(),
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        owner: None,
        stage: "initial_contact".to_owned(),
        follow_up_on: SIGNED_ON,
        renewal_of,
    })
    .unwrap()
}

#[test]
fn closing_a_recurring_case_builds_a_renewal_lead_from_the_case_and_its_lead() {
    let signed = lead("DAC", 3);
    let file = CaseFile::open(&signed, SIGNED_ON, &procedure_template("DAC")).unwrap();
    let closed_on = date!(2026 - 12 - 15);

    let closure = CaseClosure::of(file.case.clone(), &signed, closed_on);

    assert_eq!(closure.case.status(), CaseStatus::Closed);
    assert_eq!(closure.case.id(), file.case.id());
    let renewal = closure.renewal.unwrap();
    assert_ne!(renewal.id(), signed.id());
    assert_eq!(renewal.name(), "Renovación: Minera Ejemplo");
    assert_eq!(renewal.client_name(), "Minera Ejemplo");
    assert_eq!(renewal.phone(), "900000000");
    assert_eq!(renewal.ruc().map(Ruc::as_str), Some("20123456786"));
    assert_eq!(renewal.owner(), Some("Ana"));
    assert_eq!(renewal.service(), "DAC");
    assert_eq!(renewal.stage(), Stage::InitialContact);
    assert_eq!(renewal.follow_up_on(), date!(2027 - 12 - 15));
    assert_eq!(renewal.renewal_of(), Some(file.case.id()));
}

#[test]
fn closing_a_case_that_does_not_recur_builds_no_lead() {
    let signed = lead("REINFO", 3);
    let file = CaseFile::open(&signed, SIGNED_ON, &procedure_template("REINFO")).unwrap();

    let closure = CaseClosure::of(file.case, &signed, SIGNED_ON);

    assert_eq!(closure.case.status(), CaseStatus::Closed);
    assert_eq!(closure.renewal, None);
}

#[test]
fn a_second_renewal_does_not_stack_the_label() {
    let mut renewal = restored_lead("Renovación: Minera Ejemplo", Some(Uuid::now_v7()));
    renewal.advance().unwrap();
    renewal.advance().unwrap();
    renewal.advance().unwrap();

    let file = CaseFile::open(&renewal, SIGNED_ON, &procedure_template("DAC")).unwrap();
    assert_eq!(file.case.client_name(), "Minera Ejemplo");

    let next = CaseClosure::of(file.case, &renewal, SIGNED_ON).renewal;
    assert_eq!(next.unwrap().name(), "Renovación: Minera Ejemplo");
}

#[test]
fn a_prepared_message_greets_the_client_not_the_renewal_label() {
    let renewal = restored_lead("Renovación: Minera Ejemplo", Some(Uuid::now_v7()));

    let url = renewal.whatsapp_url("Hola {nombre}").unwrap();

    assert!(url.ends_with("Hola%20Minera%20Ejemplo"), "{url}");
}

#[test]
fn only_a_renewal_loses_the_label_prefix() {
    let plain = restored_lead("Renovación: Calle 1 SAC", None);

    assert_eq!(plain.client_name(), "Renovación: Calle 1 SAC");
    assert_eq!(plain.renewal_of(), None);
}

#[test]
fn a_contract_total_must_be_positive_and_survives_restore() {
    let mut case = Case::restore(CaseRecord {
        contract_total_cents: Some(123_450),
        ..case_record("open")
    })
    .unwrap();

    assert_eq!(case.contract_total().map(Money::cents), Some(123_450));
    assert_eq!(
        case.set_contract_total(Money::default()),
        Err(DomainError::InvalidAmount)
    );
    case.set_contract_total(Money::from_cents(500).unwrap())
        .unwrap();
    assert_eq!(case.contract_total().map(Money::cents), Some(500));
    for cents in [0, -1] {
        let corrupt = Case::restore(CaseRecord {
            contract_total_cents: Some(cents),
            ..case_record("open")
        });
        assert_eq!(corrupt, Err(DomainError::CorruptRecord));
    }
}
