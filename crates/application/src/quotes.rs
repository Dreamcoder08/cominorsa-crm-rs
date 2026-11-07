//! The quote form and its changes, validated through the domain.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use cominorsa_domain::{
    CaseRepository, DomainError, IgvRate, Lead, Quote, QuoteContent, QuoteDraft, QuoteLine,
    QuoteRepository, QuoteStatus, QuoteTotals, Settings, SettingsRepository, is_superseded,
};
use time::{Date, Duration, format_description::well_known::Iso8601};
use uuid::Uuid;

use crate::{
    context::{Context, Ids},
    error::AppError,
    presentation::{accepted_notice, money_input},
    quote_pdf::{pdf_path, quote_pdf, write_pdf},
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

/// The parts of a quote to set. Anything left out keeps its value, and lines
/// replace the whole list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuoteFields {
    pub issued_on: Option<String>,
    pub valid_until: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<QuoteLineValues>,
}

impl QuoteFields {
    fn apply_to(self, values: &mut QuoteValues) {
        if let Some(date) = self.issued_on {
            values.issued_on = date;
        }
        if let Some(date) = self.valid_until {
            values.valid_until = date;
        }
        if let Some(notes) = self.notes {
            values.notes = notes;
        }
        if !self.lines.is_empty() {
            values.lines = self.lines;
        }
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

/// A seeded id can repeat. The same request then returns the stored quote and
/// uses no number; the same id for anything else is refused.
fn refuse_foreign_repeat(draft: &QuoteDraft, quotes: &[Quote]) -> Result<(), AppError> {
    let Some(stored) = quotes.iter().find(|quote| quote.id() == draft.id()) else {
        return Ok(());
    };
    let requested = draft.clone().numbered(stored.number());
    // A new version copies its quote, so naming the same quote is the request.
    let same_content = draft.revised_quote().is_some()
        || (stored.issued_on() == requested.issued_on()
            && stored.valid_until() == requested.valid_until()
            && stored.notes() == requested.notes()
            && stored.lines() == requested.lines());

    if same_content
        && stored.lead_id() == requested.lead_id()
        && stored.supersedes() == requested.supersedes()
    {
        Ok(())
    } else {
        Err(AppError::seed_reused(
            "Ese identificador ya pertenece a otra cotización",
        ))
    }
}

async fn save_edit<R>(
    repository: &R,
    lead: &Lead,
    edit: QuoteEdit,
    values: &QuoteValues,
    today: Date,
    ids: &mut Ids,
) -> Result<SavedQuote, AppError>
where
    R: QuoteRepository + SettingsRepository + CaseRepository,
{
    let quotes = repository.quotes().await?;
    let settings = repository.load().await?;
    let change = match quote_change(lead, edit.clone(), values, &quotes, &settings, today)? {
        QuoteChange::Create(draft) => {
            let draft = ids.quote(draft);
            refuse_foreign_repeat(&draft, &quotes)?;
            QuoteChange::Create(draft)
        }
        change => change,
    };
    let case_is_open = repository
        .active_cases()
        .await?
        .iter()
        .any(|file| file.case.lead_id() == lead.id());
    let quote = save_quote(repository, change).await?;
    let notice = quote_notice(&edit, &quote, case_is_open);

    Ok(SavedQuote { quote, notice })
}

/// Runs one action of the quote panel against the stored quotes and settings.
pub async fn save_quote_edit<R>(
    repository: &R,
    lead: &Lead,
    edit: QuoteEdit,
    values: &QuoteValues,
    context: &mut Context,
) -> Result<SavedQuote, AppError>
where
    R: QuoteRepository + SettingsRepository + CaseRepository,
{
    let today = context.clock.today();

    save_edit(repository, lead, edit, values, today, &mut context.ids).await
}

/// Saves a new draft for the lead. Unlike the panel, it starts with no lines.
pub async fn create_quote<R>(
    repository: &R,
    lead: &Lead,
    fields: QuoteFields,
    context: &mut Context,
) -> Result<SavedQuote, AppError>
where
    R: QuoteRepository + SettingsRepository + CaseRepository,
{
    let mut values = QuoteValues {
        lines: Vec::new(),
        ..QuoteValues::starting_on(context.clock.today())
    };
    fields.apply_to(&mut values);

    save_quote_edit(repository, lead, QuoteEdit::Save, &values, context).await
}

/// Changes the given parts of a stored draft.
pub async fn edit_quote<R>(
    repository: &R,
    lead: &Lead,
    quote: &Quote,
    fields: QuoteFields,
    context: &mut Context,
) -> Result<SavedQuote, AppError>
where
    R: QuoteRepository + SettingsRepository + CaseRepository,
{
    let mut values = QuoteValues::of_quote(quote);
    fields.apply_to(&mut values);

    save_quote_edit(repository, lead, QuoteEdit::Save, &values, context).await
}

/// The totals of some lines at the stored rate, or why a line cannot be read.
pub async fn preview_quote<R: SettingsRepository>(
    repository: &R,
    lines: Vec<QuoteLineValues>,
) -> Result<QuoteTotals, AppError> {
    let settings = repository.load().await?;
    let values = QuoteValues {
        lines,
        ..QuoteValues::default()
    };

    Ok(QuoteTotals::of(
        &values.parsed_lines()?,
        settings.igv_rate(),
    )?)
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

    save_edit(
        repository,
        lead,
        QuoteEdit::Send(quote),
        &QuoteValues::default(),
        today,
        &mut Ids::generated(),
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

/// Where a quote PDF goes. `database` is never replaced; another existing file
/// is replaced only with `overwrite`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfTarget<'a> {
    pub path: &'a Path,
    pub database: &'a Path,
    pub overwrite: bool,
}

/// The real location of a path, which may not exist yet.
fn resolved(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));

        match (fs::canonicalize(parent), path.file_name()) {
            (Ok(folder), Some(name)) => folder.join(name),
            _ => path.to_path_buf(),
        }
    })
}

