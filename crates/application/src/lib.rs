//! Use cases over the domain and repository ports, free of UI types.
//!
//! The desktop app and the `cominorsa` CLI both call this crate, so what a
//! test proves through the CLI is what a person runs.

mod backup;
mod billing;
mod cases;
mod context;
mod error;
mod inspect;
mod leads;
mod presentation;
mod prospects;
mod quote_pdf;
mod quotes;
mod settings;

pub use backup::{
    AutoBackup, AutoBackupStatus, Restored, auto_backup, create_backup, is_backup_file_name,
    restore_backup, run_backup,
};
pub use billing::{
    BillingChange, BillingEdit, MilestoneFields, MilestoneValues, apply_billing, billing_change,
    edit_milestone, find_milestone, parse_milestone_date, save_billing, set_paid,
};
pub use cases::{
    CaseClosing, agenda, close_case, find_case, find_step, find_stored_case, parse_step_date,
    reschedule_step, save_close, set_step_done, toggled_step,
};
pub use context::{Clock, Context, Ids, today_in_peru};
pub use error::{AppError, ErrorKind};
pub use inspect::{
    Migration, Snapshot, Status, create_database, migrate_database, open_at_startup,
    open_for_reading, open_for_writing, snapshot, status,
};
pub use leads::{
    Advance, FormValues, NewLeadOutcome, add_lead, advance_lead, assign_lead, default_follow_up,
    find_lead, parse_form, parse_origin, record_consent, revoke_consent, save_advance,
    search_leads, whatsapp_link,
};
pub use presentation::{
    AgendaGroups, AgendaItem, FollowUp, PipelineSummary, QUOTE_TAX_NOTE, accepted_notice,
    case_progress, close_warning, closed_case_label, closing_notice, company_lines,
    company_warning, format_date, format_igv_rate, format_soles, format_timestamp, igv_input,
    igv_label, matches_query, milestone_gap_warning, milestone_status_label, money_input,
    next_follow_up, owner_options, profile_label, quote_file_name, renewal_note, responsible_label,
    step_status_label, suggested_total_note,
};
pub use prospects::{
    PROSPECT_PAGE_SIZE, ProspectConversion, convert_prospect, parse_prospect_kind,
};
pub use quote_pdf::{quote_pdf, write_pdf};
pub use quotes::{
    PdfTarget, QuoteChange, QuoteEdit, QuoteFields, QuoteLineValues, QuoteValues, SavedQuote,
    create_quote, edit_quote, find_quote, lead_quotes, preview_quote, quote_change, quote_notice,
    quote_preview, save_quote, save_quote_edit, save_quote_pdf, send_quote,
};
pub use settings::{SettingsChange, preview_message, update_settings, whatsapp_preview};
