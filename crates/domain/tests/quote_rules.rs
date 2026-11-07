use cominorsa_domain::{
    CaseClosure, CaseFile, CompanyProfile, DomainError, IgvRate, Lead, Money, NewLead, Quote,
    QuoteContent, QuoteDraft, QuoteLine, QuoteLineRecord, QuoteNumber, QuoteRecord, QuoteStatus,
    QuoteTotals, accepted_quote, is_superseded, procedure_template,
};
use time::{Date, macros::date};
use uuid::Uuid;

const ISSUED_ON: Date = date!(2026 - 11 - 07);
const VALID_UNTIL: Date = date!(2026 - 11 - 22);

fn soles(cents: i64) -> Money {
    Money::from_cents(cents).unwrap()
}

fn lead() -> Lead {
    Lead::create(NewLead {
        name: "Minera Peña Blanca".to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: "REINFO".to_owned(),
        owner: None,
        follow_up_on: ISSUED_ON,
    })
    .unwrap()
}

fn line(quantity: u32, cents: i64) -> QuoteLine {
    QuoteLine::new("Evaluación de coordenadas", quantity, soles(cents)).unwrap()
}

fn content(lines: Vec<QuoteLine>) -> QuoteContent {
    QuoteContent {
        issued_on: ISSUED_ON,
        valid_until: VALID_UNTIL,
        lines,
        notes: "Incluye visita a campo".to_owned(),
    }
}

fn numbered(draft: QuoteDraft, sequence: u32) -> Quote {
    draft.numbered(QuoteNumber::new(2026, sequence, 1).unwrap())
}

fn draft_quote() -> Quote {
    let draft =
        QuoteDraft::for_lead(&lead(), content(vec![line(1, 100_000)]), IgvRate::default()).unwrap();
    numbered(draft, 1)
}

fn sent_quote() -> Quote {
    let mut quote = draft_quote();
    quote.mark_sent(&CompanyProfile::default()).unwrap();
    quote
}

fn record() -> QuoteRecord {
    QuoteRecord {
        id: Uuid::now_v7(),
        lead_id: Uuid::now_v7(),
        year: 2026,
        sequence: 1,
        version: 1,
        supersedes: None,
        client_name: "Minera Peña Blanca".to_owned(),
        client_ruc: Some("20123456786".to_owned()),
        service: "REINFO".to_owned(),
        issued_on: ISSUED_ON,
        valid_until: VALID_UNTIL,
        notes: String::new(),
        status: "sent".to_owned(),
        igv_basis_points: 1800,
        company: CompanyProfile::default(),
        lines: vec![QuoteLineRecord {
            description: "Evaluación".to_owned(),
            quantity: 2,
            unit_price_cents: 10_000,
        }],
    }
}

#[test]
fn the_igv_rate_defaults_to_eighteen_percent() {
    assert_eq!(IgvRate::default().basis_points(), 1800);
}

#[test]
fn reads_a_typed_igv_rate_without_floats() {
    assert_eq!(IgvRate::parse("18").unwrap().basis_points(), 1800);
    assert_eq!(IgvRate::parse(" 10.5 ").unwrap().basis_points(), 1050);
    assert_eq!(IgvRate::parse("0").unwrap().basis_points(), 0);
    assert_eq!(IgvRate::parse("100").unwrap().basis_points(), 10_000);
}

#[test]
fn refuses_an_igv_rate_out_of_range_or_badly_written() {
    for text in ["", "abc", "100.01", "-1", "18,5", "18.555", "18."] {
        assert_eq!(
            IgvRate::parse(text),
            Err(DomainError::InvalidIgvRate),
            "{text:?}"
        );
    }
    assert_eq!(
        IgvRate::from_basis_points(10_001),
        Err(DomainError::InvalidIgvRate)
    );
}

#[test]
fn formats_the_number_with_year_and_four_digits() {
    assert_eq!(
        QuoteNumber::new(2026, 1, 1).unwrap().to_string(),
        "COT-2026-0001"
    );
    assert_eq!(
        QuoteNumber::new(2027, 12_345, 1).unwrap().to_string(),
        "COT-2027-12345"
    );
}

#[test]
fn a_later_version_keeps_the_number_and_adds_a_suffix() {
    assert_eq!(
        QuoteNumber::new(2026, 7, 2).unwrap().to_string(),
        "COT-2026-0007-V2"
    );
}

#[test]
fn refuses_a_number_with_a_zero_sequence_or_version() {
    assert_eq!(
        QuoteNumber::new(2026, 0, 1),
        Err(DomainError::CorruptRecord)
    );
    assert_eq!(
        QuoteNumber::new(2026, 1, 0),
        Err(DomainError::CorruptRecord)
    );
}

