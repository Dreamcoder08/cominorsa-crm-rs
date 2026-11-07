use cominorsa_domain::{
    BillingMilestone, BillingMilestoneRecord, BillingSummary, CaseFile, DomainError, Lead, Money,
    NewLead, NewMilestone, procedure_template,
};
use time::{Date, macros::date};
use uuid::Uuid;

const TODAY: Date = date!(2026 - 11 - 07);

fn soles(cents: i64) -> Money {
    Money::from_cents(cents).unwrap()
}

fn case_file() -> CaseFile {
    let mut lead = Lead::create(NewLead {
        name: "Minera Ejemplo".to_owned(),
        phone: "900000000".to_owned(),
        ruc: None,
        service: "DAC".to_owned(),
        owner: None,
        follow_up_on: TODAY,
    })
    .unwrap();
    lead.advance().unwrap();
    lead.advance().unwrap();
    lead.advance().unwrap();

    CaseFile::open(&lead, TODAY, &procedure_template("DAC")).unwrap()
}

fn with_total(cents: i64) -> CaseFile {
    let mut file = case_file();
    file.case.set_contract_total(soles(cents)).unwrap();
    file
}

fn milestone(position: u32, cents: i64) -> BillingMilestone {
    BillingMilestone::create(NewMilestone {
        case_id: Uuid::now_v7(),
        position,
        title: "Adelanto".to_owned(),
        amount: soles(cents),
        due_on: TODAY,
    })
    .unwrap()
}

fn record() -> BillingMilestoneRecord {
    BillingMilestoneRecord {
        id: Uuid::now_v7(),
        case_id: Uuid::now_v7(),
        position: 1,
        title: "Adelanto".to_owned(),
        amount_cents: 1_000,
        due_on: TODAY,
        paid_on: None,
    }
}

#[test]
fn parses_typed_amounts_into_exact_cents() {
    for (text, cents) in [
        ("1234.50", 123_450),
        ("1,234.50", 123_450),
        ("S/ 1,234.50", 123_450),
        ("  S/1234.5 ", 123_450),
        ("1,234,567", 123_456_700),
        ("0.07", 7),
        ("1234", 123_400),
        ("0.29", 29),
        ("1.10", 110),
    ] {
        assert_eq!(Money::parse(text).map(Money::cents), Ok(cents), "{text}");
    }
}

#[test]
fn refuses_amounts_it_cannot_read_exactly() {
    for text in [
        "",
        "abc",
        "0",
        "0.00",
        "-5",
        "+5",
        "1.234",
        "1234,50",
        "1,23",
        "12,3456",
        "1.",
        ".50",
        "1.5.0",
        "1,,000",
        "99999999999999999999",
        "100000000001",
        "1e3",
        "١٢",
    ] {
        assert_eq!(
            Money::parse(text),
            Err(DomainError::InvalidAmount),
            "{text}"
        );
    }
}

#[test]
fn money_stays_within_its_range() {
    assert_eq!(Money::from_cents(-1), Err(DomainError::InvalidAmount));
    assert_eq!(soles(0).cents(), 0);
}

#[test]
fn suggests_a_thirty_forty_thirty_plan_with_editable_dates() {
    let file = with_total(1_000_000);

    let plan = file.suggested_milestones(TODAY).unwrap();

    let summary: Vec<(u32, &str, i64, Date)> = plan
        .iter()
        .map(|item| {
            (
                item.position(),
                item.title(),
                item.amount().cents(),
                item.due_on(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (1, "Adelanto", 300_000, TODAY),
            (2, "Avance", 400_000, date!(2026 - 12 - 07)),
            (3, "Saldo final", 300_000, date!(2027 - 01 - 06)),
        ]
    );
    assert!(plan.iter().all(|item| !item.is_paid()));
    assert!(plan.iter().all(|item| item.case_id() == file.case.id()));
}

#[test]
fn the_last_portion_takes_the_remainder_cents() {
    let plan = with_total(100_001).suggested_milestones(TODAY).unwrap();

    let cents: Vec<i64> = plan.iter().map(|item| item.amount().cents()).collect();

    assert_eq!(cents, [30_000, 40_000, 30_001]);
}

#[test]
fn a_plan_always_sums_exactly_to_the_total() {
    for total in (1..=2_500).chain([99_999, 100_003, 1_234_567_891]) {
        let plan = with_total(total).suggested_milestones(TODAY).unwrap();

        let sum: i64 = plan.iter().map(|item| item.amount().cents()).sum();

        assert_eq!(sum, total);
        assert!(plan.iter().all(|item| item.amount().cents() > 0));
    }
}

#[test]
fn a_tiny_total_leaves_out_portions_that_round_to_zero() {
    let plan = with_total(2).suggested_milestones(TODAY).unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].title(), "Saldo final");
    assert_eq!(plan[0].position(), 1);
    assert_eq!(plan[0].amount().cents(), 2);
}

