//! Cases opened when a contract is signed, and their procedure steps.
//!
//! Template offsets are planning suggestions a person adjusts, never legal
//! deadlines: statutory dates change by resolution and must not live in code.

use std::future::Future;

use time::{Date, Duration};
use uuid::Uuid;

use crate::{
    DomainError, Lead, RepositoryError, Ruc, Stage,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcedureTemplate {
    pub name: &'static str,
    keywords: &'static [&'static str],
    pub steps: &'static [TemplateStep],
}

const fn step(title: &'static str, offset_days: i64) -> TemplateStep {
    TemplateStep { title, offset_days }
}

const GENERIC: ProcedureTemplate = ProcedureTemplate {
    name: "General",
    keywords: &[],
    steps: &[
        step("Reunir documentos del cliente", 3),
        step("Ejecutar el servicio", 14),
        step("Entregar el resultado al cliente", 30),
    ],
};

const TEMPLATES: [ProcedureTemplate; 5] = [
    ProcedureTemplate {
        name: "REINFO",
        keywords: &["reinfo"],
        steps: &[
            step("Reunir documentos del cliente", 3),
            step("Revisar la situación en el registro", 7),
            step("Preparar la solicitud", 14),
            step("Presentar y hacer seguimiento", 21),
        ],
    },
    ProcedureTemplate {
        name: "IGAFOM",
        keywords: &["igafom"],
        steps: &[
            step("Reunir documentos del cliente", 3),
            step("Realizar la visita de campo", 14),
            step("Elaborar el instrumento", 30),
            step("Presentar a la autoridad", 45),
            step("Hacer seguimiento de la evaluación", 60),
        ],
    },
    ProcedureTemplate {
        name: "DAC",
        keywords: &["dac"],
        steps: &[
            step("Confirmar el cronograma vigente", 2),
            step("Reunir la información del año", 7),
            step("Consolidar los datos", 14),
            step("Revisar con el cliente", 21),
            step("Presentar la declaración", 28),
        ],
    },
    ProcedureTemplate {
        name: "ESTAMIN",
        keywords: &["estamin"],
        steps: &[
            step("Reunir los datos de producción", 3),
            step("Revisar la información", 7),
            step("Presentar la declaración", 10),
        ],
    },
    ProcedureTemplate {
        name: "Plan de minado",
        keywords: &["plan", "de", "minado"],
        steps: &[
            step("Reunir documentos del cliente", 3),
            step("Levantar la información de campo", 14),
            step("Elaborar el plan", 30),
            step("Revisar con el cliente", 40),
            step("Presentar a la autoridad", 45),
        ],
    },
];

/// Picks the template named in the free-text service, by whole words so
/// "Redacción" never reads as "DAC". Unknown services get the generic one.
pub fn procedure_template(service: &str) -> ProcedureTemplate {
    let service = service.to_lowercase();
    let words: Vec<&str> = service
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();

    TEMPLATES
        .into_iter()
        .find(|template| {
            words
                .windows(template.keywords.len())
                .any(|window| window == template.keywords)
        })
        .unwrap_or(GENERIC)
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
        })
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

/// A case with its steps in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseFile {
    pub case: Case,
    pub steps: Vec<CaseStep>,
}

impl CaseFile {
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
            client_name: lead.name().to_owned(),
            ruc: lead.ruc().cloned(),
            service: lead.service().to_owned(),
            owner: lead.owner().map(str::to_owned),
            opened_on: signed_on,
            status: CaseStatus::Open,
        };
        let steps = (1..)
            .zip(template.steps)
            .map(|(position, planned)| CaseStep {
                id: Uuid::now_v7(),
                case_id: case.id,
                position,
                title: planned.title.to_owned(),
                due_on: signed_on + Duration::days(planned.offset_days),
                done_on: None,
            })
            .collect();

        Ok(Self { case, steps })
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

    /// Open cases, oldest first, each with its steps in order.
    fn open_cases(&self) -> impl Future<Output = Result<Vec<CaseFile>, RepositoryError>> + Send;

    /// Stores a step's completion and due date.
    fn save_step(
        &self,
        step: &CaseStep,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}
