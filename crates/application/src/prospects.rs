//! Turning a prospect into a lead.

use cominorsa_domain::{LeadRepository, ProspectKind, ProspectRepository};

use crate::{
    context::Context,
    error::AppError,
    leads::{FormValues, NewLeadOutcome, add_lead},
};

pub const PROSPECT_PAGE_SIZE: u32 = 100;

/// What a person adds to a prospect, which holds no contact data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProspectConversion {
    pub name: String,
    pub kind: Option<ProspectKind>,
    pub phone: String,
    pub service: Option<String>,
    pub owner: Option<String>,
    pub follow_up_on: Option<String>,
}

/// Creates a lead from the prospect with exactly this name.
pub async fn convert_prospect<R>(
    repository: &R,
    conversion: ProspectConversion,
    context: &mut Context,
) -> Result<NewLeadOutcome, AppError>
where
    R: LeadRepository + ProspectRepository,
{
    let found = repository.search(&conversion.name, u32::MAX).await?;
    let mut matches = found.iter().filter(|prospect| {
        prospect.name == conversion.name && conversion.kind.is_none_or(|kind| prospect.kind == kind)
    });
    let Some(prospect) = matches.next() else {
        return Err(AppError::rejected(
            "prospect_not_found",
            "No se encontró ese prospecto",
        ));
    };
    if matches.next().is_some() {
        return Err(AppError::rejected(
            "prospect_ambiguous",
            "Ese nombre es de una empresa y de una persona: indica cuál",
        ));
    }

    let mut values = FormValues::from_prospect(prospect, context.clock.today());
    values.phone = conversion.phone;
    if let Some(service) = conversion.service {
        values.service = service;
    }
    if let Some(owner) = conversion.owner {
        values.owner = owner;
    }
    if let Some(follow_up_on) = conversion.follow_up_on {
        values.follow_up_on = follow_up_on;
    }

    add_lead(repository, values, context).await
}