#[test]
fn the_plan_needs_a_total_and_a_case_without_milestones() {
    assert_eq!(
        case_file().suggested_milestones(TODAY),
        Err(DomainError::MissingContractTotal)
    );

    let mut planned = with_total(1_000);
    planned.milestones.push(milestone(1, 1_000));
    assert_eq!(
        planned.suggested_milestones(TODAY),
        Err(DomainError::MilestonesAlreadyPlanned)
    );
}

#[test]
fn creates_a_valid_milestone_and_refuses_blank_or_zero_ones() {
    let input = |title: &str, cents: i64| NewMilestone {
        case_id: Uuid::now_v7(),
        position: 4,
        title: title.to_owned(),
        amount: soles(cents),
        due_on: TODAY,
    };

    let created = BillingMilestone::create(input("  Saldo ", 500)).unwrap();

    assert_eq!(created.position(), 4);
    assert_eq!(created.title(), "Saldo");
    assert_eq!(created.amount(), soles(500));
    assert_eq!(created.paid_on(), None);
    assert_eq!(
        BillingMilestone::create(input(" ", 500)),
        Err(DomainError::MissingField("concepto"))
    );
    assert_eq!(
        BillingMilestone::create(input("Saldo", 0)),
        Err(DomainError::InvalidAmount)
    );
}

#[test]
fn edits_a_milestone_and_refuses_invalid_values_without_changing_it() {
    let mut item = milestone(1, 500);

    item.rename(" Saldo final ").unwrap();
    item.set_amount(soles(700)).unwrap();
    item.reschedule(date!(2027 - 01 - 01));
    let edited = item.clone();

    assert_eq!(item.title(), "Saldo final");
    assert_eq!(item.amount(), soles(700));
    assert_eq!(item.due_on(), date!(2027 - 01 - 01));
    assert_eq!(item.rename(" "), Err(DomainError::MissingField("concepto")));
    assert_eq!(item.set_amount(soles(0)), Err(DomainError::InvalidAmount));
    assert_eq!(item, edited);
}

#[test]
fn marks_a_milestone_paid_and_unpaid_keeping_the_first_payment_date() {
    let mut item = milestone(1, 500);

    item.mark_paid(TODAY);
    item.mark_paid(date!(2026 - 12 - 01));
    assert_eq!(item.paid_on(), Some(TODAY));
    assert!(item.is_paid());

    item.mark_unpaid();
    assert_eq!(item.paid_on(), None);
    assert!(!item.is_paid());
}

#[test]
fn refuses_corrupt_milestone_records() {
    for corrupt in [
        BillingMilestoneRecord {
            amount_cents: 0,
            ..record()
        },
        BillingMilestoneRecord {
            amount_cents: -5,
            ..record()
        },
        BillingMilestoneRecord {
            title: " ".to_owned(),
            ..record()
        },
    ] {
        assert_eq!(
            BillingMilestone::restore(corrupt),
            Err(DomainError::CorruptRecord)
        );
    }
    assert_eq!(
        BillingMilestone::restore(record()).unwrap().amount(),
        soles(1_000)
    );
}

#[test]
fn summarizes_total_collected_and_pending() {
    let mut file = with_total(100_000);
    let mut paid = milestone(1, 30_000);
    paid.mark_paid(TODAY);
    file.milestones = vec![paid, milestone(2, 40_000), milestone(3, 30_000)];

    assert_eq!(
        BillingSummary::of(&file),
        BillingSummary {
            total: Some(soles(100_000)),
            collected: soles(30_000),
            pending: soles(70_000),
        }
    );
    assert_eq!(file.unpaid_total(), soles(70_000));
}

#[test]
fn pending_never_drops_below_zero() {
    let mut file = with_total(1_000);
    let mut paid = milestone(1, 1_500);
    paid.mark_paid(TODAY);
    file.milestones = vec![paid];

    let summary = BillingSummary::of(&file);

    assert_eq!(summary.collected, soles(1_500));
    assert_eq!(summary.pending, soles(0));
}

#[test]
fn without_a_total_pending_is_what_unpaid_milestones_add_up_to() {
    let mut file = case_file();
    file.milestones = vec![milestone(1, 2_000)];

    let summary = BillingSummary::of(&file);

    assert_eq!(summary.total, None);
    assert_eq!(summary.collected, soles(0));
    assert_eq!(summary.pending, soles(2_000));
}

#[test]
fn the_next_position_never_reuses_one_after_a_delete() {
    let mut file = case_file();
    assert_eq!(file.next_milestone_position(), 1);

    file.milestones = vec![milestone(1, 100), milestone(3, 100)];

    assert_eq!(file.next_milestone_position(), 4);
}
