//! The quote form and its changes, validated through the domain.

use std::path::{Path, PathBuf};

use cominorsa_domain::{
    CaseRepository, DomainError, IgvRate, Lead, Quote, QuoteContent, QuoteDraft, QuoteLine,
    QuoteRepository, QuoteStatus, QuoteTotals, Settings, SettingsRepository, is_superseded,
};
use time::{Date, Duration, format_description::well_known::Iso8601};
use uuid::Uuid;

use crate::{
    error::AppError,
    presentation::{accepted_notice, money_input},
    quote_pdf::{quote_pdf, write_pdf},
};

/// An editable suggestion for a new quote, pending client validation.
const SUGGESTED_VALIDITY_DAYS: i64 = 15;

/// One line of the form, as typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteLineValues {
    pub description: String,
    pub quantity: String,
    pub unit_price: String,
}

impl Default for QuoteLineValues {
    fn default() -> Self {
        Self {
            description: String::new(),
            quantity: "1".to_owned(),
            unit_price: String::new(),
        }
    }
}

/// The quote form, as typed. `editing` names the draft being changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuoteValues {
    pub editing: Option<Uuid>,
    pub issued_on: String,
    pub valid_until: String,
    pub notes: String,
    pub lines: Vec<QuoteLineValues>,
}

impl QuoteValues {
    pub fn starting_on(today: Date) -> Self {
        Self {
            editing: None,
            issued_on: today.to_string(),
            valid_until: (today + Duration::days(SUGGESTED_VALIDITY_DAYS)).to_string(),
            notes: String::new(),
            lines: vec![QuoteLineValues::default()],
        }
    }

    pub fn of_quote(quote: &Quote) -> Self {
        Self {
            editing: Some(quote.id()),
            issued_on: quote.issued_on().to_string(),
            valid_until: quote.valid_until().to_string(),
            notes: quote.notes().to_owned(),
            lines: quote
                .lines()
                .iter()
                .map(|line| QuoteLineValues {
                    description: line.description().to_owned(),
                    quantity: line.quantity().to_string(),
                    unit_price: money_input(line.unit_price()),
                })
                .collect(),
        }
    }

    /// Leaves the draft being edited and starts a new quote on the same dates.
    pub fn clear(&mut self) {
        self.editing = None;
        self.notes.clear();
        self.lines = vec![QuoteLineValues::default()];
    }

    pub fn add_line(&mut self) {
        self.lines.push(QuoteLineValues::default());
    }

    /// The form always keeps one line to type in.
    pub fn remove_line(&mut self, index: usize) {
        if self.lines.len() > 1 && index < self.lines.len() {
            self.lines.remove(index);
        }
    }

    fn parsed_lines(&self) -> Result<Vec<QuoteLine>, DomainError> {
        self.lines
            .iter()
            .map(|line| QuoteLine::parse(&line.description, &line.quantity, &line.unit_price))
            .collect()
    }

    fn content(&self) -> Result<QuoteContent, AppError> {
        let date = |value: &str, missing: &str| {
            Date::parse(value, &Iso8601::DATE).map_err(|_| AppError::invalid_date(missing))
        };

        Ok(QuoteContent {
            issued_on: date(&self.issued_on, "Ingresa la fecha de emisión")?,
            valid_until: date(&self.valid_until, "Ingresa la fecha de validez")?,
            lines: self.parsed_lines()?,
            notes: self.notes.clone(),
        })
    }
}

/// What a person asked for in the quote panel, before validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuoteEdit {
    Save,
    Send(Quote),
    Accept(Quote),
    Reject(Quote),
    NewVersion(Quote),
}

/// A validated quote change, ready to persist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuoteChange {
    Create(QuoteDraft),
    Draft(Quote),
    Status(Quote, QuoteStatus),
}

/// The totals of the form, once every line can be read.
pub fn quote_preview(values: &QuoteValues, igv_rate: IgvRate) -> Option<QuoteTotals> {
    let lines = values.parsed_lines().ok()?;

    QuoteTotals::of(&lines, igv_rate).ok()
}

/// A decision on an old version would compete with the newer one.
fn current(quote: Quote, quotes: &[Quote]) -> Result<Quote, AppError> {
    if is_superseded(&quote, quotes) {
        Err(AppError::rejected(
            "quote_superseded",
            "Esta versión ya fue reemplazada por otra más reciente",
        ))
    } else {
        Ok(quote)
    }
}

/// Validates one action of the quote panel through the domain rules.
pub fn quote_change(
    lead: &Lead,
    edit: QuoteEdit,
    values: &QuoteValues,
    quotes: &[Quote],
    settings: &Settings,
    today: Date,
) -> Result<QuoteChange, AppError> {
    let igv_rate = settings.igv_rate();

    match edit {
        QuoteEdit::Save => {
            let content = values.content()?;
            let Some(id) = values.editing else {
                return Ok(QuoteChange::Create(QuoteDraft::for_lead(
                    lead, content, igv_rate,
                )?));
            };
            let mut quote = quotes
                .iter()
                .find(|quote| quote.id() == id)
                .cloned()
                .ok_or_else(|| {
                    AppError::rejected("quote_not_found", "La cotización ya no existe")
                })?;
            quote.revise(content, igv_rate)?;
            Ok(QuoteChange::Draft(quote))
        }
        QuoteEdit::Send(mut quote) => {
            quote.mark_sent(settings.company())?;
            Ok(QuoteChange::Status(quote, QuoteStatus::Draft))
        }
        QuoteEdit::Accept(quote) => {
            let mut quote = current(quote, quotes)?;
            quote.mark_accepted()?;
            Ok(QuoteChange::Status(quote, QuoteStatus::Sent))
        }
        QuoteEdit::Reject(quote) => {
            let mut quote = current(quote, quotes)?;
            quote.mark_rejected()?;
            Ok(QuoteChange::Status(quote, QuoteStatus::Sent))
        }
        QuoteEdit::NewVersion(quote) => {
            Ok(QuoteChange::Create(quote.new_version(today, igv_rate)?))
        }
    }
}

