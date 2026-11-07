//! Cases opened when a contract is signed, and their procedure steps.
//!
//! Template offsets are planning suggestions a person adjusts, never legal
//! deadlines: statutory dates change by resolution and must not live in code.

use std::future::Future;

use time::{Date, Duration, Month};
use uuid::Uuid;

use crate::{
    BillingMilestone, DomainError, Lead, Money, Quote, RepositoryError, Ruc, Stage, accepted_quote,
    lead::{optional_ruc, optional_text, required},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseStatus {
    Open,
    Closed,
}

impl CaseStatus {
    /// Stable value used for persistence.
    pub fn code(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "open" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// One suggested step of a procedure, relative to the signing date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateStep {
    pub title: &'static str,
    pub offset_days: i64,
}

/// The procedure a service needs, possibly combining several templates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcedureTemplate {
    pub name: String,
    pub steps: Vec<TemplateStep>,
    /// Suggested months until the service recurs; `None` when it does not.
    pub renewal_months: Option<u8>,
}

#[derive(Clone, Copy)]
struct KnownTemplate {
    name: &'static str,
    keywords: &'static [&'static str],
    steps: &'static [TemplateStep],
    renewal_months: Option<u8>,
}

const fn step(title: &'static str, offset_days: i64) -> TemplateStep {
    TemplateStep { title, offset_days }
}

const GENERIC: KnownTemplate = KnownTemplate {
    name: "General",
    keywords: &[],
    renewal_months: None,
    steps: &[
        step("Reunir documentos del cliente", 3),
        step("Ejecutar el servicio", 14),
        step("Entregar el resultado al cliente", 30),
    ],
};

const TEMPLATES: [KnownTemplate; 5] = [
    KnownTemplate {
        name: "REINFO",
        keywords: &["reinfo"],
        renewal_months: None,
        steps: &[
            step("Reunir documentos del cliente", 3),
            step("Revisar la situación en el registro", 7),
            step("Preparar la solicitud", 14),
            step("Presentar y hacer seguimiento", 21),
        ],
    },
    KnownTemplate {
        name: "IGAFOM",
        keywords: &["igafom"],
        renewal_months: None,
        steps: &[
            step("Reunir documentos del cliente", 3),
            step("Realizar la visita de campo", 14),
            step("Elaborar el instrumento", 30),
            step("Presentar a la autoridad", 45),
            step("Hacer seguimiento de la evaluación", 60),
        ],
    },
    KnownTemplate {
        name: "DAC",
        keywords: &["dac"],
        // A planning suggestion only: the yearly dates come from a resolution.
        renewal_months: Some(12),
        steps: &[
            step("Confirmar el cronograma vigente", 2),
            step("Reunir la información del año", 7),
            step("Consolidar los datos", 14),
            step("Revisar con el cliente", 21),
            step("Presentar la declaración", 28),
        ],
    },
    KnownTemplate {
        name: "ESTAMIN",
        keywords: &["estamin"],
        renewal_months: None,
        steps: &[
            step("Reunir los datos de producción", 3),
            step("Revisar la información", 7),
            step("Presentar la declaración", 10),
        ],
    },
    KnownTemplate {
        name: "Plan de minado",
        keywords: &["plan", "de", "minado"],
        renewal_months: None,
        steps: &[
            step("Reunir documentos del cliente", 3),
            step("Levantar la información de campo", 14),
            step("Elaborar el plan", 30),
            step("Revisar con el cliente", 40),
            step("Presentar a la autoridad", 45),
        ],
    },
];

