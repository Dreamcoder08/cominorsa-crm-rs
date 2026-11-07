use std::{env, fs, path::PathBuf, process::Command};

use cominorsa_desktop::{
    CaseDetail, QUOTE_TAX_NOTE, QuoteChange, QuoteEdit, QuoteLineValues, QuotePanel, QuoteValues,
    SaveState, SettingsView, Workspace, accepted_notice, company_lines, company_warning,
    format_igv_rate, igv_input, lead_quotes, milestone_gap_warning, quote_change, quote_file_name,
    quote_notice, quote_pdf, quote_preview, quote_status_class, save_advance, save_quote,
    suggested_total_note, write_pdf,
};
use cominorsa_domain::{
    BillingMilestone, Case, CaseFile, CaseRecord, CaseRepository, CompanyProfile, IgvRate, Lead,
    LeadRepository, Money, NewLead, NewMilestone, Quote, QuoteContent, QuoteDraft, QuoteLine,
    QuoteNumber, QuoteRepository, QuoteStatus, Settings, procedure_template,
};
use cominorsa_storage::SqliteLeads;
use dioxus::prelude::*;
use time::{Date, macros::date};
use uuid::Uuid;

const TODAY: Date = date!(2026 - 11 - 07);

fn soles(cents: i64) -> Money {
    Money::from_cents(cents).unwrap()
}

fn lead_named(name: &str, advances: usize) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: "DAC".to_owned(),
        owner: None,
        follow_up_on: TODAY,
    })
    .unwrap();
    for _ in 0..advances {
        lead.advance().unwrap();
    }
    lead
}

fn lead() -> Lead {
    lead_named("Minera Peña Blanca", 1)
}

fn company() -> CompanyProfile {
    CompanyProfile {
        name: "Empresa Ejemplo SAC".to_owned(),
        ruc: "20123456786".to_owned(),
        address: "Av. Ejemplo 123, Piura".to_owned(),
        phone: "900000001".to_owned(),
        email: "ventas@example.com".to_owned(),
    }
}

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.set_company(company()).unwrap();
    settings
}

fn content(lines: Vec<QuoteLine>) -> QuoteContent {
    QuoteContent {
        issued_on: TODAY,
        valid_until: date!(2026 - 11 - 22),
        lines,
        notes: "¿Dudas? Escríbenos.".to_owned(),
    }
}

fn evaluation() -> QuoteLine {
    QuoteLine::new("Evaluación de coordenadas", 2, soles(150_000)).unwrap()
}

fn draft_of(lead: &Lead, lines: Vec<QuoteLine>) -> Quote {
    QuoteDraft::for_lead(lead, content(lines), IgvRate::default())
        .unwrap()
        .numbered(QuoteNumber::new(2026, 1, 1).unwrap())
}

fn draft_quote() -> Quote {
    draft_of(&lead(), vec![evaluation()])
}

fn sent_quote() -> Quote {
    let mut quote = draft_quote();
    quote.mark_sent(&company()).unwrap();
    quote
}

fn accepted_quote() -> Quote {
    let mut quote = sent_quote();
    quote.mark_accepted().unwrap();
    quote
}

fn version_of(quote: &Quote) -> Quote {
    let draft = quote.new_version(TODAY, IgvRate::default()).unwrap();
    let number = draft.next_version().unwrap();
    draft.numbered(number)
}

fn filled_values() -> QuoteValues {
    QuoteValues {
        lines: vec![QuoteLineValues {
            description: "Evaluación".to_owned(),
            quantity: "2".to_owned(),
            unit_price: "1,000.00".to_owned(),
        }],
        notes: "Nota".to_owned(),
        ..QuoteValues::starting_on(TODAY)
    }
}