/// What marks one stored file, whatever name reaches it.
#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> impl Eq {
    use std::os::unix::fs::MetadataExt;

    (metadata.dev(), metadata.ino())
}

/// Stable Rust has no file index on Windows, and asking the system needs
/// `unsafe`. Hard links share these values, so a link is caught; only another
/// file that matches them all is refused by mistake.
#[cfg(not(unix))]
fn file_identity(metadata: &fs::Metadata) -> impl Eq {
    (
        metadata.len(),
        metadata.created().ok(),
        metadata.modified().ok(),
    )
}

/// Whether both paths reach one stored file, as hard links do.
fn is_same_file(left: &Path, right: &Path) -> bool {
    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => file_identity(&left) == file_identity(&right),
        _ => false,
    }
}

fn is_database_file(path: &Path, database: &Path) -> bool {
    let wanted = resolved(path);

    ["", "-wal", "-shm", "-journal"].iter().any(|suffix| {
        let mut name = OsString::from(database);
        name.push(suffix);
        let file = Path::new(&name);
        wanted == resolved(file) || is_same_file(path, file)
    })
}

/// Renders the document with the company it must show and writes it.
pub async fn save_quote_pdf<R: SettingsRepository>(
    repository: &R,
    quote: &Quote,
    target: PdfTarget<'_>,
) -> Result<PathBuf, AppError> {
    let path = pdf_path(target.path);
    if is_database_file(&path, target.database) {
        return Err(AppError::rejected(
            "pdf_target_is_database",
            "El PDF no puede reemplazar la base de datos",
        ));
    }
    if path.exists() && !target.overwrite {
        return Err(AppError::rejected(
            "pdf_target_exists",
            format!(
                "Ya existe {}. Usa --force para reemplazarlo",
                path.display()
            ),
        ));
    }

    let settings = repository.load().await?;
    let bytes = quote_pdf(quote, quote.document_company(settings.company()))
        .map_err(|message| AppError::rejected("pdf_failed", message))?;

    write_pdf(&path, &bytes).map_err(|message| AppError::storage("pdf_write_failed", message))
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