/// Combines every template named in the free-text service, in the order of
/// the text and without repeating a step title. Words match whole, so
/// "Redacción" never reads as "DAC". Unknown services get the generic one.
pub fn procedure_template(service: &str) -> ProcedureTemplate {
    let service = service.to_lowercase();
    let words: Vec<&str> = service
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();

    let mut named: Vec<(usize, KnownTemplate)> = TEMPLATES
        .into_iter()
        .filter_map(|template| {
            let position = words
                .windows(template.keywords.len())
                .position(|window| window == template.keywords)?;
            Some((position, template))
        })
        .collect();
    named.sort_by_key(|(position, _)| *position);

    let mut templates: Vec<KnownTemplate> = named.into_iter().map(|(_, known)| known).collect();
    if templates.is_empty() {
        templates.push(GENERIC);
    }

    let mut steps: Vec<TemplateStep> = Vec::new();
    for planned in templates.iter().flat_map(|template| template.steps) {
        if !steps.iter().any(|step| step.title == planned.title) {
            steps.push(*planned);
        }
    }

    ProcedureTemplate {
        name: templates
            .iter()
            .map(|template| template.name)
            .collect::<Vec<_>>()
            .join(" + "),
        steps,
        renewal_months: templates
            .iter()
            .filter_map(|template| template.renewal_months)
            .min(),
    }
}

/// Suggested follow-up date of a renewal, or `None` when the service does not
/// recur. A month too short for the day uses its last day.
pub fn renewal_date(service: &str, closed_on: Date) -> Option<Date> {
    let months = procedure_template(service).renewal_months?;
    let index = i32::from(u8::from(closed_on.month())) - 1 + i32::from(months);
    let month = Month::January.nth_next(u8::try_from(index % 12).ok()?);
    let year = closed_on.year() + index / 12;

    Date::from_calendar_date(year, month, closed_on.day().min(month.length(year))).ok()
}

/// Persisted case values, validated again when restored.
#[derive(Clone, Debug)]
pub struct CaseRecord {
    pub id: Uuid,
    pub lead_id: Uuid,
    pub client_name: String,
    pub ruc: Option<String>,
    pub service: String,
    pub owner: Option<String>,
    pub opened_on: Date,
    pub status: String,
    pub contract_total_cents: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    id: Uuid,
    lead_id: Uuid,
    client_name: String,
    ruc: Option<Ruc>,
    service: String,
    owner: Option<String>,
    opened_on: Date,
    status: CaseStatus,
    contract_total: Option<Money>,
}

impl Case {
    pub fn restore(record: CaseRecord) -> Result<Self, DomainError> {
        let corrupt = |_| DomainError::CorruptRecord;

        Ok(Self {
            id: record.id,
            lead_id: record.lead_id,
            client_name: required(&record.client_name, "cliente").map_err(corrupt)?,
            ruc: optional_ruc(record.ruc.as_deref()).map_err(corrupt)?,
            service: required(&record.service, "servicio").map_err(corrupt)?,
            owner: optional_text(record.owner.as_deref()),
            opened_on: record.opened_on,
            status: CaseStatus::from_code(&record.status).ok_or(DomainError::CorruptRecord)?,
            contract_total: record
                .contract_total_cents
                .map(Money::from_positive_cents)
                .transpose()
                .map_err(corrupt)?,
        })
    }

    /// Repeating it keeps the case closed.
    pub fn close(&mut self) {
        self.status = CaseStatus::Closed;
    }

