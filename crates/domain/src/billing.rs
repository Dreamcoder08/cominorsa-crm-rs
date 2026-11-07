//! Billing milestones of a case. They track collections and never issue tax
//! invoices. Amounts are integer cents; money never touches a float.

use std::future::Future;

use time::{Date, Duration};
use uuid::Uuid;

use crate::{CaseFile, DomainError, RepositoryError, lead::required};

const CENTS_PER_SOL: i64 = 100;
/// One hundred billion soles: far above any contract, and sums stay in `i64`.
const MAX_CENTS: i64 = 10_000_000_000_000;

/// The 30, 40, 30 split comes from an unvalidated example, so the plan is an
/// editable suggestion: title, percent, and days after the start date.
const SUGGESTED_PLAN: [(&str, i64, i64); 3] = [
    ("Adelanto", 30, 0),
    ("Avance", 40, 30),
    ("Saldo final", 30, 60),
];

/// A non-negative amount of soles, in integer cents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money(i64);

impl Money {
    pub fn from_cents(cents: i64) -> Result<Self, DomainError> {
        if (0..=MAX_CENTS).contains(&cents) {
            Ok(Self(cents))
        } else {
            Err(DomainError::InvalidAmount)
        }
    }

    pub(crate) fn from_positive_cents(cents: i64) -> Result<Self, DomainError> {
        Self::from_cents(cents)?.positive()
    }

    pub(crate) fn positive(self) -> Result<Self, DomainError> {
        if self.0 > 0 {
            Ok(self)
        } else {
            Err(DomainError::InvalidAmount)
        }
    }

    /// Reads a typed amount such as `1234.5`, `1,234.50`, or `S/ 1,234.50`.
    /// A comma only groups thousands, so `1234,50` is refused, never misread.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        let text = text.trim();
        let text = text.strip_prefix("S/").unwrap_or(text).trim_start();
        let (whole, decimals) = match text.split_once('.') {
            Some((_, "")) => return Err(DomainError::InvalidAmount),
            Some(parts) => parts,
            None => (text, ""),
        };

        let soles = ungrouped_digits(whole)
            .ok_or(DomainError::InvalidAmount)?
            .parse::<i64>()
            .map_err(|_| DomainError::InvalidAmount)?;
        let cents = decimal_cents(decimals).ok_or(DomainError::InvalidAmount)?;
        let total = soles
            .checked_mul(CENTS_PER_SOL)
            .and_then(|whole| whole.checked_add(cents))
            .ok_or(DomainError::InvalidAmount)?;

        Self::from_positive_cents(total)
    }

    pub fn cents(self) -> i64 {
        self.0
    }

    fn saturating_sub(self, other: Self) -> Self {
        Self((self.0 - other.0).max(0))
    }

    fn sum(amounts: impl Iterator<Item = Self>) -> Self {
        Self(amounts.map(Self::cents).sum())
    }
}

/// Removes thousands separators, refusing a badly grouped number.
fn ungrouped_digits(whole: &str) -> Option<String> {
    let mut groups = whole.split(',');
    let first = groups.next()?;
    let later: Vec<&str> = groups.collect();
    let is_digits =
        |group: &str| !group.is_empty() && group.bytes().all(|byte: u8| byte.is_ascii_digit());

    let well_grouped =
        later.is_empty() || (first.len() <= 3 && later.iter().all(|group| group.len() == 3));
    (well_grouped && is_digits(first) && later.iter().all(|group| is_digits(group)))
        .then(|| [first].into_iter().chain(later).collect())
}

/// Up to two digits after the point, as cents: "5" is 50 and "05" is 5.
fn decimal_cents(decimals: &str) -> Option<i64> {
    if decimals.len() > 2 || !decimals.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    format!("{decimals:0<2}").parse().ok()
}

/// Unvalidated input for a new milestone.
#[derive(Clone, Debug)]
pub struct NewMilestone {
    pub case_id: Uuid,
    pub position: u32,
    pub title: String,
    pub amount: Money,
    pub due_on: Date,
}

/// Persisted milestone values, validated again when restored.
#[derive(Clone, Debug)]
pub struct BillingMilestoneRecord {
    pub id: Uuid,
    pub case_id: Uuid,
    pub position: u32,
    pub title: String,
    pub amount_cents: i64,
    pub due_on: Date,
    pub paid_on: Option<Date>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BillingMilestone {
    id: Uuid,
    case_id: Uuid,
    position: u32,
    title: String,
    amount: Money,
    due_on: Date,
    paid_on: Option<Date>,
}

impl BillingMilestone {
    pub fn create(input: NewMilestone) -> Result<Self, DomainError> {
        Ok(Self {
            id: Uuid::now_v7(),
            case_id: input.case_id,
            position: input.position,
            title: required(&input.title, "concepto")?,
            amount: input.amount.positive()?,
            due_on: input.due_on,
            paid_on: None,
        })
    }

