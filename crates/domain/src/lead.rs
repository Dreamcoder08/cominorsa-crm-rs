use thiserror::Error;
use time::Date;
use uuid::Uuid;

use crate::{Ruc, whatsapp};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("El campo {0} es obligatorio")]
    MissingField(&'static str),
    #[error("El RUC no es válido")]
    InvalidRuc,
    #[error("El lead ya está en la etapa final")]
    FinalStage,
    #[error("El registro guardado no es válido")]
    CorruptRecord,
    #[error("Esa persona ya está en el equipo")]
    DuplicateMember,
    #[error("El plazo debe estar entre 1 y 90 días")]
    InvalidFollowUpDays,
    #[error("Solo puedes usar {{nombre}} y {{servicio}}")]
    UnknownPlaceholder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    InitialContact,
    CoordinateEvaluation,
    QuoteSent,
    ContractSigned,
}

impl Stage {
    pub const ALL: [Self; 4] = [
        Self::InitialContact,
        Self::CoordinateEvaluation,
        Self::QuoteSent,
        Self::ContractSigned,
    ];

    /// Stable value used for persistence.
    pub fn code(self) -> &'static str {
        match self {
            Self::InitialContact => "initial_contact",
            Self::CoordinateEvaluation => "coordinate_evaluation",
            Self::QuoteSent => "quote_sent",
            Self::ContractSigned => "contract_signed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::InitialContact => "Contacto inicial",
            Self::CoordinateEvaluation => "Evaluación de coordenadas",
            Self::QuoteSent => "Cotización enviada",
            Self::ContractSigned => "Contrato firmado",
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::InitialContact => Some(Self::CoordinateEvaluation),
            Self::CoordinateEvaluation => Some(Self::QuoteSent),
            Self::QuoteSent => Some(Self::ContractSigned),
            Self::ContractSigned => None,
        }
    }
}

/// Unvalidated input for a new lead.
#[derive(Clone, Debug)]
pub struct NewLead {
    pub name: String,
    pub phone: String,
    pub ruc: Option<String>,
    pub service: String,
    pub owner: Option<String>,
    pub follow_up_on: Date,
}

/// Persisted lead values, validated again when restored.
#[derive(Clone, Debug)]
pub struct LeadRecord {
    pub id: Uuid,
    pub name: String,
    pub phone: String,
    pub ruc: Option<String>,
    pub service: String,
    pub owner: Option<String>,
    pub stage: String,
    pub follow_up_on: Date,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lead {
    id: Uuid,
    name: String,
    phone: String,
    ruc: Option<Ruc>,
    service: String,
    owner: Option<String>,
    stage: Stage,
    follow_up_on: Date,
}

impl Lead {
    pub fn create(input: NewLead) -> Result<Self, DomainError> {
        Ok(Self {
            id: Uuid::now_v7(),
            name: required(&input.name, "nombre")?,
            phone: required(&input.phone, "teléfono")?,
            ruc: optional_ruc(input.ruc.as_deref())?,
            service: required(&input.service, "servicio")?,
            owner: optional_text(input.owner.as_deref()),
            stage: Stage::InitialContact,
            follow_up_on: input.follow_up_on,
        })
    }

    pub fn restore(record: LeadRecord) -> Result<Self, DomainError> {
        let corrupt = |_| DomainError::CorruptRecord;

        Ok(Self {
            id: record.id,
            name: required(&record.name, "nombre").map_err(corrupt)?,
            phone: required(&record.phone, "teléfono").map_err(corrupt)?,
            ruc: optional_ruc(record.ruc.as_deref()).map_err(corrupt)?,
            service: required(&record.service, "servicio").map_err(corrupt)?,
            owner: optional_text(record.owner.as_deref()),
            stage: Stage::from_code(&record.stage).ok_or(DomainError::CorruptRecord)?,
            follow_up_on: record.follow_up_on,
        })
    }

    /// Blank clears the responsible person.
    pub fn assign(&mut self, owner: Option<&str>) {
        self.owner = optional_text(owner);
    }

    pub fn advance(&mut self) -> Result<(), DomainError> {
        self.stage = self.stage.next().ok_or(DomainError::FinalStage)?;
        Ok(())
    }

    /// Signed contracts leave the sales follow-up queue.
    pub fn is_follow_up_due(&self, today: Date) -> bool {
        self.stage != Stage::ContractSigned && self.follow_up_on <= today
    }

    /// Prepared chat link, or `None` when the phone cannot use WhatsApp.
    pub fn whatsapp_url(&self, template: &str) -> Option<String> {
        let number = whatsapp::whatsapp_number(&self.phone)?;
        let message = whatsapp::render_message(template, &self.name, &self.service);

        Some(whatsapp::chat_url(&number, &message))
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn phone(&self) -> &str {
        &self.phone
    }

    pub fn ruc(&self) -> Option<&Ruc> {
        self.ruc.as_ref()
    }

    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn follow_up_on(&self) -> Date {
        self.follow_up_on
    }
}

fn required(value: &str, field: &'static str) -> Result<String, DomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DomainError::MissingField(field));
    }
    Ok(value.to_owned())
}

fn optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn optional_ruc(value: Option<&str>) -> Result<Option<Ruc>, DomainError> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) => Ruc::parse(value).map(Some),
    }
}
