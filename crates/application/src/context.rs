//! The day, the instant, and the ids a use case runs with. Production reads
//! the system clock and lets the domain generate ids; a caller that needs
//! determinism injects them here, so no test-only branch exists.

use cominorsa_domain::{
    BillingMilestone, BillingMilestoneRecord, Case, CaseFile, CaseRecord, CaseStep, CaseStepRecord,
    DomainError, Lead, LeadRecord,
};
use time::{
    Date, OffsetDateTime, Time, UtcOffset,
    format_description::BorrowedFormatItem,
    macros::{format_description, offset},
};
use uuid::Uuid;

/// Peru has a fixed UTC-5 offset and no daylight saving time.
const PERU: UtcOffset = offset!(-5);
const NOON: Time = time::macros::time!(12:00);
const FILE_STAMP: &[BorrowedFormatItem<'_>] =
    format_description!("[year][month][day]-[hour][minute][second]");
const SEED_NAMESPACE: Uuid = Uuid::from_u128(0x6f1e_6c0a_52d7_4a0e_9d3b_c0a1_7e5f_2b44);

pub fn today_in_peru() -> Date {
    OffsetDateTime::now_utc().to_offset(PERU).date()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    today: Date,
    now_unix: i64,
}

impl Clock {
    pub fn system() -> Self {
        let now = OffsetDateTime::now_utc();

        Self {
            today: now.to_offset(PERU).date(),
            now_unix: now.unix_timestamp(),
        }
    }

    /// Fixes what is given. A day alone means noon in Peru, and an instant
    /// alone gives its day in Peru; with neither, the system clock decides.
    pub fn fixed(today: Option<Date>, now_unix: Option<i64>) -> Self {
        let system = Self::system();
        let day_of = |instant: i64| {
            OffsetDateTime::from_unix_timestamp(instant)
                .map_or(system.today, |moment| moment.to_offset(PERU).date())
        };
        let noon_of = |day: Date| day.with_time(NOON).assume_offset(PERU).unix_timestamp();

        match (today, now_unix) {
            (Some(today), Some(now_unix)) => Self { today, now_unix },
            (Some(today), None) => Self {
                today,
                now_unix: noon_of(today),
            },
            (None, Some(now_unix)) => Self {
                today: day_of(now_unix),
                now_unix,
            },
            (None, None) => system,
        }
    }

    pub fn today(&self) -> Date {
        self.today
    }

    pub fn now_unix(&self) -> i64 {
        self.now_unix
    }

    /// The instant in Peru time, for file names.
    pub fn file_stamp(&self) -> String {
        OffsetDateTime::from_unix_timestamp(self.now_unix)
            .ok()
            .and_then(|moment| moment.to_offset(PERU).format(FILE_STAMP).ok())
            .unwrap_or_else(|| self.now_unix.to_string())
    }
}

/// Where new ids come from. Without a seed the domain keeps generating its
/// time-ordered ids; with one, each new record takes the next derived id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ids {
    seed: Option<String>,
    issued: u32,
}

impl Ids {
    pub fn generated() -> Self {
        Self::default()
    }

    pub fn seeded(seed: &str) -> Self {
        Self {
            seed: Some(seed.to_owned()),
            issued: 0,
        }
    }

    /// The next derived id, or `None` when the domain's own id stands.
    pub fn issue(&mut self) -> Option<Uuid> {
        let seed = self.seed.as_ref()?;
        let id = Uuid::new_v5(
            &SEED_NAMESPACE,
            format!("{seed}/{}", self.issued).as_bytes(),
        );
        self.issued += 1;

        Some(id)
    }

    pub(crate) fn is_seeded(&self) -> bool {
        self.seed.is_some()
    }

    pub(crate) fn lead(&mut self, lead: Lead) -> Result<Lead, DomainError> {
        let Some(id) = self.issue() else {
            return Ok(lead);
        };

        Lead::restore(LeadRecord {
            id,
            name: lead.name().to_owned(),
            phone: lead.phone().to_owned(),
            ruc: lead.ruc().map(|ruc| ruc.as_str().to_owned()),
            service: lead.service().to_owned(),
            owner: lead.owner().map(str::to_owned),
            stage: lead.stage().code().to_owned(),
            follow_up_on: lead.follow_up_on(),
            renewal_of: lead.renewal_of(),
        })
    }

    pub(crate) fn opening(&mut self, opening: CaseFile) -> Result<CaseFile, DomainError> {
        let Some(id) = self.issue() else {
            return Ok(opening);
        };
        let case = keyed_case(&opening.case, id)?;
        let steps = opening
            .steps
            .iter()
            .map(|step| {
                CaseStep::restore(CaseStepRecord {
                    id: self.issue().unwrap_or(step.id()),
                    case_id: id,
                    position: step.position(),
                    title: step.title().to_owned(),
                    due_on: step.due_on(),
                    done_on: step.done_on(),
                })
            })
            .collect::<Result<_, _>>()?;

        Ok(CaseFile {
            case,
            steps,
            milestones: opening.milestones,
        })
    }

    pub(crate) fn milestones(
        &mut self,
        milestones: Vec<BillingMilestone>,
    ) -> Result<Vec<BillingMilestone>, DomainError> {
        milestones
            .into_iter()
            .map(|milestone| {
                let Some(id) = self.issue() else {
                    return Ok(milestone);
                };

                BillingMilestone::restore(BillingMilestoneRecord {
                    id,
                    case_id: milestone.case_id(),
                    position: milestone.position(),
                    title: milestone.title().to_owned(),
                    amount_cents: milestone.amount().cents(),
                    due_on: milestone.due_on(),
                    paid_on: milestone.paid_on(),
                })
            })
            .collect()
    }
}

fn keyed_case(case: &Case, id: Uuid) -> Result<Case, DomainError> {
    Case::restore(CaseRecord {
        id,
        lead_id: case.lead_id(),
        client_name: case.client_name().to_owned(),
        ruc: case.ruc().map(|ruc| ruc.as_str().to_owned()),
        service: case.service().to_owned(),
        owner: case.owner().map(str::to_owned),
        opened_on: case.opened_on(),
        status: case.status().code().to_owned(),
        contract_total_cents: case.contract_total().map(|total| total.cents()),
    })
}

/// The clock and id source of one operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub clock: Clock,
    pub ids: Ids,
}

impl Context {
    pub fn new(clock: Clock, ids: Ids) -> Self {
        Self { clock, ids }
    }

    /// The production default: the system clock and domain-generated ids.
    pub fn system() -> Self {
        Self::new(Clock::system(), Ids::generated())
    }
}
