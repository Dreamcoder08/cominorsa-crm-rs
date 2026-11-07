//! Business rules for the COMINORSA CRM. Free of I/O and UI concerns.

mod billing;
mod case;
mod consent;
mod lead;
mod prospect;
mod quote;
mod repository;
mod ruc;
mod settings;
mod whatsapp;

pub use billing::{
    BillingMilestone, BillingMilestoneRecord, BillingRepository, BillingSummary, MilestoneGap,
    Money, NewMilestone,
};
pub use case::{
    Case, CaseClosure, CaseFile, CaseRecord, CaseRepository, CaseStatus, CaseStep, CaseStepRecord,
    ProcedureTemplate, TemplateStep, procedure_template, renewal_date,
};
pub use consent::{Consent, INBOUND_CONSENT_SOURCE, LeadOrigin};
pub use lead::{DomainError, Lead, LeadRecord, NewLead, Stage};
pub use prospect::{Prospect, ProspectKind, ProspectRepository, TaxRecord};
pub use quote::{
    IgvRate, Quote, QuoteContent, QuoteDraft, QuoteLine, QuoteLineRecord, QuoteNumber, QuoteRecord,
    QuoteRepository, QuoteStatus, QuoteTotals, accepted_quote, is_superseded,
};
pub use repository::{LeadRepository, RepositoryError};
pub use ruc::Ruc;
pub use settings::{CompanyProfile, Settings, SettingsRepository, ThemePreference};
pub use whatsapp::{render_message, whatsapp_number};
