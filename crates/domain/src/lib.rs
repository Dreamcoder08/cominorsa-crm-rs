//! Business rules for the COMINORSA CRM. Free of I/O and UI concerns.

mod lead;
mod prospect;
mod repository;
mod ruc;

pub use lead::{DomainError, Lead, LeadRecord, NewLead, Stage};
pub use prospect::{Prospect, ProspectKind, ProspectRepository};
pub use repository::{LeadRepository, RepositoryError};
pub use ruc::Ruc;
