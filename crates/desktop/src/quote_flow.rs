//! The quote form and its changes, validated through the domain.

use cominorsa_domain::{
    DomainError, IgvRate, Lead, Quote, QuoteContent, QuoteDraft, QuoteLine, QuoteRepository,
    QuoteStatus, QuoteTotals, Settings, is_superseded,
};
use time::{Date, Duration, format_description::well_known::Iso8601};
use uuid::Uuid;

use crate::{accepted_notice, money_input};

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

    fn content(&self) -> Result<QuoteContent, String> {
        let date = |value: &str, missing: &str| {
            Date::parse(value, &Iso8601::DATE).map_err(|_| missing.to_owned())
        };

        Ok(QuoteContent {
            issued_on: date(&self.issued_on, "Ingresa la fecha de emisión")?,
            valid_until: date(&self.valid_until, "Ingresa la fecha de validez")?,
            lines: self.parsed_lines().map_err(|error| error.to_string())?,
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
fn current(quote: Quote, quotes: &[Quote]) -> Result<Quote, String> {
    if is_superseded(&quote, quotes) {
        Err("Esta versión ya fue reemplazada por otra más reciente".to_owned())
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
) -> Result<QuoteChange, String> {
    let message = |error: DomainError| error.to_string();
    let igv_rate = settings.igv_rate();

    match edit {
        QuoteEdit::Save => {
            let content = values.content()?;
            let Some(id) = values.editing else {
                return QuoteDraft::for_lead(lead, content, igv_rate)
                    .map(QuoteChange::Create)
                    .map_err(message);
            };
            let mut quote = quotes
                .iter()
                .find(|quote| quote.id() == id)
                .cloned()
                .ok_or_else(|| "La cotización ya no existe".to_owned())?;
            quote.revise(content, igv_rate).map_err(message)?;
            Ok(QuoteChange::Draft(quote))
        }
        QuoteEdit::Send(mut quote) => {
            quote.mark_sent(settings.company()).map_err(message)?;
            Ok(QuoteChange::Status(quote, QuoteStatus::Draft))
        }
        QuoteEdit::Accept(quote) => {
            let mut quote = current(quote, quotes)?;
            quote.mark_accepted().map_err(message)?;
            Ok(QuoteChange::Status(quote, QuoteStatus::Sent))
        }
        QuoteEdit::Reject(quote) => {
            let mut quote = current(quote, quotes)?;
            quote.mark_rejected().map_err(message)?;
            Ok(QuoteChange::Status(quote, QuoteStatus::Sent))
        }
        QuoteEdit::NewVersion(quote) => quote
            .new_version(today, igv_rate)
            .map(QuoteChange::Create)
            .map_err(message),
    }
}

pub async fn save_quote<R: QuoteRepository>(
    repository: &R,
    change: QuoteChange,
) -> Result<Quote, String> {
    let message = |error: cominorsa_domain::RepositoryError| error.to_string();

    match change {
        QuoteChange::Create(draft) => repository.create_quote(&draft).await.map_err(message),
        QuoteChange::Draft(quote) => {
            repository.save_quote_draft(&quote).await.map_err(message)?;
            Ok(quote)
        }
        QuoteChange::Status(quote, from) => {
            repository
                .save_quote_status(&quote, from)
                .await
                .map_err(message)?;
            Ok(quote)
        }
    }
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