#[test]
fn every_status_round_trips_through_its_code_and_has_a_spanish_label() {
    let labels: Vec<_> = QuoteStatus::ALL
        .iter()
        .map(|status| status.label())
        .collect();

    assert_eq!(labels, ["Borrador", "Enviada", "Aceptada", "Rechazada"]);
    for status in QuoteStatus::ALL {
        assert_eq!(QuoteStatus::from_code(status.code()), Some(status));
    }
    assert_eq!(QuoteStatus::from_code("archived"), None);
}

#[test]
fn a_line_total_is_quantity_times_unit_price() {
    let line = line(3, 12_345);

    assert_eq!(line.total(), soles(37_035));
    assert_eq!(line.quantity(), 3);
    assert_eq!(line.unit_price(), soles(12_345));
}

#[test]
fn a_line_needs_a_description() {
    assert_eq!(
        QuoteLine::new("  ", 1, soles(100)),
        Err(DomainError::MissingField("descripción"))
    );
}

#[test]
fn a_line_trims_its_description() {
    assert_eq!(
        QuoteLine::new("  Asesoría ", 1, soles(100))
            .unwrap()
            .description(),
        "Asesoría"
    );
}

#[test]
fn a_line_needs_a_positive_quantity() {
    assert_eq!(
        QuoteLine::new("Asesoría", 0, soles(100)),
        Err(DomainError::InvalidQuantity)
    );
}

#[test]
fn a_line_needs_a_positive_unit_price() {
    assert_eq!(
        QuoteLine::new("Asesoría", 1, soles(0)),
        Err(DomainError::InvalidAmount)
    );
}

#[test]
fn a_line_refuses_a_total_beyond_the_money_limit() {
    assert_eq!(
        QuoteLine::new("Asesoría", u32::MAX, soles(10_000_000_000_000)),
        Err(DomainError::InvalidAmount)
    );
}

#[test]
fn reads_a_line_as_typed() {
    let line = QuoteLine::parse(" Asesoría ", " 2 ", "S/ 1,234.50").unwrap();

    assert_eq!(line.description(), "Asesoría");
    assert_eq!(line.quantity(), 2);
    assert_eq!(line.unit_price(), soles(123_450));
}

#[test]
fn refuses_a_typed_quantity_that_is_not_a_whole_number() {
    for quantity in ["", "1.5", "dos", "-1", "0"] {
        assert_eq!(
            QuoteLine::parse("Asesoría", quantity, "100"),
            Err(DomainError::InvalidQuantity),
            "{quantity:?}"
        );
    }
}

#[test]
fn totals_add_the_lines_and_charge_igv_on_the_subtotal() {
    let totals = QuoteTotals::of(&[line(2, 10_000), line(1, 5_000)], IgvRate::default()).unwrap();

    assert_eq!(
        totals,
        QuoteTotals {
            subtotal: soles(25_000),
            igv: soles(4_500),
            total: soles(29_500),
        }
    );
}

#[test]
fn igv_rounds_half_a_cent_up() {
    // 25 cents at 18 percent is exactly 4.5 cents.
    let half = QuoteTotals::of(&[line(1, 25)], IgvRate::default()).unwrap();
    // 26 cents at 18 percent is 4.68 cents, and 24 cents is 4.32 cents.
    let above = QuoteTotals::of(&[line(1, 26)], IgvRate::default()).unwrap();
    let below = QuoteTotals::of(&[line(1, 24)], IgvRate::default()).unwrap();

    assert_eq!(half.igv, soles(5));
    assert_eq!(half.total, soles(30));
    assert_eq!(above.igv, soles(5));
    assert_eq!(below.igv, soles(4));
}

#[test]
fn igv_is_charged_once_on_the_subtotal_and_not_line_by_line() {
    // Each line alone would round 4.5 up to 5 cents, adding 10.
    let totals = QuoteTotals::of(&[line(1, 25), line(1, 25)], IgvRate::default()).unwrap();

    assert_eq!(totals.igv, soles(9));
}

#[test]
fn a_rate_of_zero_charges_no_igv() {
    let rate = IgvRate::from_basis_points(0).unwrap();

    assert_eq!(
        QuoteTotals::of(&[line(1, 100)], rate).unwrap().total,
        soles(100)
    );
}

#[test]
fn totals_refuse_a_sum_beyond_the_money_limit() {
    let full = line(1, 10_000_000_000_000);

    assert_eq!(
        QuoteTotals::of(&[full.clone(), full], IgvRate::default()),
        Err(DomainError::InvalidAmount)
    );
}

