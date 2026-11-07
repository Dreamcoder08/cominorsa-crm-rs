//! Display rules, kept free of UI framework types so they stay testable.

use cominorsa_domain::{
    BillingMilestone, Case, CaseFile, CaseStatus, CaseStep, CompanyProfile, IgvRate, Lead,
    MilestoneGap, Money, Quote, QuoteStatus, Stage, accepted_quote, renewal_date,
};
use time::{Date, Duration, OffsetDateTime, macros::offset};

const AGENDA_DAYS: i64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowUp {
    Overdue(i64),
    DueToday,
    Upcoming(Date),
    Closed,
}

impl FollowUp {
    pub fn of(lead: &Lead, today: Date) -> Self {
        if lead.stage() == Stage::ContractSigned {
            Self::Closed
        } else {
            Self::on(lead.follow_up_on(), today)
        }
    }

    pub fn of_step(step: &CaseStep, today: Date) -> Self {
        if step.is_done() {
            Self::Closed
        } else {
            Self::on(step.due_on(), today)
        }
    }

    pub fn of_milestone(milestone: &BillingMilestone, today: Date) -> Self {
        if milestone.is_paid() {
            Self::Closed
        } else {
            Self::on(milestone.due_on(), today)
        }
    }

    fn on(date: Date, today: Date) -> Self {
        if date < today {
            Self::Overdue((today - date).whole_days())
        } else if date == today {
            Self::DueToday
        } else {
            Self::Upcoming(date)
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Overdue(1) => "Vencido hace 1 día".to_owned(),
            Self::Overdue(days) => format!("Vencido hace {days} días"),
            Self::DueToday => "Vence hoy".to_owned(),
            Self::Upcoming(date) => format!("Seguimiento {}", format_date(date)),
            Self::Closed => "Cerrado".to_owned(),
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Self::Overdue(_) => "overdue",
            Self::DueToday => "today",
            Self::Upcoming(_) => "upcoming",
            Self::Closed => "closed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineSummary {
    pub active: usize,
    pub overdue: usize,
    pub due_today: usize,
    pub signed: usize,
}

impl PipelineSummary {
    pub fn of(leads: &[Lead], today: Date) -> Self {
        leads.iter().fold(Self::default(), |mut summary, lead| {
            match FollowUp::of(lead, today) {
                FollowUp::Closed => summary.signed += 1,
                FollowUp::Overdue(_) => {
                    summary.active += 1;
                    summary.overdue += 1;
                }
                FollowUp::DueToday => {
                    summary.active += 1;
                    summary.due_today += 1;
                }
                FollowUp::Upcoming(_) => summary.active += 1,
            }
            summary
        })
    }
}

pub fn matches_query(lead: &Lead, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    let ruc = lead.ruc().map_or("", |ruc| ruc.as_str());

    [lead.name(), lead.phone(), ruc, lead.service()]
        .iter()
        .any(|field| field.to_lowercase().contains(&query))
}

pub fn format_date(date: Date) -> String {
    format!(
        "{:02}/{:02}/{}",
        date.day(),
        u8::from(date.month()),
        date.year()
    )
}

pub fn profile_label(profile: &str) -> &str {
    match profile {
        "PERFIL_1_formalizacion" => "Formalización",
        "PERFIL_2_cumplimiento" => "Cumplimiento",
        "MIXTO_1_y_2" => "Mixto",
        "OTRO_estado" => "Por revisar",
        other => other,
    }
}

pub fn next_follow_up(today: Date, days: u32) -> Date {
    today + Duration::days(i64::from(days))
}

/// A commercial follow-up, a pending case step, or an unpaid milestone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgendaItem {
    FollowUp(Lead),
    Step {
        case: Case,
        step: CaseStep,
    },
    Milestone {
        case: Case,
        milestone: BillingMilestone,
    },
}

impl AgendaItem {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::FollowUp(_) => "Seguimiento",
            Self::Step { .. } => "Trámite",
            Self::Milestone { .. } => "Cobro",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::FollowUp(lead) => lead.name(),
            Self::Step { case, .. } | Self::Milestone { case, .. } => case.client_name(),
        }
    }

    fn date(&self) -> Date {
        match self {
            Self::FollowUp(lead) => lead.follow_up_on(),
            Self::Step { step, .. } => step.due_on(),
            Self::Milestone { milestone, .. } => milestone.due_on(),
        }
    }
}