    /// The contract total must be a positive amount.
    pub fn set_contract_total(&mut self, total: Money) -> Result<(), DomainError> {
        self.contract_total = Some(total.positive()?);
        Ok(())
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn lead_id(&self) -> Uuid {
        self.lead_id
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn ruc(&self) -> Option<&Ruc> {
        self.ruc.as_ref()
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    pub fn opened_on(&self) -> Date {
        self.opened_on
    }

    pub fn status(&self) -> CaseStatus {
        self.status
    }

    pub fn contract_total(&self) -> Option<Money> {
        self.contract_total
    }
}

/// Persisted step values, validated again when restored.
#[derive(Clone, Debug)]
pub struct CaseStepRecord {
    pub id: Uuid,
    pub case_id: Uuid,
    pub position: u32,
    pub title: String,
    pub due_on: Date,
    pub done_on: Option<Date>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseStep {
    id: Uuid,
    case_id: Uuid,
    position: u32,
    title: String,
    due_on: Date,
    done_on: Option<Date>,
}

impl CaseStep {
    pub fn restore(record: CaseStepRecord) -> Result<Self, DomainError> {
        Ok(Self {
            id: record.id,
            case_id: record.case_id,
            position: record.position,
            title: required(&record.title, "paso").map_err(|_| DomainError::CorruptRecord)?,
            due_on: record.due_on,
            done_on: record.done_on,
        })
    }

    /// Repeating it keeps the first completion date.
    pub fn mark_done(&mut self, on: Date) {
        self.done_on.get_or_insert(on);
    }

    pub fn mark_undone(&mut self) {
        self.done_on = None;
    }

    pub fn reschedule(&mut self, due_on: Date) {
        self.due_on = due_on;
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

    pub fn due_on(&self) -> Date {
        self.due_on
    }

    pub fn done_on(&self) -> Option<Date> {
        self.done_on
    }

    pub fn is_done(&self) -> bool {
        self.done_on.is_some()
    }
}

/// A case with its steps and billing milestones in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseFile {
    pub case: Case,
    pub steps: Vec<CaseStep>,
    pub milestones: Vec<BillingMilestone>,
}

impl CaseFile {
    /// Suggests the total of the lead's accepted quote as the contract total.
    /// It stays editable; without an accepted quote the total stays undefined.
    pub fn suggest_total_from(&mut self, quotes: &[Quote]) -> Result<(), DomainError> {
        match accepted_quote(quotes, self.case.lead_id) {
            Some(quote) => self.case.set_contract_total(quote.totals().total),
            None => Ok(()),
        }
    }

    /// Builds the case of a signed lead. Step dates are editable suggestions.
    pub fn open(
        lead: &Lead,
        signed_on: Date,
        template: &ProcedureTemplate,
    ) -> Result<Self, DomainError> {
        if lead.stage() != Stage::ContractSigned {
            return Err(DomainError::ContractNotSigned);
        }

        let case = Case {
            id: Uuid::now_v7(),
            lead_id: lead.id(),
            client_name: lead.client_name().to_owned(),
            ruc: lead.ruc().cloned(),
            service: lead.service().to_owned(),
            owner: lead.owner().map(str::to_owned),
            opened_on: signed_on,
            status: CaseStatus::Open,
            contract_total: None,
        };
        let steps = (1..)
            .zip(&template.steps)
            .map(|(position, planned)| CaseStep {
                id: Uuid::now_v7(),
                case_id: case.id,
                position,
                title: planned.title.to_owned(),
                due_on: signed_on + Duration::days(planned.offset_days),
                done_on: None,
            })
            .collect();

        Ok(Self {
            case,
            steps,
            milestones: Vec::new(),
        })
    }
}

/// A closed case and, when its service recurs, the renewal lead to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseClosure {
    pub case: Case,
    pub renewal: Option<Lead>,
}

impl CaseClosure {
    /// The case holds no phone, so `source` is the lead that opened it. The
    /// renewal only enters the sales queue; sending anything stays human.
    pub fn of(mut case: Case, source: &Lead, closed_on: Date) -> Self {
        let renewal = renewal_date(case.service(), closed_on)
            .map(|follow_up_on| Lead::renewal(&case, source.phone(), follow_up_on));
        case.close();

        Self { case, renewal }
    }
}

/// Case persistence port.
pub trait CaseRepository: Send + Sync {
    /// Saves the signed lead and opens its case as one atomic operation.
    /// Returns `false`, keeping the stored case, when the lead already has one.
    fn sign_and_open(
        &self,
        lead: &Lead,
        opening: &CaseFile,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;

    /// Closes the case and creates its renewal lead as one atomic operation.
    /// Returns `false`, changing nothing, when the case is already closed.
    fn close_case(
        &self,
        closure: &CaseClosure,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;

    /// Open cases, oldest first, each with its steps and milestones in order.
    fn open_cases(&self) -> impl Future<Output = Result<Vec<CaseFile>, RepositoryError>> + Send;

    /// Open cases plus closed ones with unpaid milestones, so money still owed
    /// never disappears from the app.
    fn active_cases(&self) -> impl Future<Output = Result<Vec<CaseFile>, RepositoryError>> + Send;

    /// Stores a step's completion and due date.
    fn save_step(
        &self,
        step: &CaseStep,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}