#[test]
fn a_new_quote_copies_the_client_of_the_lead_and_starts_as_a_draft() {
    let lead = lead();
    let draft =
        QuoteDraft::for_lead(&lead, content(vec![line(1, 100_000)]), IgvRate::default()).unwrap();
    let quote = numbered(draft, 3);

    assert_eq!(quote.lead_id(), lead.id());
    assert_eq!(quote.client_name(), "Minera Peña Blanca");
    assert_eq!(quote.client_ruc().unwrap().as_str(), "20123456786");
    assert_eq!(quote.service(), "REINFO");
    assert_eq!(quote.status(), QuoteStatus::Draft);
    assert_eq!(quote.number().to_string(), "COT-2026-0003");
    assert_eq!(quote.issued_on(), ISSUED_ON);
    assert_eq!(quote.valid_until(), VALID_UNTIL);
    assert_eq!(quote.notes(), "Incluye visita a campo");
    assert_eq!(quote.lines().len(), 1);
    assert_eq!(quote.totals().total, soles(118_000));
    assert_eq!(quote.igv_rate(), IgvRate::default());
}

#[test]
fn a_quote_is_numbered_in_the_year_it_is_issued() {
    let mut written = content(vec![line(1, 100)]);
    written.issued_on = date!(2027 - 01 - 02);
    written.valid_until = date!(2027 - 01 - 03);
    let draft = QuoteDraft::for_lead(&lead(), written, IgvRate::default()).unwrap();

    assert_eq!(draft.year(), 2027);
}

#[test]
fn a_renewal_quote_names_the_client_without_the_renewal_prefix() {
    let mut signed = Lead::create(NewLead {
        name: "Minera Peña Blanca".to_owned(),
        phone: "900000000".to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        owner: None,
        follow_up_on: ISSUED_ON,
    })
    .unwrap();
    for _ in 0..3 {
        signed.advance().unwrap();
    }
    let opening = CaseFile::open(&signed, ISSUED_ON, &procedure_template("DAC")).unwrap();
    let renewal = CaseClosure::of(opening.case, &signed, ISSUED_ON)
        .renewal
        .unwrap();
    let draft =
        QuoteDraft::for_lead(&renewal, content(vec![line(1, 100)]), IgvRate::default()).unwrap();

    assert_ne!(renewal.name(), "Minera Peña Blanca");
    assert_eq!(numbered(draft, 1).client_name(), "Minera Peña Blanca");
}

#[test]
fn a_quote_needs_at_least_one_line() {
    assert_eq!(
        QuoteDraft::for_lead(&lead(), content(Vec::new()), IgvRate::default()),
        Err(DomainError::EmptyQuote)
    );
}

#[test]
fn the_validity_may_end_on_the_issue_date_but_not_before() {
    let mut written = content(vec![line(1, 100)]);
    written.valid_until = ISSUED_ON;
    assert!(QuoteDraft::for_lead(&lead(), written.clone(), IgvRate::default()).is_ok());

    written.valid_until = date!(2026 - 11 - 06);
    assert_eq!(
        QuoteDraft::for_lead(&lead(), written, IgvRate::default()),
        Err(DomainError::InvalidQuoteValidity)
    );
}

#[test]
fn a_draft_can_change_its_content_and_recalculates_totals() {
    let mut quote = draft_quote();

    quote
        .revise(
            content(vec![line(2, 100_000)]),
            IgvRate::from_basis_points(1000).unwrap(),
        )
        .unwrap();

    assert_eq!(quote.totals().subtotal, soles(200_000));
    assert_eq!(quote.totals().igv, soles(20_000));
    assert_eq!(quote.igv_rate().basis_points(), 1000);
    assert_eq!(quote.number().to_string(), "COT-2026-0001");
}

#[test]
fn a_revision_is_validated_like_a_new_quote() {
    let mut quote = draft_quote();

    assert_eq!(
        quote.revise(content(Vec::new()), IgvRate::default()),
        Err(DomainError::EmptyQuote)
    );
    assert_eq!(quote.totals().total, soles(118_000));
}

#[test]
fn a_sent_quote_is_immutable() {
    let mut quote = sent_quote();
    let before = quote.clone();

    assert_eq!(
        quote.revise(content(vec![line(9, 1)]), IgvRate::default()),
        Err(DomainError::QuoteLocked)
    );
    assert_eq!(quote, before);
}

