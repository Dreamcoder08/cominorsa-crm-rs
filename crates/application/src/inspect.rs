//! Read-only views of the stored state, for assertions and diagnosis.

use cominorsa_domain::{
    CaseFile, CaseRepository, Lead, LeadRepository, Prospect, ProspectRepository, Quote,
    QuoteRepository, Settings, SettingsRepository,
};

use crate::error::AppError;

/// What the repository ports can see, in a stable order.
///
/// `cases` holds open cases and closed ones that still owe money. Closed cases
/// with nothing to collect and the yearly quote counters have no read port.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub leads: Vec<Lead>,
    pub cases: Vec<CaseFile>,
    pub quotes: Vec<Quote>,
    pub prospects: Vec<Prospect>,
    pub settings: Settings,
}

pub async fn snapshot<R>(repository: &R) -> Result<Snapshot, AppError>
where
    R: LeadRepository + CaseRepository + QuoteRepository + ProspectRepository + SettingsRepository,
{
    let mut prospects = repository.search("", u32::MAX).await?;
    prospects.sort_by(|left, right| {
        (left.kind.code(), &left.name).cmp(&(right.kind.code(), &right.name))
    });

    Ok(Snapshot {
        leads: repository.list().await?,
        cases: repository.active_cases().await?,
        quotes: repository.quotes().await?,
        prospects,
        settings: repository.load().await?,
    })
}

/// Counts of the stored records the ports can see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub leads: usize,
    pub active_cases: usize,
    pub case_steps: usize,
    pub billing_milestones: usize,
    pub quotes: usize,
    pub prospects: usize,
    pub team_members: usize,
}

pub async fn status<R>(repository: &R) -> Result<Status, AppError>
where
    R: LeadRepository + CaseRepository + QuoteRepository + ProspectRepository + SettingsRepository,
{
    let cases = repository.active_cases().await?;

    Ok(Status {
        leads: repository.list().await?.len(),
        active_cases: cases.len(),
        case_steps: cases.iter().map(|file| file.steps.len()).sum(),
        billing_milestones: cases.iter().map(|file| file.milestones.len()).sum(),
        quotes: repository.quotes().await?.len(),
        prospects: repository.count().await?,
        team_members: repository.load().await?.team().len(),
    })
}
