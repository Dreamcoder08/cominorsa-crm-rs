use cominorsa_domain::{DomainError, Lead, NewLead, Ruc, Stage};
use time::macros::date;

fn new_lead(ruc: Option<&str>) -> NewLead {
    NewLead {
        name: "  Minera Ejemplo  ".to_owned(),
        phone: "900000000".to_owned(),
        ruc: ruc.map(str::to_owned),
        service: "REINFO".to_owned(),
        follow_up_on: date!(2026 - 11 - 10),
    }
}

#[test]
fn creates_lead_in_initial_stage_with_trimmed_name() {
    let lead = Lead::create(new_lead(Some("20123456786"))).unwrap();

    assert_eq!(lead.name(), "Minera Ejemplo");
    assert_eq!(lead.stage(), Stage::InitialContact);
    assert_eq!(lead.ruc().map(Ruc::as_str), Some("20123456786"));
}

#[test]
fn treats_blank_ruc_as_absent() {
    let lead = Lead::create(new_lead(Some("   "))).unwrap();

    assert_eq!(lead.ruc(), None);
}

#[test]
fn rejects_blank_required_fields() {
    let mut input = new_lead(None);
    input.name = "   ".to_owned();
    assert_eq!(Lead::create(input), Err(DomainError::MissingField("nombre")));

    let mut input = new_lead(None);
    input.phone = String::new();
    assert_eq!(Lead::create(input), Err(DomainError::MissingField("teléfono")));

    let mut input = new_lead(None);
    input.service = String::new();
    assert_eq!(Lead::create(input), Err(DomainError::MissingField("servicio")));
}

#[test]
fn rejects_invalid_rucs() {
    for invalid in ["2012345678", "201234567861", "20123456787", "30123456781", "2012345678A"] {
        assert_eq!(
            Ruc::parse(invalid),
            Err(DomainError::InvalidRuc),
            "{invalid} must be rejected"
        );
    }
}

#[test]
fn accepts_check_digit_remainders_that_wrap() {
    // Weighted sum 89: remainder 1 maps to check digit 0.
    assert!(Ruc::parse("20100070970").is_ok());
}

#[test]
fn advances_through_every_stage_and_stops_at_contract() {
    let mut lead = Lead::create(new_lead(None)).unwrap();

    lead.advance().unwrap();
    assert_eq!(lead.stage(), Stage::CoordinateEvaluation);
    lead.advance().unwrap();
    assert_eq!(lead.stage(), Stage::QuoteSent);
    lead.advance().unwrap();
    assert_eq!(lead.stage(), Stage::ContractSigned);

    assert_eq!(lead.advance(), Err(DomainError::FinalStage));
    assert_eq!(lead.stage(), Stage::ContractSigned);
}

#[test]
fn stage_codes_round_trip() {
    for stage in Stage::ALL {
        assert_eq!(Stage::from_code(stage.code()), Some(stage));
    }
    assert_eq!(Stage::from_code("unknown"), None);
}

#[test]
fn flags_follow_up_due_today_or_earlier_unless_contract_is_signed() {
    let mut lead = Lead::create(new_lead(None)).unwrap();

    assert!(!lead.is_follow_up_due(date!(2026 - 11 - 09)));
    assert!(lead.is_follow_up_due(date!(2026 - 11 - 10)));
    assert!(lead.is_follow_up_due(date!(2026 - 11 - 11)));

    lead.advance().unwrap();
    lead.advance().unwrap();
    lead.advance().unwrap();
    assert!(!lead.is_follow_up_due(date!(2026 - 11 - 11)));
}