pub async fn save_quote<R: QuoteRepository>(
    repository: &R,
    change: QuoteChange,
) -> Result<Quote, AppError> {
    match change {
        QuoteChange::Create(draft) => Ok(repository.create_quote(&draft).await?),
        QuoteChange::Draft(quote) => {
            repository.save_quote_draft(&quote).await?;
            Ok(quote)
        }
        QuoteChange::Status(quote, from) => {
            repository.save_quote_status(&quote, from).await?;
            Ok(quote)
        }
    }
}

/// A saved quote change and the words that describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedQuote {
    pub quote: Quote,
    pub notice: String,
}

/// Runs one action of the quote panel against the stored quotes and settings.
pub async fn save_quote_edit<R>(
    repository: &R,
    lead: &Lead,
    edit: QuoteEdit,
    values: &QuoteValues,
    today: Date,
) -> Result<SavedQuote, AppError>
where
    R: QuoteRepository + SettingsRepository + CaseRepository,
{
    let quotes = repository.quotes().await?;
    let settings = repository.load().await?;
    let change = quote_change(lead, edit.clone(), values, &quotes, &settings, today)?;
    let case_is_open = repository
        .active_cases()
        .await?
        .iter()
        .any(|file| file.case.lead_id() == lead.id());
    let quote = save_quote(repository, change).await?;
    let notice = quote_notice(&edit, &quote, case_is_open);

    Ok(SavedQuote { quote, notice })
}

/// Marks a quote sent. The panel asks before sending without a company name,
/// so here that choice has to be explicit too.
pub async fn send_quote<R>(
    repository: &R,
    lead: &Lead,
    quote: Quote,
    allow_blank_company: bool,
    today: Date,
) -> Result<SavedQuote, AppError>
where
    R: QuoteRepository + SettingsRepository + CaseRepository,
{
    let settings = repository.load().await?;
    if !settings.company().has_name() && !allow_blank_company {
        return Err(AppError::rejected(
            "company_name_missing",
            "Falta el nombre de la empresa. Agrégalo en Configuración antes de emitir.",
        ));
    }

    save_quote_edit(
        repository,
        lead,
        QuoteEdit::Send(quote),
        &QuoteValues::default(),
        today,
    )
    .await
}

/// Finds a quote by its number, such as `COT-2026-0001-V2`, or by its id.
pub async fn find_quote<R: QuoteRepository>(
    repository: &R,
    reference: &str,
) -> Result<Quote, AppError> {
    let wanted = reference.trim();

    repository
        .quotes()
        .await?
        .into_iter()
        .find(|quote| {
            quote.number().to_string().eq_ignore_ascii_case(wanted)
                || quote.id().to_string().eq_ignore_ascii_case(wanted)
        })
        .ok_or_else(|| {
            AppError::rejected(
                "quote_not_found",
                format!("No se encontró la cotización {wanted}"),
            )
        })
}

/// Renders the document with the company it must show and writes it to `path`.
pub async fn save_quote_pdf<R: SettingsRepository>(
    repository: &R,
    quote: &Quote,
    path: &Path,
) -> Result<PathBuf, AppError> {
    let settings = repository.load().await?;
    let bytes = quote_pdf(quote, quote.document_company(settings.company()))
        .map_err(|message| AppError::rejected("pdf_failed", message))?;

    write_pdf(path, &bytes).map_err(|message| AppError::storage("pdf_write_failed", message))
}

/// The quotes of one lead, newest number first.
pub fn lead_quotes(quotes: &[Quote], lead_id: Uuid) -> Vec<Quote> {
    let mut found: Vec<Quote> = quotes
        .iter()
        .filter(|quote| quote.lead_id() == lead_id)
        .cloned()
        .collect();
    found.sort_by_key(|quote| std::cmp::Reverse(quote.number()));
    found
}

/// What a saved change did, in the words of the person who asked for it.
pub fn quote_notice(edit: &QuoteEdit, quote: &Quote, case_is_open: bool) -> String {
    let number = quote.number();

    match edit {
        QuoteEdit::Save => format!("{number} guardada como borrador."),
        QuoteEdit::Send(_) => format!("{number} marcada como enviada."),
        QuoteEdit::Accept(_) => accepted_notice(quote, case_is_open),
        QuoteEdit::Reject(_) => format!("{number} marcada como rechazada."),
        QuoteEdit::NewVersion(_) => format!("{number} lista para editar."),
    }
}
