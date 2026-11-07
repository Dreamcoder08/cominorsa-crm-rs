use thiserror::Error;
use time::Date;
use uuid::Uuid;

use crate::{Case, Consent, LeadOrigin, Ruc, whatsapp};

const RENEWAL_PREFIX: &str = "Renovación: ";

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
    #[error("El expediente se abre al firmar el contrato")]
    ContractNotSigned,
    #[error("Ingresa un monto mayor que cero, por ejemplo 1,234.50")]
    InvalidAmount,
    #[error("Ingresa primero el total del contrato")]
    MissingContractTotal,
    #[error("El plan sugerido solo se aplica a un expediente sin hitos")]
    MilestonesAlreadyPlanned,
    #[error("La cotización necesita al menos una línea")]
    EmptyQuote,
    #[error("La cantidad debe ser un número entero mayor que cero")]
    InvalidQuantity,
    #[error("La validez no puede ser anterior a la fecha de emisión")]
    InvalidQuoteValidity,
    #[error("Ingresa una tasa de IGV entre 0 y 100, por ejemplo 18")]
    InvalidIgvRate,
    #[error("Una cotización enviada no se cambia: crea una nueva versión")]
    QuoteLocked,
    #[error("La cotización no admite ese cambio de estado")]
    InvalidQuoteTransition,
    #[error("El cliente no tiene un consentimiento vigente para recibir mensajes")]
    NoConsent,
    #[error("Ese teléfono no sirve para abrir un chat")]
    NoWhatsappNumber,
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
    pub renewal_of: Option<Uuid>,
    pub origin: String,
    pub consent: Option<Consent>,
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
    renewal_of: Option<Uuid>,
    origin: LeadOrigin,
    consent: Option<Consent>,
}

impl Lead {
    /// A lead with origin "other" and no consent.
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
            renewal_of: None,
            origin: LeadOrigin::Other,
            consent: None,
        })
    }

    /// A lead from a known origin. One the client started records consent
    /// dated `today`; any other origin starts without consent.
    pub fn receive(input: NewLead, origin: LeadOrigin, today: Date) -> Result<Self, DomainError> {
        let mut lead = Self::create(input)?;
        lead.origin = origin;
        lead.consent = origin.is_inbound().then(|| Consent::inbound(today));

        Ok(lead)
    }

    /// A new sales opportunity for the client of a closed case.
    /// It keeps the consent of `source`, the lead that opened the case.
    pub(crate) fn renewal(case: &Case, source: &Lead, follow_up_on: Date) -> Self {
        Self {
            id: Uuid::now_v7(),
            name: format!("{RENEWAL_PREFIX}{}", case.client_name()),
            phone: source.phone.clone(),
            ruc: case.ruc().cloned(),
            service: case.service().to_owned(),
            owner: case.owner().map(str::to_owned),
            stage: Stage::InitialContact,
            follow_up_on,
            renewal_of: Some(case.id()),
            origin: LeadOrigin::ExistingClient,
            consent: source.consent.clone(),
        }
    }

    pub fn restore(record: LeadRecord) -> Result<Self, DomainError> {
        let corrupt = |_| DomainError::CorruptRecord;
        let origin = LeadOrigin::from_code(&record.origin).ok_or(DomainError::CorruptRecord)?;
        if record
            .consent
            .as_ref()
            .is_some_and(|consent| consent.source.trim().is_empty())
        {
            return Err(DomainError::CorruptRecord);
        }

        Ok(Self {
            id: record.id,
            name: required(&record.name, "nombre").map_err(corrupt)?,
            phone: required(&record.phone, "teléfono").map_err(corrupt)?,
            ruc: optional_ruc(record.ruc.as_deref()).map_err(corrupt)?,
            service: required(&record.service, "servicio").map_err(corrupt)?,
            owner: optional_text(record.owner.as_deref()),
            stage: Stage::from_code(&record.stage).ok_or(DomainError::CorruptRecord)?,
            follow_up_on: record.follow_up_on,
            renewal_of: record.renewal_of,
            origin,
            consent: record.consent,
        })
    }

    /// Records that the client agreed to be contacted, and how.
    pub fn record_consent(&mut self, on: Date, source: &str) -> Result<(), DomainError> {
        self.consent = Some(Consent {
            given: true,
            recorded_on: on,
            source: required(source, "fuente")?,
        });
        Ok(())
    }

    /// Records that the client withdrew its consent, and how.
    pub fn revoke_consent(&mut self, on: Date, source: &str) -> Result<(), DomainError> {
        if !self.has_consent() {
            return Err(DomainError::NoConsent);
        }
        self.consent = Some(Consent {
            given: false,
            recorded_on: on,
            source: required(source, "fuente")?,
        });
        Ok(())
    }

    /// Whether the last consent decision is a given one.
    pub fn has_consent(&self) -> bool {
        self.consent.as_ref().is_some_and(|consent| consent.given)
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

    /// Prepared chat link. It needs current consent and a phone that can use
    /// WhatsApp.
    pub fn whatsapp_link(&self, template: &str) -> Result<String, DomainError> {
        if !self.has_consent() {
            return Err(DomainError::NoConsent);
        }
        let number = whatsapp::whatsapp_number(&self.phone).ok_or(DomainError::NoWhatsappNumber)?;
        let message = whatsapp::render_message(template, self.client_name(), &self.service);

        Ok(whatsapp::chat_url(&number, &message))
    }

    /// Prepared chat link, or `None` when `whatsapp_link` refuses.
    pub fn whatsapp_url(&self, template: &str) -> Option<String> {
        self.whatsapp_link(template).ok()
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name without the renewal label, for what the client reads.
    pub fn client_name(&self) -> &str {
        match self.renewal_of {
            Some(_) => self.name.strip_prefix(RENEWAL_PREFIX).unwrap_or(&self.name),
            None => &self.name,
        }
    }

    /// The closed case this lead renews; a renewal is an existing client.
    pub fn renewal_of(&self) -> Option<Uuid> {
        self.renewal_of
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

    pub fn origin(&self) -> LeadOrigin {
        self.origin
    }

    pub fn consent(&self) -> Option<&Consent> {
        self.consent.as_ref()
    }
}

pub(crate) fn required(value: &str, field: &'static str) -> Result<String, DomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DomainError::MissingField(field));
    }
    Ok(value.to_owned())
}

pub(crate) fn optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(crate) fn optional_ruc(value: Option<&str>) -> Result<Option<Ruc>, DomainError> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) => Ruc::parse(value).map(Some),
    }
}
