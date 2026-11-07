//! Maps each command to its use case. No business rule lives here.

use std::{
    env,
    path::{Path, PathBuf},
};

use cominorsa_application::{
    AppError, BillingChange, BillingEdit, Clock, Context, FormValues, Ids, MilestoneValues,
    PipelineSummary, ProspectConversion, QuoteEdit, QuoteLineValues, QuoteValues, SettingsChange,
    add_lead, advance_lead, agenda, apply_billing, assign_lead, billing_change, close_case,
    close_warning, closing_notice, convert_prospect, find_case, find_lead, find_milestone,
    find_quote, find_stored_case, lead_quotes, matches_query, migrate_database,
    milestone_gap_warning, next_follow_up, open_for_reading, reschedule_step, restore_backup,
    run_backup, save_billing, save_quote_edit, save_quote_pdf, send_quote, set_step_done, snapshot,
    status, suggested_total_note, update_settings,
};
use cominorsa_domain::{
    BillingMilestone, BillingSummary, CaseFile, CaseRepository, LeadRepository, ProspectKind,
    ProspectRepository, Quote, QuoteRepository, SettingsRepository,
};
use cominorsa_importer::{ImportError, enrich, run as import};
use cominorsa_storage::SqliteLeads;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    args::{
        BackupCommand, BillingCommand, CaseCommand, Cli, Command, LeadCommand, ProspectCommand,
        QuoteCommand, QuoteContentArgs, SettingsArgs, SettingsCommand, StepCommand,
    },
    views,
};

const DATABASE_PATH_VARIABLE: &str = "COMINORSA_DB_PATH";

/// A command's result, as JSON and as text for a person.
pub struct Reply {
    pub data: Value,
    pub text: String,
}

impl Reply {
    fn new(data: Value, text: impl Into<String>) -> Self {
        Self {
            data,
            text: text.into(),
        }
    }
}

