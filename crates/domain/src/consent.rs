//! Where a lead came from and whether it agreed to be contacted. Legal review
//! is pending: these rules record what happened, they do not decide the law.

use time::Date;

/// What a lead that started the contact itself records as consent.
pub const INBOUND_CONSENT_SOURCE: &str = "El cliente inició el contacto";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeadOrigin {
    WebForm,
    InboundWhatsapp,
    InboundCall,
    Referral,
    Event,
    ExistingClient,
    Prospect,
    #[default]
    Other,
}

impl LeadOrigin {
    pub const ALL: [Self; 8] = [
        Self::WebForm,
        Self::InboundWhatsapp,
        Self::InboundCall,
        Self::Referral,
        Self::Event,
        Self::ExistingClient,
        Self::Prospect,
        Self::Other,
    ];

    /// Stable value used for persistence.
    pub fn code(self) -> &'static str {
        match self {
            Self::WebForm => "web_form",
            Self::InboundWhatsapp => "inbound_whatsapp",
            Self::InboundCall => "inbound_call",
            Self::Referral => "referral",
            Self::Event => "event",
            Self::ExistingClient => "existing_client",
            Self::Prospect => "prospect",
            Self::Other => "other",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|origin| origin.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WebForm => "Formulario web",
            Self::InboundWhatsapp => "WhatsApp entrante",
            Self::InboundCall => "Llamada entrante",
            Self::Referral => "Referido",
            Self::Event => "Evento",
            Self::ExistingClient => "Cliente existente",
            Self::Prospect => "Prospecto",
            Self::Other => "Otro",
        }
    }

    /// The client started the contact, so the lead begins with consent.
    pub fn is_inbound(self) -> bool {
        matches!(
            self,
            Self::WebForm | Self::InboundWhatsapp | Self::InboundCall
        )
    }
}

/// The last consent decision: given, or revoked, with its date and source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Consent {
    pub given: bool,
    pub recorded_on: Date,
    pub source: String,
}

impl Consent {
    pub(crate) fn inbound(on: Date) -> Self {
        Self {
            given: true,
            recorded_on: on,
            source: INBOUND_CONSENT_SOURCE.to_owned(),
        }
    }
}
