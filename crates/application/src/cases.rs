//! Case steps, the agenda, and closing a case.

use cominorsa_domain::{
    CaseClosure, CaseFile, CaseRepository, CaseStatus, CaseStep, Lead, LeadRepository,
};
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

use crate::{
    context::{Context, Ids},
    error::AppError,
    presentation::AgendaGroups,
};

pub fn parse_step_date(value: &str) -> Result<Date, AppError> {
    Date::parse(value, &Iso8601::DATE)
        .map_err(|_| AppError::invalid_date("Ingresa la fecha del paso"))
}

/// The step after its checkbox flips.
pub fn toggled_step(mut step: CaseStep, today: Date) -> CaseStep {
    if step.is_done() {
        step.mark_undone();
    } else {
        step.mark_done(today);
    }
    step
}

/// Open, or closed and still owing money, as `active_cases` selects.
pub(crate) fn is_active(file: &CaseFile) -> bool {
    file.case.status() == CaseStatus::Open || file.unpaid_total().cents() > 0
}

/// An open case, or a closed one that still owes money.
pub async fn find_case<R: CaseRepository>(repository: &R, id: Uuid) -> Result<CaseFile, AppError> {
    repository
        .active_cases()
        .await?
        .into_iter()
        .find(|file| file.case.id() == id)
        .ok_or_else(|| {
            AppError::rejected(
                "case_not_active",
                "No hay un expediente activo con ese identificador",
            )
        })
}

pub async fn find_step<R: CaseRepository>(repository: &R, id: Uuid) -> Result<CaseStep, AppError> {
    repository
        .active_cases()
        .await?
        .into_iter()
        .flat_map(|file| file.steps)
        .find(|step| step.id() == id)
        .ok_or_else(|| AppError::rejected("step_not_found", "No se encontró el paso"))
}

/// Marks a step done or pending. Returns `false`, keeping the stored date,
/// when it is already that way.
pub async fn set_step_done<R: CaseRepository>(
    repository: &R,
    id: Uuid,
    done: bool,
    today: Date,
) -> Result<bool, AppError> {
    let step = find_step(repository, id).await?;
    if step.is_done() == done {
        return Ok(false);
    }

    repository.save_step(&toggled_step(step, today)).await?;
    Ok(true)
}

pub async fn reschedule_step<R: CaseRepository>(
    repository: &R,
    id: Uuid,
    value: &str,
) -> Result<CaseStep, AppError> {
    let due_on = parse_step_date(value)?;
    let mut step = find_step(repository, id).await?;
    step.reschedule(due_on);
    repository.save_step(&step).await?;

    Ok(step)
}

/// Follow-ups, open case steps, and unpaid milestones around `today`.
pub async fn agenda<R>(repository: &R, today: Date) -> Result<AgendaGroups, AppError>
where
    R: LeadRepository + CaseRepository,
{
    let leads = repository.list().await?;
    let cases = repository.active_cases().await?;

    Ok(AgendaGroups::of(&leads, &cases, today))
}

async fn close<R>(
    repository: &R,
    case: cominorsa_domain::Case,
    today: Date,
    ids: &mut Ids,
) -> Result<CaseClosing, AppError>
where
    R: LeadRepository + CaseRepository,
{
    let leads = repository.list().await?;
    let source = leads
        .iter()
        .find(|lead| lead.id() == case.lead_id())
        .ok_or_else(|| {
            AppError::rejected("lead_not_found", "No se encontró el lead del expediente")
        })?;
    let was_open = case.status() == CaseStatus::Open;
    let mut closure = CaseClosure::of(case, source, today);
    closure.renewal = closure
        .renewal
        .map(|renewal| ids.lead(renewal))
        .transpose()?;
    if let Some(renewal) = closure.renewal.as_ref().filter(|_| was_open)
        && leads.iter().any(|lead| lead.id() == renewal.id())
    {
        return Err(AppError::seed_reused(
            "Ese identificador ya pertenece a otro lead",
        ));
    }
    let closed = repository.close_case(&closure).await?;

    Ok(CaseClosing {
        closed,
        renewal: closure.renewal.filter(|_| closed),
    })
}

/// What closing did: `closed` is false for a repeat, which changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseClosing {
    pub closed: bool,
    pub renewal: Option<Lead>,
}

/// Closes the case and creates its renewal lead in one atomic operation.
/// Returns the lead created, and `None` for a service that does not recur or a
/// repeat, which changes nothing.
pub async fn save_close<R>(
    repository: &R,
    case: cominorsa_domain::Case,
    today: Date,
) -> Result<Option<Lead>, AppError>
where
    R: LeadRepository + CaseRepository,
{
    Ok(close(repository, case, today, &mut Ids::generated())
        .await?
        .renewal)
}

/// Any stored case, including a closed one with nothing left to collect.
pub async fn find_stored_case<R: CaseRepository>(
    repository: &R,
    id: Uuid,
) -> Result<CaseFile, AppError> {
    repository
        .all_cases()
        .await?
        .into_iter()
        .find(|file| file.case.id() == id)
        .ok_or_else(|| AppError::rejected("case_not_found", "No se encontró el expediente"))
}

/// Closes a case by id. Closing it again changes nothing, even after it is
/// settled and no longer active.
pub async fn close_case<R>(
    repository: &R,
    id: Uuid,
    context: &mut Context,
) -> Result<CaseClosing, AppError>
where
    R: LeadRepository + CaseRepository,
{
    let file = find_stored_case(repository, id).await?;

    close(
        repository,
        file.case,
        context.clock.today(),
        &mut context.ids,
    )
    .await
}