/// How a failure leaves the process.
pub enum Failure {
    Usage { code: &'static str, message: String },
    App(AppError),
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl From<ImportError> for Failure {
    fn from(error: ImportError) -> Self {
        let message = error.to_string();

        Self::App(match error {
            ImportError::Storage(failure) => failure.into(),
            ImportError::Unreadable(_) => AppError::rejected("import_unreadable", message),
            ImportError::NoRows
            | ImportError::MissingColumn(_)
            | ImportError::InvalidValue { .. }
            | ImportError::Duplicate { .. } => AppError::rejected("import_invalid", message),
        })
    }
}

async fn existing_store(path: &Path) -> Result<SqliteLeads, AppError> {
    if !path.exists() {
        return Err(AppError::storage(
            "database_not_found",
            format!("No existe la base de datos {}", path.display()),
        ));
    }

    Ok(SqliteLeads::open(path).await?)
}

/// A store for a write. Writes keep the app's behavior: they migrate on open.
async fn writable_store(path: &Path) -> Result<SqliteLeads, AppError> {
    Ok(SqliteLeads::open(path).await?)
}

/// The path must be explicit. There is deliberately no default location, so
/// the CLI can never reach the app's real database by accident.
fn database(cli: &Cli) -> Result<PathBuf, Failure> {
    cli.db
        .clone()
        .or_else(|| env::var_os(DATABASE_PATH_VARIABLE).map(PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or(Failure::Usage {
            code: "missing_database",
            message: "Indica la base de datos con --db o COMINORSA_DB_PATH".to_owned(),
        })
}

pub async fn run(cli: Cli) -> Result<Reply, Failure> {
    let path = database(&cli)?;
    let ids = cli
        .id_seed
        .as_deref()
        .map_or_else(Ids::generated, Ids::seeded);
    let mut context = Context::new(Clock::fixed(cli.today, cli.now), ids);

    match cli.command {
        Command::Status => {
            let store = open_for_reading(&path).await?;
            let found = status(&store).await?;
            store.close().await;
            let text = format!(
                "Esquema: {}\nIntegridad: {}\nLeads: {}\nExpedientes: {}\nExpedientes activos: {}\nCotizaciones: {}\nProspectos: {}",
                found.schema_version,
                found.integrity_check,
                found.leads,
                found.cases,
                found.active_cases,
                found.quotes,
                found.prospects
            );
            Ok(Reply::new(views::status(&found), text))
        }
        Command::Export => {
            let store = open_for_reading(&path).await?;
            let state = snapshot(&store).await?;
            store.close().await;
            let data = views::snapshot(&state);
            let text = serde_json::to_string_pretty(&data).unwrap_or_default();
            Ok(Reply::new(data, text))
        }
        Command::Migrate => {
            let done = migrate_database(&path, &context.clock).await?;
            let copy = optional_path(done.safety_copy.as_deref());
            let text = if done.changed {
                format!("Esquema actualizado de {} a {}.", done.from, done.to)
            } else {
                format!("El esquema ya estaba en la versión {}.", done.to)
            };
            Ok(Reply::new(
                json!({
                    "changed": done.changed,
                    "from": done.from,
                    "to": done.to,
                    "safety_copy": copy,
                }),
                text,
            ))
        }
        Command::Agenda => {
            let store = open_for_reading(&path).await?;
            let today = context.clock.today();
            let groups = agenda(&store, today).await?;
            store.close().await;
            let text = format!(
                "Vencidos: {}\nHoy: {}\nPróximos: {}",
                groups.overdue.len(),
                groups.today.len(),
                groups.upcoming.len()
            );
            Ok(Reply::new(views::agenda(&groups, today), text))
        }
        Command::Lead(command) => lead(&path, command, &mut context).await,
        Command::Prospect(command) => prospect(&path, command, &mut context).await,
        Command::Case(command) => case(&path, command, &mut context).await,
        Command::Step(command) => step(&path, command, &context).await,
        Command::Billing(command) => billing(&path, command, &mut context).await,
        Command::Quote(command) => quote(&path, command, &mut context).await,
        Command::Settings(command) => settings(&path, command).await,
        Command::Backup(command) => backup(&path, command, &context).await,
    }
}

async fn lead(path: &Path, command: LeadCommand, context: &mut Context) -> Result<Reply, Failure> {
    let reply = match command {
        LeadCommand::Add {
            name,
            phone,
            service,
            ruc,
            owner,
            follow_up,
        } => {
            let store = writable_store(path).await?;
            let follow_up_on = match follow_up {
                Some(date) => date,
                None => {
                    let days = store.load().await.map_err(AppError::from)?.follow_up_days();
                    next_follow_up(context.clock.today(), days).to_string()
                }
            };
            let values = FormValues {
                name,
                phone,
                ruc: ruc.unwrap_or_default(),
                service,
                owner: owner.unwrap_or_default(),
                follow_up_on,
            };
            let added = add_lead(&store, values, context).await?;
            store.close().await;
            let text = format!("Lead creado: {} ({})", added.lead.name(), added.lead.id());
            Reply::new(
                json!({"created": added.created, "lead": views::lead(&added.lead)}),
                text,
            )
        }
        LeadCommand::List { query } => {
            let store = open_for_reading(path).await?;
            let leads = store.list().await.map_err(AppError::from)?;
            store.close().await;
            let query = query.unwrap_or_default();
            let found: Vec<_> = leads
                .iter()
                .filter(|lead| matches_query(lead, &query))
                .collect();
            let text = found
                .iter()
                .map(|lead| format!("{} {} ({})", lead.id(), lead.name(), lead.stage().label()))
                .collect::<Vec<_>>()
                .join("\n");
            let data = found
                .iter()
                .map(|lead| views::lead(lead))
                .collect::<Vec<_>>();
            Reply::new(json!({"leads": data}), text)
        }
        LeadCommand::Show { id } => {
            let store = open_for_reading(path).await?;
            let lead = find_lead(&store, id).await?;
            store.close().await;
            let text = format!("{} ({})", lead.name(), lead.stage().label());
            Reply::new(json!({"lead": views::lead(&lead)}), text)
        }
        LeadCommand::Summary => {
            let store = open_for_reading(path).await?;
            let leads = store.list().await.map_err(AppError::from)?;
            store.close().await;
            let found = PipelineSummary::of(&leads, context.clock.today());
            let text = format!(
                "Activos: {}\nVencidos: {}\nHoy: {}\nFirmados: {}",
                found.active, found.overdue, found.due_today, found.signed
            );
            Reply::new(
                json!({
                    "active": found.active,
                    "overdue": found.overdue,
                    "due_today": found.due_today,
                    "signed": found.signed,
                }),
                text,
            )
        }
        LeadCommand::Advance { id } => {
            let store = existing_store(path).await?;
            let moved = advance_lead(&store, id, context).await?;
            store.close().await;
            let text = match &moved.case {
                Some(_) => format!("{} firmó contrato; expediente abierto.", moved.lead.name()),
                None => format!(
                    "{} pasó a {}.",
                    moved.lead.name(),
                    moved.lead.stage().label()
                ),
            };
            Reply::new(
                json!({
                    "lead": views::lead(&moved.lead),
                    "case": moved.case.as_ref().map(views::case),
                }),
                text,
            )
        }
        LeadCommand::Assign { id, owner } => {
            let store = existing_store(path).await?;
            let lead = assign_lead(&store, id, &owner).await?;
            store.close().await;
            let text = format!("{} queda a cargo de {owner}.", lead.name());
            Reply::new(json!({"lead": views::lead(&lead)}), text)
        }
        LeadCommand::Whatsapp { id } => {
            let store = open_for_reading(path).await?;
            let lead = find_lead(&store, id).await?;
            let settings = store.load().await.map_err(AppError::from)?;
            store.close().await;
            let url = lead
                .whatsapp_url(settings.whatsapp_template())
                .ok_or_else(|| {
                    AppError::rejected(
                        "no_whatsapp_number",
                        "Ese teléfono no sirve para abrir un chat",
                    )
                })?;
            Reply::new(json!({"url": url}), url.clone())
        }
    };

    Ok(reply)
}

fn optional_path(path: Option<&Path>) -> Option<String> {
    path.map(|path| path.display().to_string())
}

fn parse_kind(code: &str) -> Result<ProspectKind, AppError> {
    ProspectKind::from_code(code)
        .ok_or_else(|| AppError::rejected("invalid_kind", "Indica company o person"))
}

async fn prospect(
    path: &Path,
    command: ProspectCommand,
    context: &mut Context,
) -> Result<Reply, Failure> {
    let reply = match command {
        ProspectCommand::Import { companies, people } => {
            let report = import(path, &companies, &people).await?;
            let text = format!(
                "Prospectos nuevos: {}\nProspectos totales: {}",
                report.added, report.total
            );
            Reply::new(
                json!({
                    "added": report.added,
                    "total": report.total,
                    "backup": optional_path(report.backup.as_deref()),
                }),
                text,
            )
        }
        ProspectCommand::Enrich { sunat } => {
            let report = enrich(path, &sunat).await?;
            let text = format!("Empresas actualizadas: {}", report.updated);
            Reply::new(
                json!({
                    "updated": report.updated,
                    "backup": report.backup.display().to_string(),
                }),
                text,
            )
        }
        ProspectCommand::Search { query, limit } => {
            let store = open_for_reading(path).await?;
            let found = store.search(&query, limit).await.map_err(AppError::from)?;
            store.close().await;
            let text = found
                .iter()
                .map(|prospect| prospect.name.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let data = found.iter().map(views::prospect).collect::<Vec<_>>();
            Reply::new(json!({"prospects": data}), text)
        }
        ProspectCommand::Count => {
            let store = open_for_reading(path).await?;
            let total = store.count().await.map_err(AppError::from)?;
            store.close().await;
            Reply::new(json!({"total": total}), format!("Prospectos: {total}"))
        }
        ProspectCommand::Convert {
            name,
            phone,
            kind,
            service,
            owner,
            follow_up,
        } => {
            let store = existing_store(path).await?;
            let conversion = ProspectConversion {
                name,
                kind: kind.as_deref().map(parse_kind).transpose()?,
                phone,
                service,
                owner,
                follow_up_on: follow_up,
            };
            let added = convert_prospect(&store, conversion, context).await?;
            store.close().await;
            let text = format!("Lead creado: {} ({})", added.lead.name(), added.lead.id());
            Reply::new(
                json!({"created": added.created, "lead": views::lead(&added.lead)}),
                text,
            )
        }
    };

    Ok(reply)
}

async fn case(path: &Path, command: CaseCommand, context: &mut Context) -> Result<Reply, Failure> {
    let store = match command {
        CaseCommand::Close { .. } => existing_store(path).await?,
        CaseCommand::List | CaseCommand::Show { .. } => open_for_reading(path).await?,
    };
    let reply = match command {
        CaseCommand::List => {
            let cases = store.active_cases().await.map_err(AppError::from)?;
            let text = cases
                .iter()
                .map(|file| format!("{} {}", file.case.id(), file.case.client_name()))
                .collect::<Vec<_>>()
                .join("\n");
            let data = cases.iter().map(views::case).collect::<Vec<_>>();
            Reply::new(json!({"cases": data}), text)
        }
        CaseCommand::Show { id } => {
            let file = find_stored_case(&store, id).await?;
            let quotes = store.quotes().await.map_err(AppError::from)?;
            let summary = BillingSummary::of(&file);
            let text = format!(
                "{}: {}",
                file.case.client_name(),
                cominorsa_application::case_progress(&file)
            );
            Reply::new(
                json!({
                    "case": views::case(&file),
                    "billing": {
                        "total_cents": summary.total.map(|total| total.cents()),
                        "collected_cents": summary.collected.cents(),
                        "pending_cents": summary.pending.cents(),
                        "gap_warning": milestone_gap_warning(&file),
                        "total_note": suggested_total_note(&file, &quotes),
                        "close_warning": close_warning(&file),
                    },
                }),
                text,
            )
        }
        CaseCommand::Close { id } => {
            let closing = close_case(&store, id, context).await?;
            let text = if closing.closed {
                closing_notice(closing.renewal.as_ref())
            } else {
                "El expediente ya estaba cerrado.".to_owned()
            };
            Reply::new(
                json!({
                    "closed": closing.closed,
                    "renewal": closing.renewal.as_ref().map(views::lead),
                }),
                text,
            )
        }
    };
    store.close().await;

    Ok(reply)
}

async fn step(path: &Path, command: StepCommand, context: &Context) -> Result<Reply, Failure> {
    let store = existing_store(path).await?;
    let today = context.clock.today();
    let reply = match command {
        StepCommand::Done { id } => {
            let changed = set_step_done(&store, id, true, today).await?;
            Reply::new(json!({"changed": changed}), "Paso marcado como hecho.")
        }
        StepCommand::Undo { id } => {
            let changed = set_step_done(&store, id, false, today).await?;
            Reply::new(json!({"changed": changed}), "Paso marcado como pendiente.")
        }
        StepCommand::Date { id, date } => {
            let step = reschedule_step(&store, id, &date).await?;
            Reply::new(
                json!({"step": views::step(&step)}),
                "Fecha del paso guardada.",
            )
        }
    };
    store.close().await;

    Ok(reply)
}

async fn set_paid(
    store: &SqliteLeads,
    id: Uuid,
    paid: bool,
    context: &mut Context,
) -> Result<bool, AppError> {
    let (file, milestone) = find_milestone(store, id).await?;
    if milestone.is_paid() == paid {
        return Ok(false);
    }

    let draft = MilestoneValues::default();
    apply_billing(
        store,
        &file,
        BillingEdit::TogglePaid(milestone),
        &draft,
        context,
    )
    .await
}

/// One field of a milestone edit, validated without saving.
fn revised(
    file: &CaseFile,
    current: BillingMilestone,
    edit: BillingEdit,
    context: &Context,
) -> Result<BillingMilestone, AppError> {
    let draft = MilestoneValues::default();

    match billing_change(file, edit, &draft, context.clock.today())? {
        BillingChange::Save(next) => Ok(next),
        _ => Ok(current),
    }
}

/// Validates every field before saving once, so a refused edit stores nothing.
async fn edit_milestone(
    store: &SqliteLeads,
    id: Uuid,
    title: Option<String>,
    amount: Option<String>,
    due: Option<String>,
    context: &Context,
) -> Result<(), AppError> {
    let (file, mut milestone) = find_milestone(store, id).await?;

    if let Some(value) = title {
        let edit = BillingEdit::Title(milestone.clone(), value);
        milestone = revised(&file, milestone, edit, context)?;
    }
    if let Some(value) = amount {
        let edit = BillingEdit::Amount(milestone.clone(), value);
        milestone = revised(&file, milestone, edit, context)?;
    }
    if let Some(value) = due {
        let edit = BillingEdit::DueDate(milestone.clone(), value);
        milestone = revised(&file, milestone, edit, context)?;
    }

    save_billing(store, BillingChange::Save(milestone)).await
}

async fn billing(
    path: &Path,
    command: BillingCommand,
    context: &mut Context,
) -> Result<Reply, Failure> {
    let store = existing_store(path).await?;
    let draft = MilestoneValues::default();
    let reply = match command {
        BillingCommand::Total { case, amount } => {
            let file = find_case(&store, case).await?;
            apply_billing(&store, &file, BillingEdit::Total(amount), &draft, context).await?;
            Reply::new(json!({"changed": true}), "Total del contrato guardado.")
        }
        BillingCommand::Plan { case } => {
            let file = find_case(&store, case).await?;
            apply_billing(&store, &file, BillingEdit::Suggest, &draft, context).await?;
            let saved = find_case(&store, case).await?;
            let data = saved
                .milestones
                .iter()
                .map(views::milestone)
                .collect::<Vec<_>>();
            Reply::new(json!({"milestones": data}), "Plan sugerido aplicado.")
        }
        BillingCommand::Add {
            case,
            title,
            amount,
            due,
        } => {
            let file = find_case(&store, case).await?;
            let values = MilestoneValues {
                title,
                amount,
                due_on: due,
            };
            let changed = apply_billing(&store, &file, BillingEdit::Add, &values, context).await?;
            let saved = find_case(&store, case).await?;
            let data = saved
                .milestones
                .iter()
                .filter(|new| !file.milestones.contains(new))
                .map(views::milestone)
                .collect::<Vec<_>>();
            Reply::new(
                json!({"changed": changed, "milestones": data}),
                "Hito agregado.",
            )
        }
        BillingCommand::Edit {
            id,
            title,
            amount,
            due,
        } => {
            edit_milestone(&store, id, title, amount, due, context).await?;
            Reply::new(json!({"changed": true}), "Hito guardado.")
        }
        BillingCommand::Pay { id } => {
            let changed = set_paid(&store, id, true, context).await?;
            Reply::new(json!({"changed": changed}), "Hito marcado como pagado.")
        }
        BillingCommand::Unpay { id } => {
            let changed = set_paid(&store, id, false, context).await?;
            Reply::new(json!({"changed": changed}), "Hito marcado como pendiente.")
        }
        BillingCommand::Delete { id } => {
            let (file, _) = find_milestone(&store, id).await?;
            apply_billing(&store, &file, BillingEdit::Delete(id), &draft, context).await?;
            Reply::new(json!({"changed": true}), "Hito eliminado.")
        }
    };
    store.close().await;

    Ok(reply)
}

fn overlay(values: &mut QuoteValues, content: QuoteContentArgs) {
    if let Some(date) = content.issued_on {
        values.issued_on = date;
    }
    if let Some(date) = content.valid_until {
        values.valid_until = date;
    }
    if let Some(notes) = content.notes {
        values.notes = notes;
    }
    if !content.line.is_empty() {
        values.lines = content.line;
    }
}

fn saved_quote(quote: &Quote, notice: String) -> Reply {
    Reply::new(json!({"quote": views::quote(quote)}), notice)
}

async fn quote(
    path: &Path,
    command: QuoteCommand,
    context: &mut Context,
) -> Result<Reply, Failure> {
    let reads = matches!(
        command,
        QuoteCommand::List { .. } | QuoteCommand::Show { .. } | QuoteCommand::Pdf { .. }
    );
    let store = if reads {
        open_for_reading(path).await?
    } else {
        existing_store(path).await?
    };
    let today = context.clock.today();
    let blank = QuoteValues::default();
    let reply = match command {
        QuoteCommand::Create { lead, content } => {
            let lead = find_lead(&store, lead).await?;
            let mut values = QuoteValues::starting_on(today);
            values.lines = Vec::<QuoteLineValues>::new();
            overlay(&mut values, content);
            let saved = save_quote_edit(&store, &lead, QuoteEdit::Save, &values, context).await?;
            saved_quote(&saved.quote, saved.notice)
        }
        QuoteCommand::Edit { quote, content } => {
            let quote = find_quote(&store, &quote).await?;
            let lead = find_lead(&store, quote.lead_id()).await?;
            let mut values = QuoteValues::of_quote(&quote);
            overlay(&mut values, content);
            let saved = save_quote_edit(&store, &lead, QuoteEdit::Save, &values, context).await?;
            saved_quote(&saved.quote, saved.notice)
        }
        QuoteCommand::List { lead } => {
            let quotes = store.quotes().await.map_err(AppError::from)?;
            let found = match lead {
                Some(lead) => lead_quotes(&quotes, lead),
                None => quotes,
            };
            let text = found
                .iter()
                .map(|quote| format!("{} {}", quote.number(), quote.status().label()))
                .collect::<Vec<_>>()
                .join("\n");
            let data = found.iter().map(views::quote).collect::<Vec<_>>();
            Reply::new(json!({"quotes": data}), text)
        }
        QuoteCommand::Show { quote } => {
            let quote = find_quote(&store, &quote).await?;
            let text = format!("{} {}", quote.number(), quote.status().label());
            saved_quote(&quote, text)
        }
        QuoteCommand::Send {
            quote,
            allow_blank_company,
        } => {
            let quote = find_quote(&store, &quote).await?;
            let lead = find_lead(&store, quote.lead_id()).await?;
            let saved = send_quote(&store, &lead, quote, allow_blank_company, today).await?;
            saved_quote(&saved.quote, saved.notice)
        }
        QuoteCommand::Accept { quote } => {
            let quote = find_quote(&store, &quote).await?;
            let lead = find_lead(&store, quote.lead_id()).await?;
            let saved =
                save_quote_edit(&store, &lead, QuoteEdit::Accept(quote), &blank, context).await?;
            saved_quote(&saved.quote, saved.notice)
        }
        QuoteCommand::Reject { quote } => {
            let quote = find_quote(&store, &quote).await?;
            let lead = find_lead(&store, quote.lead_id()).await?;
            let saved =
                save_quote_edit(&store, &lead, QuoteEdit::Reject(quote), &blank, context).await?;
            saved_quote(&saved.quote, saved.notice)
        }
        QuoteCommand::Version { quote } => {
            let quote = find_quote(&store, &quote).await?;
            let lead = find_lead(&store, quote.lead_id()).await?;
            let saved =
                save_quote_edit(&store, &lead, QuoteEdit::NewVersion(quote), &blank, context)
                    .await?;
            saved_quote(&saved.quote, saved.notice)
        }
        QuoteCommand::Pdf { quote, out } => {
            let quote = find_quote(&store, &quote).await?;
            let saved = save_quote_pdf(&store, &quote, &out).await?;
            let path = saved.display().to_string();
            Reply::new(json!({"path": path}), format!("PDF guardado en {path}"))
        }
    };
    store.close().await;

    Ok(reply)
}

fn settings_change(arguments: SettingsArgs) -> SettingsChange {
    SettingsChange {
        theme: arguments.theme,
        follow_up_days: arguments.follow_up_days,
        whatsapp_template: arguments.whatsapp_template,
        igv: arguments.igv,
        backup_directory: arguments.backup_directory,
        company_name: arguments.company_name,
        company_ruc: arguments.company_ruc,
        company_address: arguments.company_address,
        company_phone: arguments.company_phone,
        company_email: arguments.company_email,
        add_members: arguments.add_member,
        remove_members: arguments.remove_member,
    }
}

async fn settings(path: &Path, command: SettingsCommand) -> Result<Reply, Failure> {
    let reply = match command {
        SettingsCommand::Show => {
            let store = open_for_reading(path).await?;
            let saved = store.load().await.map_err(AppError::from)?;
            store.close().await;
            let data = views::settings(&saved);
            let text = serde_json::to_string_pretty(&data).unwrap_or_default();
            Reply::new(data, text)
        }
        SettingsCommand::Set(arguments) => {
            let store = writable_store(path).await?;
            let saved = update_settings(&store, settings_change(*arguments)).await?;
            store.close().await;
            Reply::new(
                json!({"settings": views::settings(&saved)}),
                "Configuración guardada.",
            )
        }
    };

    Ok(reply)
}

async fn backup(path: &Path, command: BackupCommand, context: &Context) -> Result<Reply, Failure> {
    let store = existing_store(path).await?;
    let reply = match command {
        BackupCommand::Create { folder } => {
            let saved = run_backup(&store, folder.as_deref(), context).await?;
            store.close().await;
            let path = saved.display().to_string();
            Reply::new(json!({"path": path}), format!("Copia creada en {path}"))
        }
        BackupCommand::Restore { file } => {
            let restored = restore_backup(store, path, &file, &context.clock).await?;
            restored.store.close().await;
            let safety = restored.safety_copy.display().to_string();
            Reply::new(
                json!({"safety_copy": safety}),
                format!("Copia restaurada. Datos anteriores en {safety}"),
            )
        }
    };

    Ok(reply)
}
