//! The CLI's JSON shapes, repeated here because `crates/cli` is a binary. The
//! contract tests compare every read tool with `cominorsa --json`, so the two
//! cannot drift apart unnoticed. The only change is in `settings`: the backup
//! folder is a file system path, so a tool reports only whether one is set.

use cominorsa_application::{AgendaGroups, AgendaItem, FollowUp, Status};
use cominorsa_domain::{
    BillingMilestone, CaseFile, CaseStep, CompanyProfile, Lead, Prospect, Quote, Settings,
};
use serde_json::{Value, json};
use time::Date;

pub fn lead(lead: &Lead) -> Value {
    json!({
        "id": lead.id().to_string(),
        "name": lead.name(),
        "client_name": lead.client_name(),
        "phone": lead.phone(),
        "ruc": lead.ruc().map(|ruc| ruc.as_str()),
        "service": lead.service(),
        "owner": lead.owner(),
        "stage": lead.stage().code(),
        "follow_up_on": lead.follow_up_on().to_string(),
        "renewal_of": lead.renewal_of().map(|id| id.to_string()),
        "origin": lead.origin().code(),
        "consent": lead.consent().map(|consent| json!({
            "given": consent.given,
            "recorded_on": consent.recorded_on.to_string(),
            "source": consent.source,
        })),
    })
}

pub fn follow_up(follow_up: &FollowUp) -> Value {
    json!({"status": follow_up.class(), "label": follow_up.label()})
}

/// A lead with its follow-up status for the day, as `lead show` reports it.
pub fn listed_lead(found: &Lead, today: Date) -> Value {
    let mut listed = lead(found);
    listed["follow_up"] = follow_up(&FollowUp::of(found, today));
    listed
}

pub fn step(step: &CaseStep) -> Value {
    json!({
        "id": step.id().to_string(),
        "case_id": step.case_id().to_string(),
        "position": step.position(),
        "title": step.title(),
        "due_on": step.due_on().to_string(),
        "done_on": step.done_on().map(|date| date.to_string()),
    })
}

pub fn milestone(milestone: &BillingMilestone) -> Value {
    json!({
        "id": milestone.id().to_string(),
        "case_id": milestone.case_id().to_string(),
        "position": milestone.position(),
        "title": milestone.title(),
        "amount_cents": milestone.amount().cents(),
        "due_on": milestone.due_on().to_string(),
        "paid_on": milestone.paid_on().map(|date| date.to_string()),
    })
}

pub fn case(file: &CaseFile) -> Value {
    let case = &file.case;

    json!({
        "id": case.id().to_string(),
        "lead_id": case.lead_id().to_string(),
        "client_name": case.client_name(),
        "ruc": case.ruc().map(|ruc| ruc.as_str()),
        "service": case.service(),
        "owner": case.owner(),
        "opened_on": case.opened_on().to_string(),
        "status": case.status().code(),
        "contract_total_cents": case.contract_total().map(|total| total.cents()),
        "steps": file.steps.iter().map(step).collect::<Vec<_>>(),
        "milestones": file.milestones.iter().map(milestone).collect::<Vec<_>>(),
    })
}

fn company(company: &CompanyProfile) -> Value {
    json!({
        "name": company.name,
        "ruc": company.ruc,
        "address": company.address,
        "phone": company.phone,
        "email": company.email,
    })
}

pub fn quote(quote: &Quote) -> Value {
    let totals = quote.totals();
    let lines: Vec<Value> = quote
        .lines()
        .iter()
        .map(|line| {
            json!({
                "description": line.description(),
                "quantity": line.quantity(),
                "unit_price_cents": line.unit_price().cents(),
                "total_cents": line.total().cents(),
            })
        })
        .collect();

    json!({
        "id": quote.id().to_string(),
        "number": quote.number().to_string(),
        "lead_id": quote.lead_id().to_string(),
        "supersedes": quote.supersedes().map(|id| id.to_string()),
        "client_name": quote.client_name(),
        "client_ruc": quote.client_ruc().map(|ruc| ruc.as_str()),
        "service": quote.service(),
        "issued_on": quote.issued_on().to_string(),
        "valid_until": quote.valid_until().to_string(),
        "notes": quote.notes(),
        "status": quote.status().code(),
        "igv_basis_points": quote.igv_rate().basis_points(),
        "lines": lines,
        "totals": {
            "subtotal_cents": totals.subtotal.cents(),
            "igv_cents": totals.igv.cents(),
            "total_cents": totals.total.cents(),
        },
        "company": company(quote.company()),
    })
}

