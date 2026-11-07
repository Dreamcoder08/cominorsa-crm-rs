use std::future::Future;

use crate::RepositoryError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProspectKind {
    Company,
    Person,
}

impl ProspectKind {
    /// Stable value used for persistence.
    pub fn code(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Person => "person",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "company" => Some(Self::Company),
            "person" => Some(Self::Person),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Company => "Empresa",
            Self::Person => "Persona",
        }
    }
}

/// Public taxpayer data from the SUNAT roll. It holds no phone or email.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxRecord {
    pub ruc: String,
    pub status: String,
    pub condition: String,
    pub address: String,
    /// The name matched without its legal form, so a person must confirm it.
    pub approximate: bool,
}

/// A concession titleholder who may become a lead.
///
/// Sources carry no contact data, so a prospect has no phone or RUC. Its
/// potential service is an inference, not a verified fact.
#[derive(Clone, Debug, PartialEq)]
pub struct Prospect {
    pub name: String,
    pub kind: ProspectKind,
    pub profile: String,
    pub concessions: u32,
    pub hectares: f64,
    pub departments: String,
    pub provinces: String,
    pub substances: String,
    pub concession_states: String,
    pub potential_service: String,
    pub needs_review: bool,
    pub source: String,
    pub tax: Option<TaxRecord>,
}

/// Prospect persistence port.
pub trait ProspectRepository: Send + Sync {
    /// Adds unknown prospects atomically and returns how many were added.
    fn import(
        &self,
        prospects: &[Prospect],
    ) -> impl Future<Output = Result<usize, RepositoryError>> + Send;

    /// Updates company prospects by name atomically and returns how many changed.
    fn apply_tax_records(
        &self,
        records: &[(String, TaxRecord)],
    ) -> impl Future<Output = Result<usize, RepositoryError>> + Send;

    fn count(&self) -> impl Future<Output = Result<usize, RepositoryError>> + Send;

    fn search(
        &self,
        query: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Prospect>, RepositoryError>> + Send;
}