    pub fn restore(record: BillingMilestoneRecord) -> Result<Self, DomainError> {
        let corrupt = |_| DomainError::CorruptRecord;

        Ok(Self {
            id: record.id,
            case_id: record.case_id,
            position: record.position,
            title: required(&record.title, "concepto").map_err(corrupt)?,
            amount: Money::from_positive_cents(record.amount_cents).map_err(corrupt)?,
            due_on: record.due_on,
            paid_on: record.paid_on,
        })
    }

    pub fn rename(&mut self, title: &str) -> Result<(), DomainError> {
        self.title = required(title, "concepto")?;
        Ok(())
    }

    pub fn set_amount(&mut self, amount: Money) -> Result<(), DomainError> {
        self.amount = amount.positive()?;
        Ok(())
    }

    pub fn reschedule(&mut self, due_on: Date) {
        self.due_on = due_on;
    }

    /// Repeating it keeps the first payment date.
    pub fn mark_paid(&mut self, on: Date) {
        self.paid_on.get_or_insert(on);
    }

    pub fn mark_unpaid(&mut self) {
        self.paid_on = None;
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn case_id(&self) -> Uuid {
        self.case_id
    }

    pub fn position(&self) -> u32 {
        self.position
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn amount(&self) -> Money {
        self.amount
    }

    pub fn due_on(&self) -> Date {
        self.due_on
    }

    pub fn paid_on(&self) -> Option<Date> {
        self.paid_on
    }

    pub fn is_paid(&self) -> bool {
        self.paid_on.is_some()
    }
}

impl CaseFile {
    /// The suggested plan for a case without milestones. Portions round down
    /// and the last one takes the remainder, so the plan sums to the total;
    /// a portion that rounds to zero is left out.
    pub fn suggested_milestones(&self, from: Date) -> Result<Vec<BillingMilestone>, DomainError> {
        let total = self
            .case
            .contract_total()
            .ok_or(DomainError::MissingContractTotal)?;
        if !self.milestones.is_empty() {
            return Err(DomainError::MilestonesAlreadyPlanned);
        }

        let first = total.cents() * SUGGESTED_PLAN[0].1 / 100;
        let second = total.cents() * SUGGESTED_PLAN[1].1 / 100;
        let portions = [first, second, total.cents() - first - second];

        (1..)
            .zip(
                SUGGESTED_PLAN
                    .iter()
                    .zip(portions)
                    .filter(|(_, cents)| *cents > 0),
            )
            .map(|(position, ((title, _, days), cents))| {
                BillingMilestone::create(NewMilestone {
                    case_id: self.case.id(),
                    position,
                    title: (*title).to_owned(),
                    amount: Money::from_cents(cents)?,
                    due_on: from + Duration::days(*days),
                })
            })
            .collect()
    }

    /// Positions keep growing after a delete, so they stay unique.
    pub fn next_milestone_position(&self) -> u32 {
        self.milestones
            .iter()
            .map(BillingMilestone::position)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// What unpaid milestones add up to.
    pub fn unpaid_total(&self) -> Money {
        Money::sum(
            self.milestones
                .iter()
                .filter(|milestone| !milestone.is_paid())
                .map(BillingMilestone::amount),
        )
    }
}

/// How much of a case has been collected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BillingSummary {
    pub total: Option<Money>,
    pub collected: Money,
    pub pending: Money,
}

impl BillingSummary {
    /// Pending is what the contract still owes, never below zero. Without a
    /// contract total it is what the unpaid milestones add up to.
    pub fn of(file: &CaseFile) -> Self {
        let total = file.case.contract_total();
        let collected = Money::sum(
            file.milestones
                .iter()
                .filter(|milestone| milestone.is_paid())
                .map(BillingMilestone::amount),
        );
        let pending = total.map_or_else(
            || file.unpaid_total(),
            |total| total.saturating_sub(collected),
        );

        Self {
            total,
            collected,
            pending,
        }
    }
}

/// Billing persistence port.
pub trait BillingRepository: Send + Sync {
    fn save_contract_total(
        &self,
        case_id: Uuid,
        total: Money,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Adds every milestone or none.
    fn add_milestones(
        &self,
        milestones: &[BillingMilestone],
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Stores a milestone's title, amount, due date, and payment.
    fn save_milestone(
        &self,
        milestone: &BillingMilestone,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    fn delete_milestone(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}
