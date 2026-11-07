//! Billing milestones of a case.

use cominorsa_domain::{
    BillingMilestone, BillingRepository, CaseFile, CaseRepository, Money, NewMilestone,
};
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

use crate::{context::Context, error::AppError};

/// The new-milestone form, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilestoneValues {
    pub title: String,
    pub amount: String,
    pub due_on: String,
}

impl MilestoneValues {
    pub fn starting_on(due_on: Date) -> Self {
        Self {
            due_on: due_on.to_string(),
            ..Self::default()
        }
    }
}

/// What a person did in the billing panel, before validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillingEdit {
    Total(String),
    Suggest,
    Add,
    Title(BillingMilestone, String),
    Amount(BillingMilestone, String),
    DueDate(BillingMilestone, String),
    TogglePaid(BillingMilestone),
    Delete(Uuid),
}

/// A validated billing change, ready to persist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillingChange {
    Total(Uuid, Money),
    Add(Vec<BillingMilestone>),
    Save(BillingMilestone),
    Delete(Uuid),
}

/// Validates one edit of the billing panel through the domain rules.
pub fn billing_change(
    file: &CaseFile,
    edit: BillingEdit,
    draft: &MilestoneValues,
    today: Date,
) -> Result<BillingChange, AppError> {
    match edit {
        BillingEdit::Total(text) => Ok(BillingChange::Total(file.case.id(), Money::parse(&text)?)),
        BillingEdit::Suggest => Ok(BillingChange::Add(file.suggested_milestones(today)?)),
        BillingEdit::Add => {
            let amount = Money::parse(&draft.amount)?;
            let milestone = BillingMilestone::create(NewMilestone {
                case_id: file.case.id(),
                position: file.next_milestone_position(),
                title: draft.title.clone(),
                amount,
                due_on: parse_milestone_date(&draft.due_on)?,
            })?;

            Ok(BillingChange::Add(vec![milestone]))
        }
        BillingEdit::Title(mut milestone, title) => {
            milestone.rename(&title)?;
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::Amount(mut milestone, text) => {
            milestone.set_amount(Money::parse(&text)?)?;
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::DueDate(mut milestone, value) => {
            milestone.reschedule(parse_milestone_date(&value)?);
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::TogglePaid(mut milestone) => {
            if milestone.is_paid() {
                milestone.mark_unpaid();
            } else {
                milestone.mark_paid(today);
            }
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::Delete(id) => Ok(BillingChange::Delete(id)),
    }
}

pub fn parse_milestone_date(value: &str) -> Result<Date, AppError> {
    Date::parse(value, &Iso8601::DATE)
        .map_err(|_| AppError::invalid_date("Ingresa la fecha del hito"))
}

pub async fn save_billing<R: BillingRepository>(
    repository: &R,
    change: BillingChange,
) -> Result<(), AppError> {
    match change {
        BillingChange::Total(case_id, total) => {
            repository.save_contract_total(case_id, total).await
        }
        BillingChange::Add(milestones) => repository.add_milestones(&milestones).await,
        BillingChange::Save(milestone) => repository.save_milestone(&milestone).await,
        BillingChange::Delete(id) => repository.delete_milestone(id).await,
    }
    .map_err(AppError::from)
}

/// Whether seeded milestones already exist. The same request repeats and
/// changes nothing; the same id with other data is refused.
fn is_repeat(milestones: &[BillingMilestone], cases: &[CaseFile]) -> Result<bool, AppError> {
    let stored = || cases.iter().flat_map(|file| &file.milestones);
    let mut repeated = false;

    for new in milestones {
        let Some(old) = stored().find(|old| old.id() == new.id()) else {
            continue;
        };
        let same = old.case_id() == new.case_id()
            && old.title() == new.title()
            && old.amount() == new.amount()
            && old.due_on() == new.due_on();
        if !same {
            return Err(AppError::seed_reused(
                "Ese identificador ya pertenece a otro hito",
            ));
        }
        repeated = true;
    }

    Ok(repeated)
}

/// Validates and saves one billing edit. Returns `false` when a seeded
/// request repeats, which changes nothing.
pub async fn apply_billing<R>(
    repository: &R,
    file: &CaseFile,
    edit: BillingEdit,
    draft: &MilestoneValues,
    context: &mut Context,
) -> Result<bool, AppError>
where
    R: BillingRepository + CaseRepository,
{
    let change = match billing_change(file, edit, draft, context.clock.today())? {
        BillingChange::Add(milestones) => {
            let milestones = context.ids.milestones(milestones)?;
            if context.ids.is_seeded() && is_repeat(&milestones, &repository.all_cases().await?)? {
                return Ok(false);
            }
            BillingChange::Add(milestones)
        }
        change => change,
    };

    save_billing(repository, change).await?;
    Ok(true)
}

pub async fn find_milestone<R: CaseRepository>(
    repository: &R,
    id: Uuid,
) -> Result<(CaseFile, BillingMilestone), AppError> {
    for file in repository.active_cases().await? {
        if let Some(milestone) = file.milestones.iter().find(|item| item.id() == id) {
            let milestone = milestone.clone();
            return Ok((file, milestone));
        }
    }

    Err(AppError::rejected(
        "milestone_not_found",
        "No se encontró el hito",
    ))
}
