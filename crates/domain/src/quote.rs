//! Numbered commercial quotes of a lead. A quote is not a tax invoice, and
//! this module never issues SUNAT documents.
//!
//! Money is integer cents. Unit prices exclude IGV. A line total is quantity
//! times unit price, and IGV is charged once on the subtotal, rounding half a
//! cent up, so a quote shows a single unambiguous total.

use std::{fmt, future::Future};

use time::Date;
use uuid::Uuid;

use crate::{
    CompanyProfile, DomainError, Lead, Money, RepositoryError, Ruc,
    lead::{optional_ruc, required},
};

/// To be reviewed by the client's accountant; it is an editable setting.
const DEFAULT_IGV_BASIS_POINTS: u32 = 1800;
const BASIS_POINTS_PER_PERCENT: u32 = 100;
const BASIS_POINTS_PER_WHOLE: u32 = 10_000;

/// An IGV rate in hundredths of a percent, so it never touches a float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IgvRate(u32);

impl Default for IgvRate {
    fn default() -> Self {
        Self(DEFAULT_IGV_BASIS_POINTS)
    }
}

impl IgvRate {
    pub fn from_basis_points(basis_points: u32) -> Result<Self, DomainError> {
        if basis_points <= BASIS_POINTS_PER_WHOLE {
            Ok(Self(basis_points))
        } else {
            Err(DomainError::InvalidIgvRate)
        }
    }

    /// Reads a typed percent such as `18` or `10.5`.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        let (whole, decimals) = match text.trim().split_once('.') {
            Some((_, "")) => return Err(DomainError::InvalidIgvRate),
            Some(parts) => parts,
            None => (text.trim(), ""),
        };
        let is_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if whole.is_empty() || decimals.len() > 2 || !is_digits(whole) || !is_digits(decimals) {
            return Err(DomainError::InvalidIgvRate);
        }

        let whole: u32 = whole.parse().map_err(|_| DomainError::InvalidIgvRate)?;
        let decimals: u32 = format!("{decimals:0<2}")
            .parse()
            .map_err(|_| DomainError::InvalidIgvRate)?;
        let basis_points = whole
            .checked_mul(BASIS_POINTS_PER_PERCENT)
            .and_then(|whole| whole.checked_add(decimals))
            .ok_or(DomainError::InvalidIgvRate)?;

        Self::from_basis_points(basis_points)
    }

    pub fn basis_points(self) -> u32 {
        self.0
    }
}

/// `COT-2026-0001`, sequential per year. Later versions of the same quote keep
/// the number and add a suffix, as in `COT-2026-0001-V2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct QuoteNumber {
    year: i32,
    sequence: u32,
    version: u32,
}

impl QuoteNumber {
    pub fn new(year: i32, sequence: u32, version: u32) -> Result<Self, DomainError> {
        if sequence == 0 || version == 0 {
            return Err(DomainError::CorruptRecord);
        }

        Ok(Self {
            year,
            sequence,
            version,
        })
    }

    /// The same number, one version later.
    pub fn following_version(self) -> Self {
        Self {
            version: self.version + 1,
            ..self
        }
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn sequence(self) -> u32 {
        self.sequence
    }

    pub fn version(self) -> u32 {
        self.version
    }
}

impl fmt::Display for QuoteNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "COT-{}-{:04}", self.year, self.sequence)?;
        if self.version > 1 {
            write!(formatter, "-V{}", self.version)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteStatus {
    Draft,
    Sent,
    Accepted,
    Rejected,
}

impl QuoteStatus {
    pub const ALL: [Self; 4] = [Self::Draft, Self::Sent, Self::Accepted, Self::Rejected];

    /// Stable value used for persistence.
    pub fn code(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Sent => "sent",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Borrador",
            Self::Sent => "Enviada",
            Self::Accepted => "Aceptada",
            Self::Rejected => "Rechazada",
        }
    }
}

/// One line of a quote, with its amount already computed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteLine {
    description: String,
    quantity: u32,
    unit_price: Money,
    total: Money,
}

