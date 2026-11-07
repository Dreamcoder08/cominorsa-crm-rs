//! Lead origin and contact consent, proven through the CLI alone.

mod common;

use std::{fs, path::Path};

use common::{NO_ID, Scratch, TODAY, add_lead, data, export, failure, refused, run, text};
use serde_json::{Value, json};

fn add_from(db: &Path, origin: &str, phone: &str) -> Value {
    data(&run(
        db,
        &[
            "lead",
            "add",
            "--name",
            "Minera Consentida",
            "--phone",
            phone,
            "--service",
            "DAC",
            "--origin",
            origin,
        ],
    ))["lead"]
        .clone()
}

fn inbound_consent() -> Value {
    json!({
        "given": true,
        "recorded_on": TODAY,
        "source": "El cliente inició el contacto",
    })
}

#[test]
fn an_inbound_lead_records_consent_dated_today() {
    let scratch = Scratch::new();
    let db = scratch.database();

    for origin in ["web_form", "inbound_whatsapp", "inbound_call"] {
        let lead = add_from(&db, origin, "987654321");

        assert_eq!(lead["origin"], origin);
        assert_eq!(lead["consent"], inbound_consent());
    }
}

#[test]
fn any_other_origin_and_a_missing_one_start_without_consent() {
    let scratch = Scratch::new();
    let db = scratch.database();

    for origin in ["referral", "event", "existing_client", "prospect", "other"] {
        let lead = add_from(&db, origin, "987654321");

        assert_eq!(lead["origin"], origin);
        assert_eq!(lead["consent"], Value::Null);
    }
    let plain = add_lead(&db, "Sin Origen", "987654321", "DAC");
    let shown = data(&run(&db, &["lead", "show", &plain]));
    assert_eq!(shown["lead"]["origin"], "other");
    assert_eq!(shown["lead"]["consent"], Value::Null);
}

#[test]
fn an_unknown_origin_is_refused() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let before = export(&db);

    let output = run(
        &db,
        &[
            "lead",
            "add",
            "--name",
            "X",
            "--phone",
            "987654321",
            "--service",
            "DAC",
            "--origin",
            "email",
        ],
    );

    refused(&db, output, 1, "invalid_origin", &before);
}

#[test]
fn whatsapp_is_refused_without_consent_and_changes_nothing() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Sin Consentimiento", "987654321", "DAC");
    let before = export(&db);
    let bytes = fs::read(&db).unwrap();

    let output = run(&db, &["lead", "whatsapp", &lead]);

    let (code, message) = failure(&output, 1);
    assert_eq!(code, "no_consent");
    assert!(message.contains("consentimiento"), "{message}");
    assert_eq!(export(&db), before);
    assert_eq!(fs::read(&db).unwrap(), bytes);
}

#[test]
fn recorded_consent_opens_the_chat_and_revoking_closes_it() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Referida", "987654321", "DAC");

    let recorded = data(&run(
        &db,
        &["lead", "consent", &lead, "--source", "Aceptó por llamada"],
    ));
    assert_eq!(
        recorded["lead"]["consent"],
        json!({"given": true, "recorded_on": TODAY, "source": "Aceptó por llamada"})
    );
    let url = data(&run(&db, &["lead", "whatsapp", &lead]));
    assert!(text(&url["url"]).starts_with("https://wa.me/51987654321?text="));

    let revoked = data(&run(
        &db,
        &[
            "lead",
            "revoke-consent",
            &lead,
            "--source",
            "Pidió no recibir mensajes",
        ],
    ));
    assert_eq!(
        revoked["lead"]["consent"],
        json!({"given": false, "recorded_on": TODAY, "source": "Pidió no recibir mensajes"})
    );
    assert_eq!(
        failure(&run(&db, &["lead", "whatsapp", &lead]), 1).0,
        "no_consent"
    );
    let exported = export(&db);
    assert_eq!(exported["leads"][0]["consent"], revoked["lead"]["consent"]);
    assert_eq!(exported["leads"][0]["origin"], "other");
}

#[test]
fn consent_changes_refuse_a_blank_source_a_missing_lead_and_a_revocation_without_consent() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_lead(&db, "Referida", "987654321", "DAC");
    let before = export(&db);

    for (args, code) in [
        (
            vec!["lead", "consent", &lead, "--source", "  "],
            "missing_field",
        ),
        (
            vec!["lead", "consent", NO_ID, "--source", "x"],
            "lead_not_found",
        ),
        (
            vec!["lead", "revoke-consent", &lead, "--source", "x"],
            "no_consent",
        ),
    ] {
        refused(&db, run(&db, &args), 1, code, &before);
    }
}

#[test]
fn a_renewal_inherits_the_consent_of_the_lead_that_opened_the_case() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_from(&db, "web_form", "987654321");
    let id = text(&lead["id"]).to_owned();
    data(&run(&db, &["lead", "advance", &id]));
    data(&run(&db, &["lead", "advance", &id]));
    let case = data(&run(&db, &["lead", "advance", &id]))["case"]["id"].clone();

    let closed = data(&run(&db, &["case", "close", text(&case)]));

    assert_eq!(closed["renewal"]["origin"], "existing_client");
    assert_eq!(closed["renewal"]["consent"], inbound_consent());
}

#[test]
fn a_phone_without_a_chat_number_is_still_refused_with_consent() {
    let scratch = Scratch::new();
    let db = scratch.database();
    let lead = add_from(&db, "inbound_call", "12");

    let (code, _) = failure(&run(&db, &["lead", "whatsapp", text(&lead["id"])]), 1);

    assert_eq!(code, "no_whatsapp_number");
}