/// Follow-ups, open case steps, and unpaid milestones, oldest date first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgendaGroups {
    pub overdue: Vec<AgendaItem>,
    pub today: Vec<AgendaItem>,
    pub upcoming: Vec<AgendaItem>,
}

impl AgendaGroups {
    pub fn of(leads: &[Lead], cases: &[CaseFile], today: Date) -> Self {
        let mut groups = Self::default();

        for lead in leads {
            groups.place(
                FollowUp::of(lead, today),
                AgendaItem::FollowUp(lead.clone()),
                today,
            );
        }
        for file in cases
            .iter()
            .filter(|file| file.case.status() == CaseStatus::Open)
        {
            for step in &file.steps {
                groups.place(
                    FollowUp::of_step(step, today),
                    AgendaItem::Step {
                        case: file.case.clone(),
                        step: step.clone(),
                    },
                    today,
                );
            }
        }
        // Closing a case stops its steps, never what the client still owes.
        for file in cases {
            for milestone in &file.milestones {
                groups.place(
                    FollowUp::of_milestone(milestone, today),
                    AgendaItem::Milestone {
                        case: file.case.clone(),
                        milestone: milestone.clone(),
                    },
                    today,
                );
            }
        }
        for group in [&mut groups.overdue, &mut groups.today, &mut groups.upcoming] {
            group.sort_by_key(AgendaItem::date);
        }
        groups
    }

    fn place(&mut self, status: FollowUp, item: AgendaItem, today: Date) {
        let horizon = today + Duration::days(AGENDA_DAYS);

        match status {
            FollowUp::Overdue(_) => self.overdue.push(item),
            FollowUp::DueToday => self.today.push(item),
            FollowUp::Upcoming(date) if date <= horizon => self.upcoming.push(item),
            FollowUp::Upcoming(_) | FollowUp::Closed => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.overdue.is_empty() && self.today.is_empty() && self.upcoming.is_empty()
    }
}

pub fn step_status_label(step: &CaseStep, today: Date) -> String {
    match (step.done_on(), FollowUp::of_step(step, today)) {
        (Some(date), _) => format!("Hecho el {}", format_date(date)),
        (None, FollowUp::Upcoming(_)) => "Próximo".to_owned(),
        (None, status) => status.label(),
    }
}

pub fn case_progress(file: &CaseFile) -> String {
    let done = file.steps.iter().filter(|step| step.is_done()).count();

    format!("{done} de {} pasos", file.steps.len())
}

pub fn responsible_label(owner: Option<&str>) -> &str {
    owner.unwrap_or("Sin responsable")
}

/// Formats Unix seconds in Peru time, which has no daylight saving.
pub fn format_timestamp(unix_seconds: i64) -> String {
    let local = OffsetDateTime::from_unix_timestamp(unix_seconds)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .to_offset(offset!(-5));

    format!(
        "{} {:02}:{:02}",
        format_date(local.date()),
        local.hour(),
        local.minute()
    )
}

/// Team members plus the current owner, who may have left the team.
pub fn owner_options(team: &[String], owner: Option<&str>) -> Vec<String> {
    let mut options = team.to_vec();
    if let Some(owner) = owner.filter(|owner| !team.iter().any(|member| member == owner)) {
        options.push(owner.to_owned());
    }
    options
}

pub fn tax_status_class(status: &str) -> &'static str {
    if status == "ACTIVO" {
        "closed"
    } else {
        "overdue"
    }
}

/// Formats an amount as `S/ 1,234.50`.
pub fn format_soles(amount: Money) -> String {
    let digits = (amount.cents() / 100).to_string();
    let mut grouped = String::new();

    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }

    format!("S/ {grouped}.{:02}", amount.cents() % 100)
}

/// The amount as an input value, such as `1234.50`.
pub fn money_input(amount: Money) -> String {
    format!("{}.{:02}", amount.cents() / 100, amount.cents() % 100)
}

pub fn milestone_status_label(milestone: &BillingMilestone, today: Date) -> String {
    match (
        milestone.paid_on(),
        FollowUp::of_milestone(milestone, today),
    ) {
        (Some(date), _) => format!("Pagado el {}", format_date(date)),
        (None, FollowUp::Upcoming(_)) => "Próximo".to_owned(),
        (None, status) => status.label(),
    }
}

