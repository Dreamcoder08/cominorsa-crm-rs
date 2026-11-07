//! Registering leads and moving them through the pipeline.

use cominorsa_domain::{
    CaseFile, CaseRepository, Lead, LeadRepository, NewLead, Prospect, QuoteRepository,
    SettingsRepository, Stage, procedure_template,
};
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

use crate::{
    context::{Context, Ids},
    error::AppError,
    presentation::{matches_query, next_follow_up},
};

const MANUAL_REVIEW_SERVICE: &str = "REVISAR_MANUAL";

/// The lead form, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormValues {
    pub name: String,
    pub phone: String,
    pub ruc: String,
    pub service: String,
    pub owner: String,
    pub follow_up_on: String,
}

impl FormValues {
    /// Starts with a real value: an unset date input shows a greyed-out date
    /// that looks filled in but submits nothing.
    pub fn starting_on(follow_up_on: Date) -> Self {
        Self {
            follow_up_on: follow_up_on.to_string(),
            ..Self::default()
        }
    }

    /// Phone stays blank: sources carry no contact data. Only confirmed RUCs prefill.
    pub fn from_prospect(prospect: &Prospect, follow_up_on: Date) -> Self {
        let service = match prospect.potential_service.as_str() {
            MANUAL_REVIEW_SERVICE => "",
            service => service,
        };

        let ruc = prospect
            .tax
            .as_ref()
            .filter(|tax| !tax.approximate)
            .map_or_else(String::new, |tax| tax.ruc.clone());

        Self {
            name: prospect.name.clone(),
            ruc,
            service: service.to_owned(),
            ..Self::starting_on(follow_up_on)
        }
    }
}

pub fn parse_form(values: FormValues) -> Result<Lead, AppError> {
    let follow_up_on = Date::parse(&values.follow_up_on, &Iso8601::DATE)
        .map_err(|_| AppError::invalid_date("Ingresa la fecha de seguimiento"))?;

    Ok(Lead::create(NewLead {
        name: values.name,
        phone: values.phone,
        ruc: Some(values.ruc),
        service: values.service,
        owner: Some(values.owner),
        follow_up_on,
    })?)
}

/// A lead after `add_lead`. `created` is false for a repeated seeded request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewLeadOutcome {
    pub lead: Lead,
    pub created: bool,
}

fn same_request(stored: &Lead, requested: &Lead) -> bool {
    stored.name() == requested.name()
        && stored.phone() == requested.phone()
        && stored.ruc() == requested.ruc()
        && stored.service() == requested.service()
        && stored.owner() == requested.owner()
        && stored.follow_up_on() == requested.follow_up_on()
}

/// Only someone on the team can be responsible for a lead. A lead may have no
/// one responsible.
async fn require_team_owner<R: SettingsRepository>(
    repository: &R,
    owner: Option<&str>,
) -> Result<(), AppError> {
    let Some(owner) = owner else {
        return Ok(());
    };
    let settings = repository.load().await?;

    if settings.team().iter().any(|member| member == owner) {
        Ok(())
    } else {
        Err(AppError::rejected(
            "owner_not_in_team",
            format!("{owner} no está en el equipo"),
        ))
    }
}

/// Saves a new lead. A seeded id can repeat, so a repeat of the same request
/// changes nothing, and the same seed with other data is refused rather than
/// overwriting the stored lead.
pub async fn add_lead<R>(
    repository: &R,
    values: FormValues,
    context: &mut Context,
) -> Result<NewLeadOutcome, AppError>
where
    R: LeadRepository + SettingsRepository,
{
    let lead = context.ids.lead(parse_form(values)?)?;

    if context.ids.is_seeded() {
        let stored = repository.list().await?;
        if let Some(found) = stored.into_iter().find(|item| item.id() == lead.id()) {
            return if same_request(&found, &lead) {
                Ok(NewLeadOutcome {
                    lead: found,
                    created: false,
                })
            } else {
                Err(AppError::seed_reused(
                    "Ese identificador ya pertenece a otro lead",
                ))
            };
        }
    }

    require_team_owner(repository, lead.owner()).await?;
    repository.save(&lead).await?;
    Ok(NewLeadOutcome {
        lead,
        created: true,
    })
}

