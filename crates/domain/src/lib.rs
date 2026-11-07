//! Business rules for the COMINORSA CRM. Free of I/O and UI concerns.

mod lead;
mod prospect;
mod repository;
mod ruc;
mod settings;
mod whatsapp;

pub use lead::{DomainError, Lead, LeadRecord, NewLead, Stage};
pub use prospect::{Prospect, ProspectKind, ProspectRepository, TaxRecord};
pub use repository::{LeadRepository, RepositoryError};
pub use ruc::Ruc;
pub use settings::{Settings, SettingsRepository, ThemePreference};
pub use whatsapp::{render_message, whatsapp_number};