#[test]
fn accepted_and_rejected_quotes_are_immutable_too() {
    let mut accepted = sent_quote();
    accepted.mark_accepted().unwrap();
    let mut rejected = sent_quote();
    rejected.mark_rejected().unwrap();

    for mut quote in [accepted, rejected] {
        assert_eq!(
            quote.revise(content(vec![line(9, 1)]), IgvRate::default()),
            Err(DomainError::QuoteLocked)
        );
    }
}

#[test]
fn only_a_draft_can_be_sent() {
    let mut quote = sent_quote();

    assert_eq!(
        quote.mark_sent(&CompanyProfile::default()),
        Err(DomainError::InvalidQuoteTransition)
    );
    assert_eq!(quote.status(), QuoteStatus::Sent);
}

#[test]
fn only_a_sent_quote_can_be_accepted_or_rejected() {
    let mut draft = draft_quote();

    assert_eq!(
        draft.mark_accepted(),
        Err(DomainError::InvalidQuoteTransition)
    );
    assert_eq!(
        draft.mark_rejected(),
        Err(DomainError::InvalidQuoteTransition)
    );
    assert_eq!(draft.status(), QuoteStatus::Draft);

    let mut accepted = sent_quote();
    accepted.mark_accepted().unwrap();
    assert_eq!(accepted.status(), QuoteStatus::Accepted);
    assert_eq!(
        accepted.mark_rejected(),
        Err(DomainError::InvalidQuoteTransition)
    );

    let mut rejected = sent_quote();
    rejected.mark_rejected().unwrap();
    assert_eq!(rejected.status(), QuoteStatus::Rejected);
    assert_eq!(
        rejected.mark_accepted(),
        Err(DomainError::InvalidQuoteTransition)
    );
}

#[test]
fn sending_freezes_the_company_and_a_draft_follows_the_current_profile() {
    let first = CompanyProfile {
        name: "Empresa Uno".to_owned(),
        ..CompanyProfile::default()
    };
    let later = CompanyProfile {
        name: "Empresa Dos".to_owned(),
        ..CompanyProfile::default()
    };
    let mut quote = draft_quote();

    assert_eq!(quote.document_company(&first).name, "Empresa Uno");

    quote.mark_sent(&first).unwrap();

    assert_eq!(quote.company(), &first);
    assert_eq!(quote.document_company(&later).name, "Empresa Uno");
}

#[test]
fn a_new_version_keeps_the_number_family_and_starts_as_a_draft() {
    let original = sent_quote();
    let draft = original
        .new_version(date!(2026 - 12 - 01), IgvRate::default())
        .unwrap();

    assert_eq!(draft.revised_quote(), Some(original.id()));
    let number = draft.next_version().unwrap();
    assert_eq!(number.to_string(), "COT-2026-0001-V2");

    let version = draft.numbered(number);
    assert_ne!(version.id(), original.id());
    assert_eq!(version.status(), QuoteStatus::Draft);
    assert_eq!(version.supersedes(), Some(original.id()));
    assert_eq!(version.lead_id(), original.lead_id());
    assert_eq!(version.client_name(), original.client_name());
    assert_eq!(version.lines(), original.lines());
    assert_eq!(version.notes(), original.notes());
    assert_eq!(version.issued_on(), date!(2026 - 12 - 01));
    // The original validity lasted 15 days.
    assert_eq!(version.valid_until(), date!(2026 - 12 - 16));
    assert_eq!(version.company(), &CompanyProfile::default());
}

#[test]
fn a_new_version_uses_the_current_igv_rate() {
    let rate = IgvRate::from_basis_points(1000).unwrap();
    let version = {
        let draft = sent_quote().new_version(ISSUED_ON, rate).unwrap();
        let number = draft.next_version().unwrap();
        draft.numbered(number)
    };

    assert_eq!(version.igv_rate(), rate);
    assert_eq!(version.totals().igv, soles(10_000));
}

#[test]
fn a_new_version_leaves_the_original_unchanged() {
    let original = sent_quote();
    let before = original.clone();

    original.new_version(ISSUED_ON, IgvRate::default()).unwrap();

    assert_eq!(original, before);
}

#[test]
fn a_new_quote_has_no_version_to_follow() {
    let draft =
        QuoteDraft::for_lead(&lead(), content(vec![line(1, 100)]), IgvRate::default()).unwrap();

    assert_eq!(draft.next_version(), None);
    assert_eq!(draft.revised_quote(), None);
}

#[test]
fn a_draft_is_edited_and_gets_no_new_version() {
    assert_eq!(
        draft_quote().new_version(ISSUED_ON, IgvRate::default()),
        Err(DomainError::InvalidQuoteTransition)
    );
}

