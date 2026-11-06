//! Business rules for the COMINORSA CRM. Free of I/O and UI concerns.

mod lead;
mod repository;
mod ruc;

pub use lead::{DomainError, Lead, LeadRecord, NewLead, Stage};
pub use repository::{LeadRepository, RepositoryError};
pub use ruc::Ruc;