/// What closing does with the service, stated before a person confirms.
pub fn renewal_note(case: &Case, today: Date) -> String {
    renewal_date(case.service(), today).map_or_else(
        || "Este servicio no genera renovación.".to_owned(),
        |date| {
            format!(
                "Se creará un lead de renovación con seguimiento sugerido el {}. \
                 Ningún mensaje se envía solo.",
                format_date(date)
            )
        },
    )
}

/// Unpaid milestones survive a closing, and the person should know it.
pub fn close_warning(file: &CaseFile) -> Option<String> {
    let unpaid = file.unpaid_total();

    (unpaid.cents() > 0).then(|| {
        format!(
            "Quedan {} por cobrar; seguirán en la Agenda.",
            format_soles(unpaid)
        )
    })
}

pub fn closing_notice(renewal: Option<&Lead>) -> String {
    renewal.map_or_else(
        || "Expediente cerrado.".to_owned(),
        |lead| {
            format!(
                "Expediente cerrado. Se creó «{}» en el Embudo.",
                lead.name()
            )
        },
    )
}

/// Only closed cases that still owe money stay listed.
pub fn closed_case_label(case: &Case) -> Option<&'static str> {
    (case.status() == CaseStatus::Closed).then_some("Cerrado, con cobros pendientes")
}

/// Printed on every quote, because a quote never replaces a tax document.
pub const QUOTE_TAX_NOTE: &str = "Esta cotización no es un comprobante de pago ni una factura.";

/// The rate as a typed percent, such as `18` or `10.5`.
pub fn igv_input(rate: IgvRate) -> String {
    let whole = rate.basis_points() / 100;
    let decimals = rate.basis_points() % 100;

    match decimals {
        0 => whole.to_string(),
        decimals if decimals.is_multiple_of(10) => format!("{whole}.{}", decimals / 10),
        decimals => format!("{whole}.{decimals:02}"),
    }
}

pub fn format_igv_rate(rate: IgvRate) -> String {
    format!("{}%", igv_input(rate))
}

pub fn igv_label(rate: IgvRate) -> String {
    format!("IGV ({})", format_igv_rate(rate))
}

pub fn quote_file_name(quote: &Quote) -> String {
    format!("{}.pdf", quote.number())
}

/// The header lines of a quote below the company name. Blank fields are left out.
pub fn company_lines(company: &CompanyProfile) -> Vec<String> {
    let fields = [
        ("RUC ", company.ruc.as_str()),
        ("", company.address.as_str()),
        ("Tel. ", company.phone.as_str()),
        ("", company.email.as_str()),
    ];

    fields
        .into_iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(label, value)| format!("{label}{}", value.trim()))
        .collect()
}

pub fn company_warning(company: &CompanyProfile) -> Option<&'static str> {
    (!company.has_name())
        .then_some("Falta el nombre de la empresa. Agrégalo en Configuración antes de emitir.")
}

pub fn quote_status_class(status: QuoteStatus) -> &'static str {
    match status {
        QuoteStatus::Draft => "upcoming",
        QuoteStatus::Sent => "today",
        QuoteStatus::Accepted => "closed",
        QuoteStatus::Rejected => "overdue",
    }
}

pub fn milestone_gap_warning(file: &CaseFile) -> Option<String> {
    let (word, amount) = match file.milestone_gap()? {
        MilestoneGap::Short(amount) => ("faltan", amount),
        MilestoneGap::Over(amount) => ("sobran", amount),
    };
    let total = file.case.contract_total()?;

    Some(format!(
        "Los hitos suman {} y el contrato {}: {word} {}.",
        format_soles(file.milestones_total()),
        format_soles(total),
        format_soles(amount)
    ))
}

/// Names the quote behind the contract total, while the total still matches it.
pub fn suggested_total_note(file: &CaseFile, quotes: &[Quote]) -> Option<String> {
    accepted_quote(quotes, file.case.lead_id())
        .filter(|quote| file.case.contract_total() == Some(quote.totals().total))
        .map(|quote| {
            format!(
                "Sugerido por {}; ajústalo si el contrato difiere.",
                quote.number()
            )
        })
}

pub fn accepted_notice(quote: &Quote, case_is_open: bool) -> String {
    let next = if case_is_open {
        "El expediente ya está abierto: revisa su total en Cobros."
    } else {
        "Su total se sugerirá al abrir el expediente."
    };

    format!("{} aceptada. {next}", quote.number())
}