fn render(component: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(component);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = env::temp_dir().join(format!("cominorsa-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn database(&self) -> PathBuf {
        self.0.join("crm.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pdftotext_exists() -> bool {
    Command::new("pdftotext").arg("-v").output().is_ok()
}

/// Text of a PDF through poppler, or `None` when it is not installed; then the
/// tests fall back to `assert_valid_pdf`, which every PDF test also runs.
fn extracted_text(pdf: &[u8], flags: &[&str]) -> Option<String> {
    if !pdftotext_exists() {
        eprintln!("SKIPPED text extraction: pdftotext is not installed");
        return None;
    }

    let scratch = Scratch::new();
    let path = scratch.0.join("quote.pdf");
    fs::write(&path, pdf).unwrap();
    let output = Command::new("pdftotext")
        .args(["-enc", "UTF-8"])
        .args(flags)
        .arg(&path)
        .arg("-")
        .output()
        .unwrap();

    // Older builds, such as the one on Windows runners, lack layout flags.
    if !output.status.success() && !flags.is_empty() {
        eprintln!("SKIPPED layout extraction: pdftotext does not support {flags:?}");
        return None;
    }
    assert!(output.status.success(), "pdftotext refused the file");

    Some(
        String::from_utf8(output.stdout)
            .unwrap()
            .replace("\r\n", "\n"),
    )
}

fn assert_valid_pdf(pdf: &[u8]) {
    assert!(pdf.starts_with(b"%PDF-"));
    let tail = String::from_utf8_lossy(&pdf[pdf.len() - 32..]);
    assert!(tail.contains("%%EOF"), "{tail}");
    let body = String::from_utf8_lossy(pdf);
    assert!(body.contains("/FontFile"), "the fonts must be embedded");
    assert!(
        body.contains("/ToUnicode"),
        "the text must stay extractable"
    );
}

fn pdf_text(quote: &Quote, company: &CompanyProfile) -> Option<String> {
    let pdf = quote_pdf(quote, company).unwrap();
    assert_valid_pdf(&pdf);
    extracted_text(&pdf, &[])
}

#[test]
fn formats_the_igv_rate_without_trailing_zeros() {
    let rate = |text| IgvRate::parse(text).unwrap();

    assert_eq!(format_igv_rate(rate("18")), "18%");
    assert_eq!(format_igv_rate(rate("10.5")), "10.5%");
    assert_eq!(format_igv_rate(rate("10.25")), "10.25%");
    assert_eq!(format_igv_rate(rate("0")), "0%");
}

#[test]
fn the_igv_input_round_trips_through_the_parser() {
    for text in ["18", "10.5", "10.25", "0", "100"] {
        let rate = IgvRate::parse(text).unwrap();

        assert_eq!(igv_input(rate), text);
    }
}

#[test]
fn the_file_name_is_the_quote_number() {
    assert_eq!(quote_file_name(&draft_quote()), "COT-2026-0001.pdf");
    assert_eq!(
        quote_file_name(&version_of(&sent_quote())),
        "COT-2026-0001-V2.pdf"
    );
}

#[test]
fn company_lines_leave_out_blank_fields() {
    assert_eq!(
        company_lines(&company()),
        [
            "RUC 20123456786",
            "Av. Ejemplo 123, Piura",
            "Tel. 900000001",
            "ventas@example.com"
        ]
    );
    assert_eq!(
        company_lines(&CompanyProfile::default()),
        Vec::<String>::new()
    );
    assert_eq!(
        company_lines(&CompanyProfile {
            phone: "  ".to_owned(),
            ..CompanyProfile::default()
        }),
        Vec::<String>::new()
    );
}

#[test]
fn warns_only_when_the_company_name_is_missing() {
    assert_eq!(company_warning(&company()), None);
    assert_eq!(
        company_warning(&CompanyProfile::default()),
        Some("Falta el nombre de la empresa. Agrégalo en Configuración antes de emitir.")
    );
}

#[test]
fn each_status_has_its_badge_class() {
    let classes = QuoteStatus::ALL.map(quote_status_class);

    assert_eq!(classes, ["upcoming", "today", "closed", "overdue"]);
}

fn planned_case(total: i64, milestones: &[i64]) -> CaseFile {
    let source = lead_named("Minera Peña Blanca", 3);
    let mut file = CaseFile::open(&source, TODAY, &procedure_template("DAC")).unwrap();
    file.case.set_contract_total(soles(total)).unwrap();
    file.milestones = (1..)
        .zip(milestones)
        .map(|(position, cents)| {
            BillingMilestone::create(NewMilestone {
                case_id: file.case.id(),
                position,
                title: format!("Hito {position}"),
                amount: soles(*cents),
                due_on: TODAY,
            })
            .unwrap()
        })
        .collect();
    file
}

#[test]
fn warns_when_milestones_do_not_sum_to_the_contract_total() {
    assert_eq!(
        milestone_gap_warning(&planned_case(100_000, &[60_000])).as_deref(),
        Some("Los hitos suman S/ 600.00 y el contrato S/ 1,000.00: faltan S/ 400.00.")
    );
    assert_eq!(
        milestone_gap_warning(&planned_case(100_000, &[60_000, 50_000])).as_deref(),
        Some("Los hitos suman S/ 1,100.00 y el contrato S/ 1,000.00: sobran S/ 100.00.")
    );
    assert_eq!(
        milestone_gap_warning(&planned_case(100_000, &[100_000])),
        None
    );
}

#[test]
fn names_the_quote_that_suggested_the_contract_total() {
    let quote = accepted_quote();
    let matching = {
        let mut file = planned_case(1, &[]);
        file.case.set_contract_total(quote.totals().total).unwrap();
        file
    };
    let edited = planned_case(5_000, &[]);
    let quotes = [quote.clone()];
    let for_lead = |mut file: CaseFile| {
        let source = quote.lead_id();
        file.case = Case::restore(CaseRecord {
            id: file.case.id(),
            lead_id: source,
            client_name: file.case.client_name().to_owned(),
            ruc: None,
            service: file.case.service().to_owned(),
            owner: None,
            opened_on: TODAY,
            status: "open".to_owned(),
            contract_total_cents: file.case.contract_total().map(Money::cents),
        })
        .unwrap();
        file
    };

    assert_eq!(
        suggested_total_note(&for_lead(matching), &quotes).as_deref(),
        Some("Sugerido por COT-2026-0001; ajústalo si el contrato difiere.")
    );
    assert_eq!(suggested_total_note(&for_lead(edited), &quotes), None);
    assert_eq!(suggested_total_note(&planned_case(1, &[]), &quotes), None);
}

#[test]
fn acceptance_says_when_the_total_will_be_suggested() {
    let quote = accepted_quote();

    assert_eq!(
        accepted_notice(&quote, false),
        "COT-2026-0001 aceptada. Su total se sugerirá al abrir el expediente."
    );
    assert_eq!(
        accepted_notice(&quote, true),
        "COT-2026-0001 aceptada. El expediente ya está abierto: revisa su total en Cobros."
    );
}

#[test]
fn a_new_form_has_one_line_and_a_fifteen_day_validity() {
    let values = QuoteValues::starting_on(TODAY);

    assert_eq!(values.editing, None);
    assert_eq!(values.issued_on, "2026-11-07");
    assert_eq!(values.valid_until, "2026-11-22");
    assert_eq!(values.notes, "");
    assert_eq!(
        values.lines,
        [QuoteLineValues {
            description: String::new(),
            quantity: "1".to_owned(),
            unit_price: String::new(),
        }]
    );
}

#[test]
fn the_form_loads_a_draft_to_edit_it() {
    let quote = draft_quote();

    let values = QuoteValues::of_quote(&quote);

    assert_eq!(values.editing, Some(quote.id()));
    assert_eq!(values.issued_on, "2026-11-07");
    assert_eq!(values.valid_until, "2026-11-22");
    assert_eq!(values.notes, "¿Dudas? Escríbenos.");
    assert_eq!(
        values.lines,
        [QuoteLineValues {
            description: "Evaluación de coordenadas".to_owned(),
            quantity: "2".to_owned(),
            unit_price: "1500.00".to_owned(),
        }]
    );
}

#[test]
fn the_form_adds_and_removes_lines_but_keeps_one() {
    let mut values = QuoteValues::starting_on(TODAY);

    values.add_line();
    assert_eq!(values.lines.len(), 2);
    values.remove_line(0);
    assert_eq!(values.lines.len(), 1);
    values.remove_line(0);
    assert_eq!(values.lines.len(), 1);
}

#[test]
fn clearing_an_edit_returns_to_a_new_quote_and_keeps_the_dates() {
    let mut values = QuoteValues::of_quote(&draft_quote());

    values.clear();

    assert_eq!(values.editing, None);
    assert_eq!(values.issued_on, "2026-11-07");
    assert_eq!(values.valid_until, "2026-11-22");
    assert_eq!(values.notes, "");
    assert_eq!(values.lines, QuoteValues::starting_on(TODAY).lines);
}

#[test]
fn lists_only_the_quotes_of_the_lead_newest_first() {
    let lead = lead();
    let number = |sequence| QuoteNumber::new(2026, sequence, 1).unwrap();
    let draft = || QuoteDraft::for_lead(&lead, content(vec![evaluation()]), IgvRate::default());
    let older = draft().unwrap().numbered(number(1));
    let newer = draft().unwrap().numbered(number(2));
    let other = draft_quote();

    assert_eq!(
        lead_quotes(&[older.clone(), other, newer.clone()], lead.id()),
        vec![newer, older]
    );
}

#[test]
fn each_saved_change_reports_what_happened() {
    let quote = sent_quote();
    let notice = |edit: QuoteEdit| quote_notice(&edit, &quote, false);

    assert_eq!(
        notice(QuoteEdit::Save),
        "COT-2026-0001 guardada como borrador."
    );
    assert_eq!(
        notice(QuoteEdit::Send(quote.clone())),
        "COT-2026-0001 marcada como enviada."
    );
    assert_eq!(
        notice(QuoteEdit::Accept(quote.clone())),
        "COT-2026-0001 aceptada. Su total se sugerirá al abrir el expediente."
    );
    assert_eq!(
        notice(QuoteEdit::Reject(quote.clone())),
        "COT-2026-0001 marcada como rechazada."
    );
    assert_eq!(
        notice(QuoteEdit::NewVersion(quote.clone())),
        "COT-2026-0001 lista para editar."
    );
}

#[test]
fn the_preview_uses_the_domain_totals() {
    let totals = quote_preview(&filled_values(), IgvRate::default()).unwrap();

    assert_eq!(totals.subtotal, soles(200_000));
    assert_eq!(totals.igv, soles(36_000));
    assert_eq!(totals.total, soles(236_000));
}

#[test]
fn the_preview_waits_for_valid_lines() {
    assert_eq!(
        quote_preview(&QuoteValues::starting_on(TODAY), IgvRate::default()),
        None
    );
}

#[test]
fn saving_the_form_builds_a_draft_for_the_lead() {
    let lead = lead();

    let change = quote_change(
        &lead,
        QuoteEdit::Save,
        &filled_values(),
        &[],
        &settings(),
        TODAY,
    )
    .unwrap();

    let QuoteChange::Create(draft) = change else {
        panic!("expected a new quote");
    };
    assert_eq!(draft.year(), 2026);
    let quote = draft.numbered(QuoteNumber::new(2026, 1, 1).unwrap());
    assert_eq!(quote.lead_id(), lead.id());
    assert_eq!(quote.notes(), "Nota");
    assert_eq!(quote.totals().total, soles(236_000));
}

#[test]
fn saving_uses_the_igv_rate_of_the_settings() {
    let mut settings = settings();
    settings.set_igv_rate(IgvRate::parse("10").unwrap());

    let change = quote_change(
        &lead(),
        QuoteEdit::Save,
        &filled_values(),
        &[],
        &settings,
        TODAY,
    )
    .unwrap();

    let QuoteChange::Create(draft) = change else {
        panic!("expected a new quote");
    };
    let quote = draft.numbered(QuoteNumber::new(2026, 1, 1).unwrap());
    assert_eq!(quote.totals().igv, soles(20_000));
}

#[test]
fn saving_shows_the_domain_message_for_a_bad_line() {
    let mut values = filled_values();
    values.lines[0].quantity = "1.5".to_owned();

    assert_eq!(
        quote_change(&lead(), QuoteEdit::Save, &values, &[], &settings(), TODAY),
        Err("La cantidad debe ser un número entero mayor que cero".to_owned())
    );
}

#[test]
fn saving_names_a_missing_date() {
    let mut issued = filled_values();
    issued.issued_on = String::new();
    let mut valid = filled_values();
    valid.valid_until = String::new();

    assert_eq!(
        quote_change(&lead(), QuoteEdit::Save, &issued, &[], &settings(), TODAY),
        Err("Ingresa la fecha de emisión".to_owned())
    );
    assert_eq!(
        quote_change(&lead(), QuoteEdit::Save, &valid, &[], &settings(), TODAY),
        Err("Ingresa la fecha de validez".to_owned())
    );
}

#[test]
fn saving_an_edited_draft_revises_it_and_keeps_its_number() {
    let quote = draft_quote();
    let mut values = QuoteValues::of_quote(&quote);
    values.notes = "Revisada".to_owned();

    let change = quote_change(
        &lead(),
        QuoteEdit::Save,
        &values,
        std::slice::from_ref(&quote),
        &settings(),
        TODAY,
    )
    .unwrap();

    let QuoteChange::Draft(revised) = change else {
        panic!("expected a revision");
    };
    assert_eq!(revised.id(), quote.id());
    assert_eq!(revised.number(), quote.number());
    assert_eq!(revised.notes(), "Revisada");
}

#[test]
fn an_edit_of_a_quote_already_sent_is_refused() {
    let quote = sent_quote();
    let values = QuoteValues::of_quote(&quote);

    assert_eq!(
        quote_change(
            &lead(),
            QuoteEdit::Save,
            &values,
            &[quote],
            &settings(),
            TODAY
        ),
        Err("Una cotización enviada no se cambia: crea una nueva versión".to_owned())
    );
}

#[test]
fn sending_freezes_the_company_of_the_settings() {
    let change = quote_change(
        &lead(),
        QuoteEdit::Send(draft_quote()),
        &filled_values(),
        &[],
        &settings(),
        TODAY,
    )
    .unwrap();

    let QuoteChange::Status(quote, from) = change else {
        panic!("expected a status change");
    };
    assert_eq!(from, QuoteStatus::Draft);
    assert_eq!(quote.status(), QuoteStatus::Sent);
    assert_eq!(quote.company(), &company());
}

#[test]
fn accepting_and_rejecting_start_from_sent() {
    for (edit, expected) in [
        (QuoteEdit::Accept(sent_quote()), QuoteStatus::Accepted),
        (QuoteEdit::Reject(sent_quote()), QuoteStatus::Rejected),
    ] {
        let change = quote_change(&lead(), edit, &filled_values(), &[], &settings(), TODAY);

        let Ok(QuoteChange::Status(quote, from)) = change else {
            panic!("expected a status change");
        };
        assert_eq!(from, QuoteStatus::Sent);
        assert_eq!(quote.status(), expected);
    }
}

#[test]
fn a_superseded_quote_can_no_longer_be_decided() {
    let original = sent_quote();
    let quotes = [original.clone(), version_of(&original)];

    assert_eq!(
        quote_change(
            &lead(),
            QuoteEdit::Accept(original),
            &filled_values(),
            &quotes,
            &settings(),
            TODAY,
        ),
        Err("Esta versión ya fue reemplazada por otra más reciente".to_owned())
    );
}

#[test]
fn a_new_version_is_a_draft_of_the_same_number() {
    let original = sent_quote();

    let change = quote_change(
        &lead(),
        QuoteEdit::NewVersion(original.clone()),
        &filled_values(),
        &[],
        &settings(),
        TODAY,
    )
    .unwrap();

    let QuoteChange::Create(draft) = change else {
        panic!("expected a new version");
    };
    assert_eq!(draft.revised_quote(), Some(original.id()));
    assert_eq!(
        draft.next_version().unwrap().to_string(),
        "COT-2026-0001-V2"
    );
}

#[tokio::test]
async fn saving_persists_each_kind_of_change() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let lead = lead();
    store.save(&lead).await.unwrap();

    let created = quote_change(
        &lead,
        QuoteEdit::Save,
        &filled_values(),
        &[],
        &settings(),
        TODAY,
    )
    .unwrap();
    let created = save_quote(&store, created).await.unwrap();
    assert_eq!(created.number().to_string(), "COT-2026-0001");

    let mut values = QuoteValues::of_quote(&created);
    values.notes = "Revisada".to_owned();
    let revised = quote_change(
        &lead,
        QuoteEdit::Save,
        &values,
        std::slice::from_ref(&created),
        &settings(),
        TODAY,
    )
    .unwrap();
    let revised = save_quote(&store, revised).await.unwrap();
    assert_eq!(revised.notes(), "Revisada");

    let sent = quote_change(
        &lead,
        QuoteEdit::Send(revised),
        &filled_values(),
        &[],
        &settings(),
        TODAY,
    )
    .unwrap();
    let sent = save_quote(&store, sent).await.unwrap();

    assert_eq!(store.quotes().await.unwrap(), vec![sent]);
    store.close().await;
}

async fn stored_quote_at_final_step(store: &SqliteLeads, accept: bool) -> (Lead, Quote) {
    let lead = lead_named("Minera Peña Blanca", 2);
    store.save(&lead).await.unwrap();
    let draft =
        QuoteDraft::for_lead(&lead, content(vec![evaluation()]), IgvRate::default()).unwrap();
    let mut quote = store.create_quote(&draft).await.unwrap();
    quote.mark_sent(&company()).unwrap();
    store
        .save_quote_status(&quote, QuoteStatus::Draft)
        .await
        .unwrap();
    if accept {
        quote.mark_accepted().unwrap();
        store
            .save_quote_status(&quote, QuoteStatus::Sent)
            .await
            .unwrap();
    }
    (lead, quote)
}

#[tokio::test]
async fn an_accepted_quote_becomes_the_suggested_total_when_the_case_opens() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (lead, quote) = stored_quote_at_final_step(&store, true).await;

    save_advance(&store, lead, TODAY).await.unwrap();

    let cases = store.open_cases().await.unwrap();
    assert_eq!(cases[0].case.contract_total(), Some(quote.totals().total));
    assert_eq!(quote.totals().total, soles(354_000));
    store.close().await;
}

#[tokio::test]
async fn a_case_without_an_accepted_quote_opens_with_no_total() {
    let scratch = Scratch::new();
    let store = SqliteLeads::open(&scratch.database()).await.unwrap();
    let (lead, _) = stored_quote_at_final_step(&store, false).await;

    save_advance(&store, lead, TODAY).await.unwrap();

    assert_eq!(
        store.open_cases().await.unwrap()[0].case.contract_total(),
        None
    );
    store.close().await;
}

#[test]
fn the_pdf_shows_spanish_text_correctly() {
    let Some(text) = pdf_text(&sent_quote(), &company()) else {
        return;
    };

    for expected in [
        "Cotización",
        "Peña",
        "S/",
        "Evaluación de coordenadas",
        "¿Dudas? Escríbenos.",
        "Válida hasta",
        "Descripción",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
}

#[test]
fn the_pdf_carries_the_quote_data_and_totals() {
    let Some(text) = pdf_text(&sent_quote(), &company()) else {
        return;
    };

    for expected in [
        "COT-2026-0001",
        "07/11/2026",
        "22/11/2026",
        "20123456786",
        "DAC",
        "S/ 1,500.00",
        "S/ 3,000.00",
        "IGV (18%)",
        "S/ 540.00",
        "S/ 3,540.00",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
}

#[test]
fn the_pdf_shows_the_company_and_omits_blank_fields() {
    let partial = CompanyProfile {
        name: "Empresa Ñandú".to_owned(),
        email: "hola@example.com".to_owned(),
        ..CompanyProfile::default()
    };
    let Some(text) = pdf_text(&draft_quote(), &partial) else {
        return;
    };

    assert!(text.contains("Empresa Ñandú"), "{text}");
    assert!(text.contains("hola@example.com"), "{text}");
    assert!(!text.contains("Tel."), "{text}");
    assert!(!text.contains("null"), "{text}");
    // The client RUC stays; the company has no RUC line of its own.
    assert_eq!(text.matches("RUC").count(), 1, "{text}");
}

#[test]
fn the_pdf_invents_no_company_data() {
    let Some(text) = pdf_text(&draft_quote(), &CompanyProfile::default()) else {
        return;
    };

    assert!(!text.contains("COMINORSA"), "{text}");
}

#[test]
fn the_pdf_states_it_is_not_a_tax_invoice_and_never_mentions_sunat() {
    let Some(text) = pdf_text(&sent_quote(), &company()) else {
        return;
    };

    assert_eq!(
        QUOTE_TAX_NOTE,
        "Esta cotización no es un comprobante de pago ni una factura."
    );
    assert!(text.contains(QUOTE_TAX_NOTE), "{text}");
    assert!(!text.to_uppercase().contains("SUNAT"), "{text}");
}

#[test]
fn a_draft_pdf_says_it_is_a_draft_and_a_sent_one_does_not() {
    let (Some(draft), Some(sent)) = (
        pdf_text(&draft_quote(), &company()),
        pdf_text(&sent_quote(), &company()),
    ) else {
        return;
    };

    assert!(draft.contains("Borrador"), "{draft}");
    assert!(!sent.contains("Borrador"), "{sent}");
}

#[test]
fn a_long_quote_flows_over_pages_and_keeps_every_line() {
    let lines = (1..=60)
        .map(|index| QuoteLine::new(&format!("Servicio número {index}"), 1, soles(100)).unwrap())
        .collect();
    let quote = draft_of(&lead(), lines);
    let pdf = quote_pdf(&quote, &company()).unwrap();
    assert_valid_pdf(&pdf);
    let Some(text) = extracted_text(&pdf, &[]) else {
        return;
    };

    for index in 1..=60 {
        let label = format!("Servicio número {index}");
        let found = text.match_indices(&label).any(|(position, _)| {
            !text[position + label.len()..].starts_with(|next: char| next.is_ascii_digit())
        });
        assert!(found, "{index}");
    }
    let pages = text.matches('\u{c}').count();
    assert!(pages >= 2, "{pages} pages");
    assert!(
        text.contains(&format!("Página {pages} de {pages}")),
        "{text}"
    );
    assert_eq!(text.matches(QUOTE_TAX_NOTE).count(), pages);
    assert_eq!(text.matches("Descripción").count(), pages);
    assert!(text.contains("S/ 60.00"), "{text}");
}

#[test]
fn long_text_wraps_inside_the_page() {
    let long = "Trámite con una descripción muy extensa ".repeat(12);
    let word = "Supercalifragilístico".repeat(12);
    let mut quote = draft_of(
        &lead(),
        vec![
            QuoteLine::new(&long, 1, soles(100)).unwrap(),
            QuoteLine::new(&word, 1, soles(100)).unwrap(),
        ],
    );
    quote
        .revise(
            QuoteContent {
                notes: format!("{long}\n{word}"),
                ..content(quote.lines().to_vec())
            },
            IgvRate::default(),
        )
        .unwrap();
    let pdf = quote_pdf(&quote, &company()).unwrap();
    assert_valid_pdf(&pdf);
    let Some(boxes) = extracted_text(&pdf, &["-bbox"]) else {
        return;
    };

    let page_width: f32 = boxes
        .split("width=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .parse()
        .unwrap();
    let widest = boxes
        .split("xMax=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next()?.parse::<f32>().ok())
        .fold(0.0, f32::max);
    assert!(widest <= page_width - 40.0, "{widest} of {page_width}");
}

#[test]
fn the_pdf_embeds_only_the_glyphs_it_uses() {
    let pdf = quote_pdf(&sent_quote(), &company()).unwrap();

    assert!(pdf.len() < 150_000, "{} bytes", pdf.len());
}

#[test]
fn the_same_quote_produces_the_same_bytes() {
    let quote = sent_quote();

    assert_eq!(
        quote_pdf(&quote, &company()).unwrap(),
        quote_pdf(&quote, &company()).unwrap()
    );
}

#[test]
fn writes_the_pdf_where_the_person_chose() {
    let scratch = Scratch::new();
    let path = scratch.0.join("cotización.pdf");

    let written = write_pdf(&path, b"%PDF-").unwrap();

    assert_eq!(written, path);
    assert_eq!(fs::read(&path).unwrap(), b"%PDF-");
}

#[test]
fn adds_the_extension_when_the_dialog_returns_none() {
    let scratch = Scratch::new();

    let written = write_pdf(&scratch.0.join("cotizacion"), b"%PDF-").unwrap();

    assert_eq!(written, scratch.0.join("cotizacion.pdf"));
    assert!(written.exists());
}

#[test]
fn a_failed_write_reports_the_cause() {
    let scratch = Scratch::new();

    let failure = write_pdf(&scratch.0.join("falta/quote.pdf"), b"%PDF-").unwrap_err();

    assert!(
        failure.starts_with("No se pudo guardar el PDF: "),
        "{failure}"
    );
}

fn workspace_with_lead() -> Element {
    rsx! {
        Workspace {
            leads: vec![lead()],
            today: TODAY,
            query: String::new(),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
            on_quote: |_| {},
        }
    }
}

#[test]
fn a_lead_card_opens_its_quotes() {
    let html = render(workspace_with_lead);

    assert!(
        html.contains("Cotizaciones de Minera Peña Blanca"),
        "{html}"
    );
    assert!(html.contains(">Cotizar<"), "{html}");
}

fn panel(
    quotes: Vec<Quote>,
    values: QuoteValues,
    company: CompanyProfile,
    saving: bool,
    error: &str,
    notice: &str,
    confirming_send: Option<Uuid>,
) -> Element {
    rsx! {
        QuotePanel {
            lead: lead(),
            quotes,
            values,
            company,
            igv_rate: IgvRate::default(),
            saving,
            error: error.to_owned(),
            notice: notice.to_owned(),
            confirming_send,
            on_change: |_| {},
            on_edit: |_| {},
            on_load: |_| {},
            on_pdf: |_| {},
            on_confirm_send: |_| {},
            on_close: |_| {},
        }
    }
}

fn empty_panel() -> Element {
    panel(
        Vec::new(),
        QuoteValues::starting_on(TODAY),
        company(),
        false,
        "",
        "",
        None,
    )
}

#[test]
fn the_panel_starts_empty_with_a_form_and_the_tax_note() {
    let html = render(empty_panel);

    assert!(
        html.contains("Cotizaciones de Minera Peña Blanca"),
        "{html}"
    );
    assert!(html.contains("Aún no hay cotizaciones"), "{html}");
    assert!(html.contains("Descripción"), "{html}");
    assert!(html.contains("Agregar línea"), "{html}");
    assert!(html.contains("Guardar borrador"), "{html}");
    assert!(
        html.contains("Completa las líneas para ver el total"),
        "{html}"
    );
    assert!(html.contains(QUOTE_TAX_NOTE), "{html}");
    assert!(!html.contains("Falta el nombre"), "{html}");
    assert!(!html.to_uppercase().contains("SUNAT"), "{html}");
}

fn filled_panel() -> Element {
    panel(Vec::new(), filled_values(), company(), false, "", "", None)
}

#[test]
fn the_panel_previews_the_totals_while_typing() {
    let html = render(filled_panel);

    assert!(html.contains("Subtotal"), "{html}");
    assert!(html.contains("S/ 2,000.00"), "{html}");
    assert!(html.contains("IGV (18%)"), "{html}");
    assert!(html.contains("S/ 360.00"), "{html}");
    assert!(html.contains("S/ 2,360.00"), "{html}");
    assert!(!html.contains("Completa las líneas"), "{html}");
}

fn saving_panel() -> Element {
    panel(
        vec![draft_quote()],
        filled_values(),
        company(),
        true,
        "",
        "",
        None,
    )
}

#[test]
fn every_save_of_the_panel_is_disabled_while_pending() {
    let html = render(saving_panel);

    assert!(html.contains("Guardando…"), "{html}");
    assert!(!html.contains(">Guardar borrador<"), "{html}");
    let enabled = html
        .split("<button")
        .skip(1)
        .map(|button| button.split('>').next().unwrap())
        .filter(|button| !button.contains("disabled") && !button.contains("Cerrar"))
        .count();
    assert_eq!(enabled, 0, "{html}");
}

fn panel_with_error() -> Element {
    panel(
        vec![sent_quote()],
        QuoteValues::starting_on(TODAY),
        company(),
        false,
        "No se pudo guardar",
        "PDF guardado",
        None,
    )
}

#[test]
fn the_panel_shows_errors_and_notices() {
    let html = render(panel_with_error);

    assert!(html.contains(r#"role="alert""#), "{html}");
    assert!(html.contains("No se pudo guardar"), "{html}");
    assert!(html.contains(r#"role="status""#), "{html}");
    assert!(html.contains("PDF guardado"), "{html}");
}

fn panel_without_company() -> Element {
    panel(
        Vec::new(),
        QuoteValues::starting_on(TODAY),
        CompanyProfile::default(),
        false,
        "",
        "",
        None,
    )
}

#[test]
fn the_panel_warns_about_a_missing_company_name() {
    let html = render(panel_without_company);

    assert!(html.contains("Falta el nombre de la empresa"), "{html}");
}

fn panel_with_draft() -> Element {
    panel(
        vec![draft_quote()],
        QuoteValues::starting_on(TODAY),
        company(),
        false,
        "",
        "",
        None,
    )
}

#[test]
fn a_draft_can_be_edited_sent_or_exported() {
    let html = render(panel_with_draft);

    assert!(html.contains("COT-2026-0001"), "{html}");
    assert!(html.contains("Borrador"), "{html}");
    assert!(html.contains("S/ 3,540.00"), "{html}");
    assert!(html.contains("Válida hasta 22/11/2026"), "{html}");
    assert!(html.contains(">Editar<"), "{html}");
    assert!(html.contains(">Marcar enviada<"), "{html}");
    assert!(html.contains(">Guardar PDF<"), "{html}");
    assert!(!html.contains(">Nueva versión<"), "{html}");
    assert!(!html.contains(">Marcar aceptada<"), "{html}");
}

fn panel_with_sent() -> Element {
    panel(
        vec![sent_quote()],
        QuoteValues::starting_on(TODAY),
        company(),
        false,
        "",
        "",
        None,
    )
}

#[test]
fn a_sent_quote_is_decided_or_versioned_and_never_edited() {
    let html = render(panel_with_sent);

    assert!(html.contains(">Marcar aceptada<"), "{html}");
    assert!(html.contains(">Marcar rechazada<"), "{html}");
    assert!(html.contains(">Nueva versión<"), "{html}");
    assert!(html.contains(">Guardar PDF<"), "{html}");
    assert!(!html.contains(">Editar<"), "{html}");
    assert!(!html.contains(">Marcar enviada<"), "{html}");
}

fn panel_with_accepted() -> Element {
    panel(
        vec![accepted_quote()],
        QuoteValues::starting_on(TODAY),
        company(),
        false,
        "",
        "",
        None,
    )
}

#[test]
fn an_accepted_quote_is_only_versioned() {
    let html = render(panel_with_accepted);

    assert!(html.contains("Aceptada"), "{html}");
    assert!(html.contains(">Nueva versión<"), "{html}");
    assert!(!html.contains(">Marcar aceptada<"), "{html}");
    assert!(!html.contains(">Marcar rechazada<"), "{html}");
}

fn panel_with_superseded() -> Element {
    let original = sent_quote();
    let version = version_of(&original);

    panel(
        vec![original, version],
        QuoteValues::starting_on(TODAY),
        company(),
        false,
        "",
        "",
        None,
    )
}

#[test]
fn a_superseded_quote_only_exports_its_pdf() {
    let html = render(panel_with_superseded);

    assert!(html.contains("COT-2026-0001-V2"), "{html}");
    assert!(
        html.contains("Reemplazada por una versión más reciente"),
        "{html}"
    );
    assert_eq!(html.matches(">Nueva versión<").count(), 0, "{html}");
    assert_eq!(html.matches(">Marcar aceptada<").count(), 0, "{html}");
    assert_eq!(html.matches(">Guardar PDF<").count(), 2, "{html}");
}

fn panel_confirming_without_company() -> Element {
    let quote = draft_quote();
    let id = quote.id();

    panel(
        vec![quote],
        QuoteValues::starting_on(TODAY),
        CompanyProfile::default(),
        false,
        "",
        "",
        Some(id),
    )
}

#[test]
fn sending_without_a_company_name_asks_first() {
    let html = render(panel_confirming_without_company);

    assert!(html.contains(r#"role="alertdialog""#), "{html}");
    assert!(
        html.contains("¿Marcar como enviada sin nombre de empresa?"),
        "{html}"
    );
    assert!(html.contains(">Enviar sin nombre<"), "{html}");
    assert!(html.contains(">Cancelar<"), "{html}");
}

fn settings_with_company() -> Element {
    let mut settings = Settings::default();
    settings
        .set_company(CompanyProfile {
            name: "Empresa Ejemplo SAC".to_owned(),
            ..CompanyProfile::default()
        })
        .unwrap();

    rsx! {
        SettingsView {
            settings,
            database: "/datos/crm.db".to_owned(),
            save_state: SaveState::Idle,
            backup_notice: String::new(),
            pending_restore: None,
            busy: false,
            on_change: |_| {},
            on_pick_folder: |_| {},
            on_backup: |_| {},
            on_pick_restore: |_| {},
            on_confirm_restore: |_| {},
            on_cancel_restore: |_| {},
        }
    }
}

#[test]
fn settings_edit_the_company_and_the_igv_for_review() {
    let html = render(settings_with_company);

    assert!(html.contains("Empresa"), "{html}");
    for label in [
        "Nombre de la empresa",
        "RUC de la empresa",
        "Dirección",
        "Teléfono de la empresa",
        "Correo de la empresa",
        "Tasa de IGV en porcentaje",
    ] {
        assert!(html.contains(label), "missing {label:?} in {html}");
    }
    assert!(html.contains(r#"value="Empresa Ejemplo SAC""#), "{html}");
    assert!(html.contains(r#"value="18""#), "{html}");
    assert!(
        html.contains("Por revisar: confirma la tasa con tu contador."),
        "{html}"
    );
}

fn case_with_gap() -> Element {
    rsx! {
        CaseDetail {
            file: planned_case(100_000, &[60_000]),
            today: TODAY,
            saving: false,
            error: String::new(),
            total_note: Some("Sugerido por COT-2026-0001; ajústalo si el contrato difiere.".to_owned()),
            on_back: |_| {},
            on_toggle: |_| {},
            on_due_date: |_| {},
        }
    }
}

#[test]
fn the_case_detail_warns_about_the_gap_and_names_the_suggestion() {
    let html = render(case_with_gap);

    assert!(html.contains("Los hitos suman S/ 600.00"), "{html}");
    assert!(html.contains("faltan S/ 400.00"), "{html}");
    assert!(html.contains(r#"role="status""#), "{html}");
    assert!(html.contains("Sugerido por COT-2026-0001"), "{html}");
}

fn case_without_gap() -> Element {
    rsx! {
        CaseDetail {
            file: planned_case(100_000, &[100_000]),
            today: TODAY,
            saving: false,
            error: String::new(),
            on_back: |_| {},
            on_toggle: |_| {},
            on_due_date: |_| {},
        }
    }
}

#[test]
fn a_case_that_sums_shows_no_warning() {
    let html = render(case_without_gap);

    assert!(!html.contains("Los hitos suman"), "{html}");
}

#[test]
fn the_style_sheet_defines_the_quote_styles() {
    let style = include_str!("../src/style.css");

    for selector in [".quote-panel", ".quote-row", ".quote-line", ".warning-note"] {
        assert!(style.contains(selector), "missing {selector}");
    }
}