/// The follow-up date a new lead gets when none is given.
pub async fn default_follow_up<R: SettingsRepository>(
    repository: &R,
    today: Date,
) -> Result<String, AppError> {
    let days = repository.load().await?.follow_up_days();

    Ok(next_follow_up(today, days).to_string())
}

/// The leads a search finds, in the stored order.
pub async fn search_leads<R: LeadRepository>(
    repository: &R,
    query: &str,
) -> Result<Vec<Lead>, AppError> {
    let mut leads = repository.list().await?;
    leads.retain(|lead| matches_query(lead, query));

    Ok(leads)
}

/// The prepared chat link of a lead. A person reviews and sends the message.
pub async fn whatsapp_link<R>(repository: &R, id: Uuid) -> Result<String, AppError>
where
    R: LeadRepository + SettingsRepository,
{
    let lead = find_lead(repository, id).await?;
    let settings = repository.load().await?;

    lead.whatsapp_url(settings.whatsapp_template())
        .ok_or_else(|| {
            AppError::rejected(
                "no_whatsapp_number",
                "Ese teléfono no sirve para abrir un chat",
            )
        })
}

pub async fn find_lead<R: LeadRepository>(repository: &R, id: Uuid) -> Result<Lead, AppError> {
    repository
        .list()
        .await?
        .into_iter()
        .find(|lead| lead.id() == id)
        .ok_or_else(|| AppError::rejected("lead_not_found", "No se encontró el lead"))
}

/// Gives the lead a team member as responsible. A blank owner clears it.
pub async fn assign_lead<R>(repository: &R, id: Uuid, owner: &str) -> Result<Lead, AppError>
where
    R: LeadRepository + SettingsRepository,
{
    let mut lead = find_lead(repository, id).await?;
    lead.assign(Some(owner));
    require_team_owner(repository, lead.owner()).await?;
    repository.save(&lead).await?;

    Ok(lead)
}

/// What a pipeline move did. `case` is the case of a lead that reached the
/// final stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advance {
    pub lead: Lead,
    pub case: Option<CaseFile>,
}

async fn advance<R>(
    repository: &R,
    mut lead: Lead,
    today: Date,
    ids: &mut Ids,
) -> Result<Advance, AppError>
where
    R: LeadRepository + CaseRepository + QuoteRepository,
{
    lead.advance()?;

    if lead.stage() != Stage::ContractSigned {
        repository.save(&lead).await?;
        return Ok(Advance { lead, case: None });
    }

    let template = procedure_template(lead.service());
    let mut opening = ids.opening(CaseFile::open(&lead, today, &template)?)?;
    opening.suggest_total_from(&repository.quotes().await?)?;
    if ids.is_seeded() {
        let taken =
            repository.all_cases().await?.iter().any(|file| {
                file.case.id() == opening.case.id() && file.case.lead_id() != lead.id()
            });
        if taken {
            return Err(AppError::seed_reused(
                "Ese identificador ya pertenece a otro expediente",
            ));
        }
    }
    let case = if repository.sign_and_open(&lead, &opening).await? {
        Some(opening)
    } else {
        let stored = repository.active_cases().await?;
        stored
            .into_iter()
            .find(|file| file.case.lead_id() == lead.id())
    };

    Ok(Advance { lead, case })
}

/// Saves one pipeline move. Reaching the final stage opens the case in the same
/// atomic operation, with suggested step dates counted from `today` and the
/// total of an accepted quote as the suggested contract total.
pub async fn save_advance<R>(repository: &R, lead: Lead, today: Date) -> Result<(), AppError>
where
    R: LeadRepository + CaseRepository + QuoteRepository,
{
    advance(repository, lead, today, &mut Ids::generated())
        .await
        .map(|_| ())
}

/// Advances the stored lead one stage.
pub async fn advance_lead<R>(
    repository: &R,
    id: Uuid,
    context: &mut Context,
) -> Result<Advance, AppError>
where
    R: LeadRepository + CaseRepository + QuoteRepository,
{
    let lead = find_lead(repository, id).await?;

    advance(repository, lead, context.clock.today(), &mut context.ids).await
}
