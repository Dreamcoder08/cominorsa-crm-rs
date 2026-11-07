use cominorsa_domain::{
    CaseClosure, CaseFile, Consent, DomainError, Lead, LeadOrigin, LeadRecord, NewLead,
    procedure_template,
};
use time::{Date, macros::date};
use uuid::Uuid;

const TODAY: Date = date!(2026 - 11 - 07);
const INBOUND_SOURCE: &str = "El cliente inició el contacto";

fn new_lead(phone: &str) -> NewLead {
    NewLead {
        name: "Minera Ejemplo".to_owned(),
        phone: phone.to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        owner: None,
        follow_up_on: date!(2026 - 11 - 10),
    }
}

fn received(origin: LeadOrigin) -> Lead {
    Lead::receive(new_lead("987654321"), origin, TODAY).unwrap()
}

fn given(on: Date, source: &str) -> Option<Consent> {
    Some(Consent {
        given: true,
        recorded_on: on,
        source: source.to_owned(),
    })
}

#[test]
fn a_lead_the_client_started_records_consent_dated_today() {
    for origin in [
        LeadOrigin::WebForm,
        LeadOrigin::InboundWhatsapp,
        LeadOrigin::InboundCall,
    ] {
        let lead = received(origin);

        assert_eq!(lead.origin(), origin);
        assert_eq!(lead.consent(), given(TODAY, INBOUND_SOURCE).as_ref());
        assert!(lead.has_consent(), "{origin:?}");
    }
}

#[test]
fn a_lead_from_any_other_origin_starts_without_consent() {
    for origin in [
        LeadOrigin::Referral,
        LeadOrigin::Event,
        LeadOrigin::ExistingClient,
        LeadOrigin::Prospect,
        LeadOrigin::Other,
    ] {
        let lead = received(origin);

        assert_eq!(lead.origin(), origin);
        assert_eq!(lead.consent(), None);
        assert!(!lead.has_consent(), "{origin:?}");
    }
}

#[test]
fn a_plain_lead_has_origin_other_and_no_consent() {
    let lead = Lead::create(new_lead("987654321")).unwrap();

    assert_eq!(lead.origin(), LeadOrigin::Other);
    assert_eq!(lead.consent(), None);
}

#[test]
fn origins_keep_stable_codes() {
    let codes: Vec<&str> = LeadOrigin::ALL.iter().map(|origin| origin.code()).collect();

    assert_eq!(
        codes,
        [
            "web_form",
            "inbound_whatsapp",
            "inbound_call",
            "referral",
            "event",
            "existing_client",
            "prospect",
            "other"
        ]
    );
    for origin in LeadOrigin::ALL {
        assert_eq!(LeadOrigin::from_code(origin.code()), Some(origin));
        assert!(!origin.label().is_empty());
    }
    assert_eq!(LeadOrigin::from_code("email"), None);
}

#[test]
fn recording_consent_needs_a_source_and_keeps_its_date() {
    let mut lead = received(LeadOrigin::Referral);

    assert_eq!(
        lead.record_consent(TODAY, "   "),
        Err(DomainError::MissingField("fuente"))
    );
    assert_eq!(lead.consent(), None);

    lead.record_consent(date!(2026 - 11 - 08), "  Aceptó por llamada  ")
        .unwrap();

    assert_eq!(
        lead.consent(),
        given(date!(2026 - 11 - 08), "Aceptó por llamada").as_ref()
    );
}

#[test]
fn revoking_consent_keeps_the_date_and_source_of_the_revocation() {
    let mut lead = received(LeadOrigin::WebForm);

    lead.revoke_consent(date!(2026 - 11 - 09), "Pidió no recibir mensajes")
        .unwrap();

    assert!(!lead.has_consent());
    assert_eq!(
        lead.consent(),
        Some(&Consent {
            given: false,
            recorded_on: date!(2026 - 11 - 09),
            source: "Pidió no recibir mensajes".to_owned(),
        })
    );
}

#[test]
fn only_current_consent_can_be_revoked() {
    let mut lead = received(LeadOrigin::Prospect);

    assert_eq!(
        lead.revoke_consent(TODAY, "Pidió no recibir mensajes"),
        Err(DomainError::NoConsent)
    );
    assert_eq!(lead.consent(), None);

    let mut consenting = received(LeadOrigin::WebForm);
    assert_eq!(
        consenting.revoke_consent(TODAY, " "),
        Err(DomainError::MissingField("fuente"))
    );
    assert!(consenting.has_consent());
}

#[test]
fn a_whatsapp_link_needs_current_consent() {
    let without = received(LeadOrigin::Referral);
    assert_eq!(without.whatsapp_link("Hola"), Err(DomainError::NoConsent));
    assert_eq!(without.whatsapp_url("Hola"), None);

    let mut revoked = received(LeadOrigin::InboundCall);
    revoked
        .revoke_consent(TODAY, "Pidió no recibir mensajes")
        .unwrap();
    assert_eq!(revoked.whatsapp_link("Hola"), Err(DomainError::NoConsent));

    let consenting = received(LeadOrigin::InboundWhatsapp);
    assert_eq!(
        consenting.whatsapp_link("Hola").unwrap(),
        "https://wa.me/51987654321?text=Hola"
    );
}

#[test]
fn consent_does_not_make_an_invalid_phone_usable() {
    let lead = Lead::receive(new_lead("073123456"), LeadOrigin::WebForm, TODAY).unwrap();

    assert_eq!(
        lead.whatsapp_link("Hola"),
        Err(DomainError::NoWhatsappNumber)
    );
}

#[test]
fn a_renewal_inherits_the_consent_of_its_source_lead() {
    for source_origin in [LeadOrigin::WebForm, LeadOrigin::Prospect] {
        let mut signed = received(source_origin);
        for _ in 0..3 {
            signed.advance().unwrap();
        }
        let file = CaseFile::open(&signed, TODAY, &procedure_template("DAC")).unwrap();

        let renewal = CaseClosure::of(file.case, &signed, date!(2026 - 12 - 15))
            .renewal
            .unwrap();

        assert_eq!(renewal.origin(), LeadOrigin::ExistingClient);
        assert_eq!(renewal.consent(), signed.consent());
    }
}

fn record(origin: &str, consent: Option<Consent>) -> LeadRecord {
    LeadRecord {
        id: Uuid::nil(),
        name: "Minera Ejemplo".to_owned(),
        phone: "987654321".to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        owner: None,
        stage: "initial_contact".to_owned(),
        follow_up_on: TODAY,
        renewal_of: None,
        origin: origin.to_owned(),
        consent,
    }
}

#[test]
fn restores_origin_and_consent_and_refuses_corrupt_values() {
    let restored = Lead::restore(record("event", given(TODAY, "Firmó en la feria"))).unwrap();
    assert_eq!(restored.origin(), LeadOrigin::Event);
    assert_eq!(
        restored.consent(),
        given(TODAY, "Firmó en la feria").as_ref()
    );

    assert_eq!(
        Lead::restore(record("email", None)),
        Err(DomainError::CorruptRecord)
    );
    assert_eq!(
        Lead::restore(record("other", given(TODAY, " "))),
        Err(DomainError::CorruptRecord)
    );
}