#[test]
fn a_quote_knows_when_a_newer_version_exists() {
    let original = sent_quote();
    let version = {
        let draft = original.new_version(ISSUED_ON, IgvRate::default()).unwrap();
        let number = draft.next_version().unwrap();
        draft.numbered(number)
    };
    let quotes = [original.clone(), version.clone()];

    assert!(is_superseded(&original, &quotes));
    assert!(!is_superseded(&version, &quotes));
}

#[test]
fn the_newest_accepted_quote_of_the_lead_is_the_suggestion() {
    let lead = lead();
    let quote = |sequence, cents, status: fn(&mut Quote)| {
        let draft =
            QuoteDraft::for_lead(&lead, content(vec![line(1, cents)]), IgvRate::default()).unwrap();
        let mut quote = numbered(draft, sequence);
        quote.mark_sent(&CompanyProfile::default()).unwrap();
        status(&mut quote);
        quote
    };
    let older = quote(1, 100, |quote| quote.mark_accepted().unwrap());
    let newer = quote(2, 200, |quote| quote.mark_accepted().unwrap());
    let rejected = quote(3, 300, |quote| quote.mark_rejected().unwrap());
    let sent = quote(4, 400, |_| {});
    let other_lead = sent_quote();
    let quotes = [newer.clone(), older, rejected, sent, other_lead];

    assert_eq!(accepted_quote(&quotes, lead.id()), Some(&newer));
    assert_eq!(accepted_quote(&quotes, Uuid::now_v7()), None);
}

#[test]
fn an_accepted_quote_suggests_the_total_when_the_case_opens() {
    let mut signed = lead();
    for _ in 0..3 {
        signed.advance().unwrap();
    }
    let draft =
        QuoteDraft::for_lead(&signed, content(vec![line(1, 100_000)]), IgvRate::default()).unwrap();
    let mut quote = numbered(draft, 1);
    quote.mark_sent(&CompanyProfile::default()).unwrap();
    let unaccepted = quote.clone();
    quote.mark_accepted().unwrap();
    let mut opening = CaseFile::open(&signed, ISSUED_ON, &procedure_template("DAC")).unwrap();

    opening.suggest_total_from(&[unaccepted]).unwrap();
    assert_eq!(opening.case.contract_total(), None);

    opening.suggest_total_from(&[quote]).unwrap();
    assert_eq!(opening.case.contract_total(), Some(soles(118_000)));
}

#[test]
fn restores_a_stored_quote_and_recalculates_its_totals() {
    let stored = record();
    let id = stored.id;
    let quote = Quote::restore(stored).unwrap();

    assert_eq!(quote.id(), id);
    assert_eq!(quote.status(), QuoteStatus::Sent);
    assert_eq!(quote.number().to_string(), "COT-2026-0001");
    assert_eq!(quote.totals().total, soles(23_600));
}

#[test]
fn refuses_a_corrupt_stored_quote() {
    let corruptions: [fn(&mut QuoteRecord); 11] = [
        |record| record.status = "archived".to_owned(),
        |record| record.sequence = 0,
        |record| record.version = -1,
        |record| record.year = i64::MAX,
        |record| record.client_name = " ".to_owned(),
        |record| record.client_ruc = Some("123".to_owned()),
        |record| record.igv_basis_points = 20_000,
        |record| record.lines.clear(),
        |record| record.lines[0].quantity = 0,
        |record| record.lines[0].unit_price_cents = -5,
        |record| record.valid_until = date!(2020 - 01 - 01),
    ];

    for (index, corrupt) in corruptions.iter().enumerate() {
        let mut stored = record();
        corrupt(&mut stored);

        assert_eq!(
            Quote::restore(stored),
            Err(DomainError::CorruptRecord),
            "corruption {index}"
        );
    }
}

#[test]
fn a_draft_can_take_an_injected_id_and_keeps_everything_else() {
    let draft =
        QuoteDraft::for_lead(&lead(), content(vec![line(1, 100_000)]), IgvRate::default()).unwrap();
    let id = uuid::Uuid::from_u128(7);

    let keyed = draft.clone().with_id(id);

    assert_eq!(keyed.id(), id);
    assert_eq!(keyed.year(), draft.year());
    let (quote, original) = (numbered(keyed, 1), numbered(draft, 1));
    assert_eq!(quote.id(), id);
    assert_eq!(quote.lead_id(), original.lead_id());
    assert_eq!(quote.lines(), original.lines());
    assert_eq!(quote.totals(), original.totals());
}