impl QuoteLine {
    pub fn new(description: &str, quantity: u32, unit_price: Money) -> Result<Self, DomainError> {
        if quantity == 0 {
            return Err(DomainError::InvalidQuantity);
        }
        let unit_price = unit_price.positive()?;
        let total = unit_price
            .cents()
            .checked_mul(i64::from(quantity))
            .ok_or(DomainError::InvalidAmount)
            .and_then(Money::from_cents)?;

        Ok(Self {
            description: required(description, "descripción")?,
            quantity,
            unit_price,
            total,
        })
    }

    /// Reads a line as typed in a form.
    pub fn parse(description: &str, quantity: &str, unit_price: &str) -> Result<Self, DomainError> {
        let quantity = quantity
            .trim()
            .parse()
            .map_err(|_| DomainError::InvalidQuantity)?;

        Self::new(description, quantity, Money::parse(unit_price)?)
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn quantity(&self) -> u32 {
        self.quantity
    }

    pub fn unit_price(&self) -> Money {
        self.unit_price
    }

    pub fn total(&self) -> Money {
        self.total
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteTotals {
    pub subtotal: Money,
    pub igv: Money,
    pub total: Money,
}

impl QuoteTotals {
    pub fn of(lines: &[QuoteLine], rate: IgvRate) -> Result<Self, DomainError> {
        let subtotal = lines
            .iter()
            .try_fold(0_i64, |sum, line| sum.checked_add(line.total.cents()))
            .ok_or(DomainError::InvalidAmount)
            .and_then(Money::from_cents)?;
        let igv = Money::from_cents(igv_cents(subtotal, rate))?;

        Ok(Self {
            subtotal,
            igv,
            total: Money::from_cents(subtotal.cents() + igv.cents())?,
        })
    }
}

/// Rounds half a cent up. A subtotal fits `Money`, so the product fits `i64`.
fn igv_cents(subtotal: Money, rate: IgvRate) -> i64 {
    let whole = i64::from(BASIS_POINTS_PER_WHOLE);

    (subtotal.cents() * i64::from(rate.0) + whole / 2) / whole
}

/// What a person writes in a quote, before it belongs to a lead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteContent {
    pub issued_on: Date,
    pub valid_until: Date,
    pub lines: Vec<QuoteLine>,
    pub notes: String,
}

impl QuoteContent {
    fn validated(self, rate: IgvRate) -> Result<(Self, QuoteTotals), DomainError> {
        if self.lines.is_empty() {
            return Err(DomainError::EmptyQuote);
        }
        if self.valid_until < self.issued_on {
            return Err(DomainError::InvalidQuoteValidity);
        }

        let totals = QuoteTotals::of(&self.lines, rate)?;
        let notes = self.notes.trim().to_owned();
        Ok((Self { notes, ..self }, totals))
    }
}

/// A validated quote waiting for its number. Storage assigns the number inside
/// the transaction that saves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteDraft {
    id: Uuid,
    lead_id: Uuid,
    client_name: String,
    client_ruc: Option<Ruc>,
    service: String,
    content: QuoteContent,
    igv_rate: IgvRate,
    totals: QuoteTotals,
    revises: Option<(Uuid, QuoteNumber)>,
}

impl QuoteDraft {
    /// Copies the lead's client data, so later lead edits leave the quote as it was.
    pub fn for_lead(
        lead: &Lead,
        content: QuoteContent,
        igv_rate: IgvRate,
    ) -> Result<Self, DomainError> {
        let (content, totals) = content.validated(igv_rate)?;

        Ok(Self {
            id: Uuid::now_v7(),
            lead_id: lead.id(),
            client_name: lead.client_name().to_owned(),
            client_ruc: lead.ruc().cloned(),
            service: lead.service().to_owned(),
            content,
            igv_rate,
            totals,
            revises: None,
        })
    }

    /// The same draft under an id chosen by the caller, for reproducible runs.
    #[must_use]
    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn year(&self) -> i32 {
        self.content.issued_on.year()
    }

    pub fn revised_quote(&self) -> Option<Uuid> {
        self.revises.map(|(id, _)| id)
    }