pub fn prospect(prospect: &Prospect) -> Value {
    let tax = prospect.tax.as_ref().map(|tax| {
        json!({
            "ruc": tax.ruc,
            "status": tax.status,
            "condition": tax.condition,
            "address": tax.address,
            "approximate": tax.approximate,
        })
    });

    json!({
        "kind": prospect.kind.code(),
        "name": prospect.name,
        "profile": prospect.profile,
        "concessions": prospect.concessions,
        "hectares": prospect.hectares,
        "departments": prospect.departments,
        "provinces": prospect.provinces,
        "substances": prospect.substances,
        "concession_states": prospect.concession_states,
        "potential_service": prospect.potential_service,
        "needs_review": prospect.needs_review,
        "source": prospect.source,
        "tax": tax,
    })
}

pub fn settings(settings: &Settings) -> Value {
    json!({
        "theme": settings.theme.code(),
        "team": settings.team(),
        "follow_up_days": settings.follow_up_days(),
        "whatsapp_template": settings.whatsapp_template(),
        "backup_directory_set": settings.backup_directory().is_some(),
        "last_backup_at": settings.last_backup_at(),
        "backup_interval_days": settings.backup_interval_days(),
        "backup_keep": settings.backup_keep(),
        "igv_basis_points": settings.igv_rate().basis_points(),
        "company": company(settings.company()),
    })
}

fn agenda_item(item: &AgendaItem, today: Date) -> Value {
    let date = item.date().to_string();

    match item {
        AgendaItem::FollowUp(lead) => json!({
            "kind": "follow_up",
            "name": item.name(),
            "date": date,
            "status": FollowUp::of(lead, today).class(),
            "lead_id": lead.id().to_string(),
        }),
        AgendaItem::Step { case, step } => json!({
            "kind": "step",
            "name": item.name(),
            "date": date,
            "status": FollowUp::of_step(step, today).class(),
            "case_id": case.id().to_string(),
            "step_id": step.id().to_string(),
            "title": step.title(),
        }),
        AgendaItem::Milestone { case, milestone } => json!({
            "kind": "milestone",
            "name": item.name(),
            "date": date,
            "status": FollowUp::of_milestone(milestone, today).class(),
            "case_id": case.id().to_string(),
            "milestone_id": milestone.id().to_string(),
            "title": milestone.title(),
            "amount_cents": milestone.amount().cents(),
        }),
    }
}

pub fn agenda(groups: &AgendaGroups, today: Date) -> Value {
    let list = |items: &[AgendaItem]| {
        items
            .iter()
            .map(|item| agenda_item(item, today))
            .collect::<Vec<_>>()
    };

    json!({
        "date": today.to_string(),
        "overdue": list(&groups.overdue),
        "due_today": list(&groups.today),
        "upcoming": list(&groups.upcoming),
    })
}

pub fn status(status: &Status) -> Value {
    json!({
        "schema_version": status.schema_version,
        "integrity_check": status.integrity_check,
        "counts": {
            "leads": status.leads,
            "cases": status.cases,
            "active_cases": status.active_cases,
            "case_steps": status.case_steps,
            "billing_milestones": status.billing_milestones,
            "quotes": status.quotes,
            "prospects": status.prospects,
            "team_members": status.team_members,
        },
        "unreadable_rows": {"lead_consents": status.unreadable_consents},
    })
}
