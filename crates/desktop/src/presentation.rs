//! Display rules, kept free of UI framework types so they stay testable.

use cominorsa_domain::{Case, CaseFile, CaseStatus, CaseStep, Lead, Stage};
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

/// A commercial follow-up or a pending case step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgendaItem {
    FollowUp(Lead),
    Step { case: Case, step: CaseStep },
}

impl AgendaItem {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::FollowUp(_) => "Seguimiento",
            Self::Step { .. } => "Trámite",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::FollowUp(lead) => lead.name(),
            Self::Step { case, .. } => case.client_name(),
        }
    }

    fn date(&self) -> Date {
        match self {
            Self::FollowUp(lead) => lead.follow_up_on(),
            Self::Step { step, .. } => step.due_on(),
        }
    }
}

/// Follow-ups and open case steps that need attention, oldest date first.
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