    /// The number of the new version, or `None` for a new quote.
    pub fn next_version(&self) -> Option<QuoteNumber> {
        self.revises.map(|(_, number)| number.following_version())
    }

    pub fn numbered(self, number: QuoteNumber) -> Quote {
        Quote {
            id: self.id,
            lead_id: self.lead_id,
            number,
            supersedes: self.revised_quote(),
            client_name: self.client_name,
            client_ruc: self.client_ruc,
            service: self.service,
            content: self.content,
            igv_rate: self.igv_rate,
            totals: self.totals,
            status: QuoteStatus::Draft,
            company: CompanyProfile::default(),
        }
    }
}

/// Persisted line values, validated again when restored.
#[derive(Clone, Debug)]
pub struct QuoteLineRecord {
    pub description: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
}

/// Persisted quote values, validated again when restored.
#[derive(Clone, Debug)]
pub struct QuoteRecord {
    pub id: Uuid,
    pub lead_id: Uuid,
    pub year: i64,
    pub sequence: i64,
    pub version: i64,
    pub supersedes: Option<Uuid>,
    pub client_name: String,
    pub client_ruc: Option<String>,
    pub service: String,
    pub issued_on: Date,
    pub valid_until: Date,
    pub notes: String,
    pub status: String,
    pub igv_basis_points: i64,
    pub company: CompanyProfile,
    pub lines: Vec<QuoteLineRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    id: Uuid,
    lead_id: Uuid,
    number: QuoteNumber,
    supersedes: Option<Uuid>,
    client_name: String,
    client_ruc: Option<Ruc>,
    service: String,
    content: QuoteContent,
    igv_rate: IgvRate,
    totals: QuoteTotals,
    status: QuoteStatus,
    company: CompanyProfile,
}

impl Quote {
    pub fn restore(record: QuoteRecord) -> Result<Self, DomainError> {
        let corrupt = |_| DomainError::CorruptRecord;
        let whole = |value: i64| u32::try_from(value).map_err(|_| DomainError::CorruptRecord);

        let lines = record
            .lines
            .iter()
            .map(|line| {
                QuoteLine::new(
                    &line.description,
                    whole(line.quantity)?,
                    Money::from_cents(line.unit_price_cents)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(corrupt)?;
        let igv_rate = whole(record.igv_basis_points)
            .and_then(IgvRate::from_basis_points)
            .map_err(corrupt)?;
        let (content, totals) = QuoteContent {
            issued_on: record.issued_on,
            valid_until: record.valid_until,
            lines,
            notes: record.notes,
        }
        .validated(igv_rate)
        .map_err(corrupt)?;
        let year = i32::try_from(record.year).map_err(|_| DomainError::CorruptRecord)?;

        Ok(Self {
            id: record.id,
            lead_id: record.lead_id,
            number: QuoteNumber::new(year, whole(record.sequence)?, whole(record.version)?)?,
            supersedes: record.supersedes,
            client_name: required(&record.client_name, "cliente").map_err(corrupt)?,
            client_ruc: optional_ruc(record.client_ruc.as_deref()).map_err(corrupt)?,
            service: record.service,
            content,
            igv_rate,
            totals,
            status: QuoteStatus::from_code(&record.status).ok_or(DomainError::CorruptRecord)?,
            company: record.company,
        })
    }

    /// Replaces the content of a draft. Anything sent needs a new version.
    pub fn revise(&mut self, content: QuoteContent, igv_rate: IgvRate) -> Result<(), DomainError> {
        if self.status != QuoteStatus::Draft {
            return Err(DomainError::QuoteLocked);
        }

        let (content, totals) = content.validated(igv_rate)?;
        self.content = content;
        self.igv_rate = igv_rate;
        self.totals = totals;
        Ok(())
    }

    /// Issues a draft, freezing the company data printed on it.
    pub fn mark_sent(&mut self, company: &CompanyProfile) -> Result<(), DomainError> {
        self.move_status(QuoteStatus::Draft, QuoteStatus::Sent)?;
        self.company = company.clone();
        Ok(())
    }

    pub fn mark_accepted(&mut self) -> Result<(), DomainError> {
        self.move_status(QuoteStatus::Sent, QuoteStatus::Accepted)
    }

    pub fn mark_rejected(&mut self) -> Result<(), DomainError> {
        self.move_status(QuoteStatus::Sent, QuoteStatus::Rejected)
    }

    fn move_status(&mut self, from: QuoteStatus, to: QuoteStatus) -> Result<(), DomainError> {
        if self.status != from {
            return Err(DomainError::InvalidQuoteTransition);
        }

        self.status = to;
        Ok(())
    }

    /// A new draft with the same content, number, and client. The validity
    /// keeps its length from `issued_on`. A draft is edited, not versioned.
    pub fn new_version(
        &self,
        issued_on: Date,
        igv_rate: IgvRate,
    ) -> Result<QuoteDraft, DomainError> {
        if self.status == QuoteStatus::Draft {
            return Err(DomainError::InvalidQuoteTransition);
        }

        let content = QuoteContent {
            issued_on,
            valid_until: issued_on + (self.content.valid_until - self.content.issued_on),
            ..self.content.clone()
        };
        let (content, totals) = content.validated(igv_rate)?;

        Ok(QuoteDraft {
            id: Uuid::now_v7(),
            lead_id: self.lead_id,
            client_name: self.client_name.clone(),
            client_ruc: self.client_ruc.clone(),
            service: self.service.clone(),
            content,
            igv_rate,
            totals,
            revises: Some((self.id, self.number)),
        })
    }

    /// The company printed on the document: today's profile while it is a
    /// draft, and the one frozen when it was sent afterwards.
    pub fn document_company<'a>(&'a self, current: &'a CompanyProfile) -> &'a CompanyProfile {
        match self.status {
            QuoteStatus::Draft => current,
            _ => &self.company,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn lead_id(&self) -> Uuid {
        self.lead_id
    }

    pub fn number(&self) -> QuoteNumber {
        self.number
    }

    pub fn supersedes(&self) -> Option<Uuid> {
        self.supersedes
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn client_ruc(&self) -> Option<&Ruc> {
        self.client_ruc.as_ref()
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn issued_on(&self) -> Date {
        self.content.issued_on
    }

    pub fn valid_until(&self) -> Date {
        self.content.valid_until
    }

    pub fn lines(&self) -> &[QuoteLine] {
        &self.content.lines
    }

    pub fn notes(&self) -> &str {
        &self.content.notes
    }

    pub fn igv_rate(&self) -> IgvRate {
        self.igv_rate
    }

    pub fn totals(&self) -> QuoteTotals {
        self.totals
    }

    pub fn status(&self) -> QuoteStatus {
        self.status
    }

    pub fn company(&self) -> &CompanyProfile {
        &self.company
    }
}

/// The lead's accepted quote that suggests a contract total: the newest one by
/// number when several were accepted.
pub fn accepted_quote(quotes: &[Quote], lead_id: Uuid) -> Option<&Quote> {
    quotes
        .iter()
        .filter(|quote| quote.lead_id == lead_id && quote.status == QuoteStatus::Accepted)
        .max_by_key(|quote| quote.number)
}

/// Whether a newer version of this quote exists.
pub fn is_superseded(quote: &Quote, quotes: &[Quote]) -> bool {
    quotes
        .iter()
        .any(|other| other.supersedes == Some(quote.id))
}

/// Quote persistence port.
pub trait QuoteRepository: Send + Sync {
    /// Assigns the number and stores the quote with its lines in one
    /// transaction. Repeating a draft, or versioning a quote twice, returns the
    /// quote already stored and uses no number.
    fn create_quote(
        &self,
        draft: &QuoteDraft,
    ) -> impl Future<Output = Result<Quote, RepositoryError>> + Send;

    /// Stores new content of a quote that is still a draft in storage.
    fn save_quote_draft(
        &self,
        quote: &Quote,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Stores a status change, if the stored quote is still in status `from`.
    fn save_quote_status(
        &self,
        quote: &Quote,
        from: QuoteStatus,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// Every quote, oldest number first.
    fn quotes(&self) -> impl Future<Output = Result<Vec<Quote>, RepositoryError>> + Send;
}
