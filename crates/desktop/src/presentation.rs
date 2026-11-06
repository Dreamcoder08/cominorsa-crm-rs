//! Display rules, kept free of UI framework types so they stay testable.

use cominorsa_domain::{Lead, Stage};
use time::{Date, Duration};

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
        let date = lead.follow_up_on();

        if lead.stage() == Stage::ContractSigned {
            Self::Closed
        } else if date < today {
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

/// Leads that need attention, in the order the repository returned them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgendaGroups {
    pub overdue: Vec<Lead>,
    pub today: Vec<Lead>,
    pub upcoming: Vec<Lead>,
}

impl AgendaGroups {
    pub fn of(leads: &[Lead], today: Date) -> Self {
        let horizon = today + Duration::days(AGENDA_DAYS);
        let mut groups = Self::default();

        for lead in leads {
            match FollowUp::of(lead, today) {
                FollowUp::Overdue(_) => groups.overdue.push(lead.clone()),
                FollowUp::DueToday => groups.today.push(lead.clone()),
                FollowUp::Upcoming(date) if date <= horizon => groups.upcoming.push(lead.clone()),
                FollowUp::Upcoming(_) | FollowUp::Closed => {}
            }
        }
        groups
    }

    pub fn is_empty(&self) -> bool {
        self.overdue.is_empty() && self.today.is_empty() && self.upcoming.is_empty()
    }
}
