use cominorsa_domain::{DomainError, Lead, NewLead, Settings, whatsapp_number};
use time::macros::date;

fn lead(phone: &str) -> Lead {
    Lead::create(NewLead {
        name: "Minera Peña".to_owned(),
        phone: phone.to_owned(),
        ruc: None,
        service: "REINFO & DAC".to_owned(),
        follow_up_on: date!(2026 - 11 - 10),
    })
    .unwrap()
}

#[test]
fn normalizes_peruvian_and_international_numbers() {
    assert_eq!(
        whatsapp_number("987 654 321").as_deref(),
        Some("51987654321")
    );
    assert_eq!(
        whatsapp_number("51987654321").as_deref(),
        Some("51987654321")
    );
    assert_eq!(
        whatsapp_number("+51 987-654-321").as_deref(),
        Some("51987654321")
    );
    assert_eq!(
        whatsapp_number("+34 600 111 222").as_deref(),
        Some("34600111222")
    );
}

#[test]
fn rejects_numbers_that_cannot_receive_whatsapp() {
    for invalid in [
        "",
        "abc",
        "073123456",
        "12345",
        "+1234567",
        "+1234567890123456",
    ] {
        assert_eq!(whatsapp_number(invalid), None, "{invalid} must be rejected");
    }
}

#[test]
fn builds_a_chat_url_with_the_encoded_message() {
    let url = lead("987654321")
        .whatsapp_url("Hola {nombre}, sobre {servicio}?")
        .unwrap();

    assert_eq!(
        url,
        "https://wa.me/51987654321?text=Hola%20Minera%20Pe%C3%B1a%2C%20sobre%20REINFO%20%26%20DAC%3F"
    );
}

#[test]
fn offers_no_url_for_an_invalid_phone() {
    assert_eq!(lead("073123456").whatsapp_url("Hola"), None);
}

#[test]
fn keeps_a_non_blank_trimmed_template() {
    let mut settings = Settings::default();
    assert!(settings.whatsapp_template().contains("{nombre}"));

    settings.set_whatsapp_template("  Hola {nombre}  ").unwrap();
    assert_eq!(settings.whatsapp_template(), "Hola {nombre}");

    assert_eq!(
        settings.set_whatsapp_template("   "),
        Err(DomainError::MissingField("mensaje"))
    );
    assert_eq!(settings.whatsapp_template(), "Hola {nombre}");
}
