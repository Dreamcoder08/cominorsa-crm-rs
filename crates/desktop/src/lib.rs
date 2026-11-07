//! Desktop adapter: Dioxus views over the domain and storage crates.

use std::{env, fs, path::PathBuf};

mod billing_view;
mod cases_view;
mod presentation;
mod quote_flow;
mod quote_pdf;
mod quote_view;
mod settings_view;
mod views;

use cominorsa_domain::{
    BillingMilestone, BillingRepository, Case, CaseClosure, CaseFile, CaseRepository, CaseStep,
    DomainError, Lead, LeadRepository, Money, NewLead, NewMilestone, Prospect, ProspectRepository,
    Quote, QuoteRepository, Settings, SettingsRepository, Stage, procedure_template,
};
use cominorsa_storage::SqliteLeads;
use dioxus::{
    desktop::{icon_from_memory, tao::window::Icon},
    prelude::*,
};
use directories::ProjectDirs;
use time::{
    Date, OffsetDateTime,
    format_description::{BorrowedFormatItem, well_known::Iso8601},
    macros::{format_description, offset},
};
use uuid::Uuid;

pub use billing_view::BillingPanel;
pub use cases_view::{CaseDetail, Cases, CloseStep};
pub use presentation::{
    AgendaGroups, AgendaItem, FollowUp, PipelineSummary, QUOTE_TAX_NOTE, accepted_notice,
    case_progress, close_warning, closed_case_label, closing_notice, company_lines,
    company_warning, format_date, format_igv_rate, format_soles, format_timestamp, igv_input,
    igv_label, matches_query, milestone_gap_warning, milestone_status_label, money_input,
    next_follow_up, owner_options, profile_label, quote_file_name, quote_status_class,
    renewal_note, responsible_label, step_status_label, suggested_total_note, tax_status_class,
};
pub use quote_flow::{
    QuoteChange, QuoteEdit, QuoteLineValues, QuoteValues, lead_quotes, quote_change, quote_notice,
    quote_preview, save_quote,
};
pub use quote_pdf::{quote_pdf, write_pdf};
pub use quote_view::QuotePanel;
pub use settings_view::{SaveState, SettingsView};
pub use views::{Agenda, LeadForm, Prospects, Section, Shell, Workspace};

const STYLE: &str = include_str!("style.css");
const ICON: &[u8] = include_bytes!("../assets/icon.png");
const DATABASE_PATH_VARIABLE: &str = "COMINORSA_DB_PATH";
const PROSPECT_PAGE_SIZE: u32 = 100;
const FILE_STAMP: &[BorrowedFormatItem<'_>] =
    format_description!("[year][month][day]-[hour][minute][second]");
const MANUAL_REVIEW_SERVICE: &str = "REVISAR_MANUAL";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormValues {
    pub name: String,
    pub phone: String,
    pub ruc: String,
    pub service: String,
    pub owner: String,
    pub follow_up_on: String,
}

impl FormValues {
    /// Starts with a real value: an unset date input shows a greyed-out date
    /// that looks filled in but submits nothing.
    pub fn starting_on(follow_up_on: Date) -> Self {
        Self {
            follow_up_on: follow_up_on.to_string(),
            ..Self::default()
        }
    }

    /// Phone stays blank: sources carry no contact data. Only confirmed RUCs prefill.
    pub fn from_prospect(prospect: &Prospect, follow_up_on: Date) -> Self {
        let service = match prospect.potential_service.as_str() {
            MANUAL_REVIEW_SERVICE => "",
            service => service,
        };

        let ruc = prospect
            .tax
            .as_ref()
            .filter(|tax| !tax.approximate)
            .map_or_else(String::new, |tax| tax.ruc.clone());

        Self {
            name: prospect.name.clone(),
            ruc,
            service: service.to_owned(),
            ..Self::starting_on(follow_up_on)
        }
    }
}

/// The new-milestone form, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MilestoneValues {
    pub title: String,
    pub amount: String,
    pub due_on: String,
}

impl MilestoneValues {
    pub fn starting_on(due_on: Date) -> Self {
        Self {
            due_on: due_on.to_string(),
            ..Self::default()
        }
    }
}

/// What a person did in the billing panel, before validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillingEdit {
    Total(String),
    Suggest,
    Add,
    Title(BillingMilestone, String),
    Amount(BillingMilestone, String),
    DueDate(BillingMilestone, String),
    TogglePaid(BillingMilestone),
    Delete(Uuid),
}

/// A validated billing change, ready to persist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillingChange {
    Total(Uuid, Money),
    Add(Vec<BillingMilestone>),
    Save(BillingMilestone),
    Delete(Uuid),
}

/// Validates one edit of the billing panel through the domain rules.
pub fn billing_change(
    file: &CaseFile,
    edit: BillingEdit,
    draft: &MilestoneValues,
    today: Date,
) -> Result<BillingChange, String> {
    let message = |error: DomainError| error.to_string();

    match edit {
        BillingEdit::Total(text) => Money::parse(&text)
            .map(|total| BillingChange::Total(file.case.id(), total))
            .map_err(message),
        BillingEdit::Suggest => file
            .suggested_milestones(today)
            .map(BillingChange::Add)
            .map_err(message),
        BillingEdit::Add => {
            let amount = Money::parse(&draft.amount).map_err(message)?;
            let milestone = BillingMilestone::create(NewMilestone {
                case_id: file.case.id(),
                position: file.next_milestone_position(),
                title: draft.title.clone(),
                amount,
                due_on: parse_milestone_date(&draft.due_on)?,
            })
            .map_err(message)?;

            Ok(BillingChange::Add(vec![milestone]))
        }
        BillingEdit::Title(mut milestone, title) => {
            milestone.rename(&title).map_err(message)?;
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::Amount(mut milestone, text) => {
            let amount = Money::parse(&text).map_err(message)?;
            milestone.set_amount(amount).map_err(message)?;
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::DueDate(mut milestone, value) => {
            milestone.reschedule(parse_milestone_date(&value)?);
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::TogglePaid(mut milestone) => {
            if milestone.is_paid() {
                milestone.mark_unpaid();
            } else {
                milestone.mark_paid(today);
            }
            Ok(BillingChange::Save(milestone))
        }
        BillingEdit::Delete(id) => Ok(BillingChange::Delete(id)),
    }
}

fn parse_milestone_date(value: &str) -> Result<Date, String> {
    Date::parse(value, &Iso8601::DATE).map_err(|_| "Ingresa la fecha del hito".to_owned())
}

pub async fn save_billing<R: BillingRepository>(
    repository: &R,
    change: BillingChange,
) -> Result<(), String> {
    let saved = match change {
        BillingChange::Total(case_id, total) => {
            repository.save_contract_total(case_id, total).await
        }
        BillingChange::Add(milestones) => repository.add_milestones(&milestones).await,
        BillingChange::Save(milestone) => repository.save_milestone(&milestone).await,
        BillingChange::Delete(id) => repository.delete_milestone(id).await,
    };

    saved.map_err(|error| error.to_string())
}

/// Closes the case and creates its renewal lead in one atomic operation.
/// Returns the lead created, and `None` for a service that does not recur or a
/// repeat, which changes nothing.
pub async fn save_close<R>(repository: &R, case: Case, today: Date) -> Result<Option<Lead>, String>
where
    R: LeadRepository + CaseRepository,
{
    let leads = repository.list().await.map_err(|error| error.to_string())?;
    let source = leads
        .iter()
        .find(|lead| lead.id() == case.lead_id())
        .ok_or_else(|| "No se encontró el lead del expediente".to_owned())?;
    let closure = CaseClosure::of(case, source, today);
    let closed = repository
        .close_case(&closure)
        .await
        .map_err(|error| error.to_string())?;

    Ok(closure.renewal.filter(|_| closed))
}

pub fn parse_form(values: FormValues) -> Result<Lead, String> {
    let follow_up_on = Date::parse(&values.follow_up_on, &Iso8601::DATE)
        .map_err(|_| "Ingresa la fecha de seguimiento".to_owned())?;

    Lead::create(NewLead {
        name: values.name,
        phone: values.phone,
        ruc: Some(values.ruc),
        service: values.service,
        owner: Some(values.owner),
        follow_up_on,
    })
    .map_err(|error| error.to_string())
}

pub fn parse_step_date(value: &str) -> Result<Date, String> {
    Date::parse(value, &Iso8601::DATE).map_err(|_| "Ingresa la fecha del paso".to_owned())
}

/// Saves one pipeline move. Reaching the final stage opens the case in the same
/// atomic operation, with suggested step dates counted from `today` and the
/// total of an accepted quote as the suggested contract total.
pub async fn save_advance<R>(repository: &R, mut lead: Lead, today: Date) -> Result<(), String>
where
    R: LeadRepository + CaseRepository + QuoteRepository,
{
    lead.advance().map_err(|error| error.to_string())?;

    if lead.stage() != Stage::ContractSigned {
        return repository
            .save(&lead)
            .await
            .map_err(|error| error.to_string());
    }

    let template = procedure_template(lead.service());
    let mut opening = CaseFile::open(&lead, today, &template).map_err(|error| error.to_string())?;
    let quotes = repository
        .quotes()
        .await
        .map_err(|error| error.to_string())?;
    opening
        .suggest_total_from(&quotes)
        .map_err(|error| error.to_string())?;

    repository
        .sign_and_open(&lead, &opening)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn window_icon() -> Result<Icon, String> {
    icon_from_memory(ICON).map_err(|error| error.to_string())
}

/// Peru has a fixed UTC-5 offset and no daylight saving time.
fn today_in_peru() -> Date {
    OffsetDateTime::now_utc().to_offset(offset!(-5)).date()
}

fn database_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os(DATABASE_PATH_VARIABLE) {
        return Ok(PathBuf::from(path));
    }

    ProjectDirs::from("pe", "COMINORSA", "CRM")
        .map(|dirs| dirs.data_dir().join("crm.db"))
        .ok_or_else(|| "No se encontró la carpeta de datos".to_owned())
}

fn file_stamp() -> String {
    OffsetDateTime::now_utc()
        .to_offset(offset!(-5))
        .format(FILE_STAMP)
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string())
}

/// Writes a timestamped copy to the chosen folder and returns its path.
async fn create_backup(store: SqliteLeads, folder: String) -> Result<PathBuf, String> {
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let path = PathBuf::from(folder).join(format!("crm-{}.db", file_stamp()));

    store
        .backup_to(&path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(path)
}

async fn restore_backup(store: SqliteLeads, backup: PathBuf) -> Result<SqliteLeads, String> {
    let database = database_path()?;
    let safety = database.with_extension(format!("antes-de-restaurar-{}.db", file_stamp()));

    store
        .restore(&database, &backup, &safety)
        .await
        .map_err(|error| error.to_string())
}

/// Loads settings before the window opens, so the saved theme never flashes.
pub fn load_initial_settings() -> Settings {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Settings::default();
    };

    runtime.block_on(async {
        let Ok(store) = open_store().await else {
            return Settings::default();
        };
        let settings = store.load().await.unwrap_or_default();
        store.close().await;
        settings
    })
}

async fn open_store() -> Result<SqliteLeads, String> {
    let path = database_path()?;
    SqliteLeads::open(&path)
        .await
        .map_err(|error| error.to_string())
}

#[component]
pub fn App() -> Element {
    let mut store = use_signal(|| None::<SqliteLeads>);
    let mut leads = use_signal(Vec::<Lead>::new);
    let mut cases = use_signal(Vec::<CaseFile>::new);
    let mut selected_case = use_signal(|| None::<Uuid>);
    let mut action_error = use_signal(String::new);
    let mut startup_error = use_signal(|| None::<String>);
    let mut form_error = use_signal(String::new);
    let mut settings = use_signal(|| try_consume_context::<Settings>().unwrap_or_default());
    let mut backup_notice = use_signal(String::new);
    let mut save_state = use_signal(SaveState::default);
    let mut pending_restore = use_signal(|| None::<PathBuf>);
    let mut busy = use_signal(|| false);
    let mut settings_generation = use_signal(|| 0_u32);
    let new_form = move || {
        FormValues::starting_on(next_follow_up(
            today_in_peru(),
            settings.read().follow_up_days(),
        ))
    };
    let mut form = use_signal(new_form);
    let mut saving = use_signal(|| false);
    let mut panel_open = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut section = use_signal(|| Section::Pipeline);
    let mut prospect_query = use_signal(String::new);
    let mut milestone_draft = use_signal(|| MilestoneValues::starting_on(today_in_peru()));
    let mut confirming_close = use_signal(|| false);
    let mut case_notice = use_signal(String::new);
    let mut quotes = use_signal(Vec::<Quote>::new);
    let mut quote_lead = use_signal(|| None::<Uuid>);
    let mut quote_values = use_signal(|| QuoteValues::starting_on(today_in_peru()));
    let mut quote_error = use_signal(String::new);
    let mut quote_notice_text = use_signal(String::new);
    let mut confirming_send = use_signal(|| None::<Uuid>);
    let selected_file = move || {
        selected_case().and_then(|id| {
            cases
                .read()
                .iter()
                .find(|file| file.case.id() == id)
                .cloned()
        })
    };
    let mut reset_case_view = move || {
        action_error.set(String::new());
        case_notice.set(String::new());
        confirming_close.set(false);
        milestone_draft.set(MilestoneValues::starting_on(today_in_peru()));
    };

    let prospects = use_resource(move || async move {
        let query = prospect_query();
        let repository = store.read().clone()?;
        let page = async {
            let total = repository.count().await?;
            let found = repository.search(&query, PROSPECT_PAGE_SIZE).await?;
            Ok::<_, cominorsa_domain::RepositoryError>((total, found))
        };

        Some(page.await.map_err(|error| error.to_string()))
    });

    use_future(move || async move {
        let loaded = async {
            let opened = open_store().await?;
            let list = opened.list().await.map_err(|error| error.to_string())?;
            let open = opened
                .active_cases()
                .await
                .map_err(|error| error.to_string())?;
            let saved = opened.load().await.map_err(|error| error.to_string())?;
            let stored = opened.quotes().await.map_err(|error| error.to_string())?;
            Ok::<_, String>((opened, list, open, saved, stored))
        };

        match loaded.await {
            Ok((opened, list, open, saved, stored)) => {
                store.set(Some(opened));
                leads.set(list);
                cases.set(open);
                settings.set(saved);
                quotes.set(stored);
                form.set(new_form());
            }
            Err(message) => startup_error.set(Some(message)),
        }
    });

    let mut persist = move |lead: Lead, from_form: bool| {
        let Some(repository) = store.read().clone() else {
            form_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                repository.save(&lead).await?;
                repository.list().await
            };

            match saved.await {
                Ok(list) => {
                    leads.set(list);
                    form_error.set(String::new());
                    if from_form {
                        form.set(new_form());
                        panel_open.set(false);
                    }
                }
                Err(failure) => form_error.set(failure.to_string()),
            }
            saving.set(false);
        });
    };

    let mut advance = move |lead: Lead| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                save_advance(&repository, lead, today_in_peru()).await?;
                let list = repository.list().await.map_err(|error| error.to_string())?;
                let open = repository
                    .active_cases()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((list, open))
            };

            match saved.await {
                Ok((list, open)) => {
                    leads.set(list);
                    cases.set(open);
                    action_error.set(String::new());
                }
                Err(failure) => action_error.set(failure),
            }
            saving.set(false);
        });
    };

    let mut save_step = move |step: CaseStep| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                repository.save_step(&step).await?;
                repository.active_cases().await
            };

            match saved.await {
                Ok(open) => {
                    cases.set(open);
                    action_error.set(String::new());
                }
                Err(failure) => action_error.set(failure.to_string()),
            }
            saving.set(false);
        });
    };

    let mut save_billing_edit = move |file: CaseFile, edit: BillingEdit| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };
        let clears_draft = edit == BillingEdit::Add;
        let change = match billing_change(&file, edit, &milestone_draft.read(), today_in_peru()) {
            Ok(change) => change,
            Err(message) => {
                action_error.set(message);
                return;
            }
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                save_billing(&repository, change).await?;
                repository
                    .active_cases()
                    .await
                    .map_err(|error| error.to_string())
            };

            match saved.await {
                Ok(open) => {
                    cases.set(open);
                    action_error.set(String::new());
                    if clears_draft {
                        milestone_draft.set(MilestoneValues::starting_on(today_in_peru()));
                    }
                }
                Err(failure) => action_error.set(failure),
            }
            saving.set(false);
        });
    };

    let mut close_case = move |case: Case| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                let renewal = save_close(&repository, case, today_in_peru()).await?;
                let list = repository.list().await.map_err(|error| error.to_string())?;
                let open = repository
                    .active_cases()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((renewal, list, open))
            };

            match saved.await {
                Ok((renewal, list, open)) => {
                    leads.set(list);
                    cases.set(open);
                    action_error.set(String::new());
                    case_notice.set(closing_notice(renewal.as_ref()));
                    confirming_close.set(false);
                }
                Err(failure) => action_error.set(failure),
            }
            saving.set(false);
        });
    };

    let mut save_quote_edit = move |lead: Lead, edit: QuoteEdit| {
        let Some(repository) = store.read().clone() else {
            quote_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };
        let change = quote_change(
            &lead,
            edit.clone(),
            &quote_values.read(),
            &quotes.read(),
            &settings.read(),
            today_in_peru(),
        );
        let change = match change {
            Ok(change) => change,
            Err(message) => {
                quote_error.set(message);
                return;
            }
        };
        let case_is_open = cases
            .read()
            .iter()
            .any(|file| file.case.lead_id() == lead.id());

        saving.set(true);
        spawn(async move {
            let saved = async {
                let quote = save_quote(&repository, change).await?;
                let list = repository
                    .quotes()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((quote, list))
            };

            match saved.await {
                Ok((quote, list)) => {
                    quotes.set(list);
                    quote_error.set(String::new());
                    quote_notice_text.set(quote_notice(&edit, &quote, case_is_open));
                    confirming_send.set(None);
                    match edit {
                        QuoteEdit::Save => {
                            quote_values.set(QuoteValues::starting_on(today_in_peru()));
                        }
                        QuoteEdit::NewVersion(_) => quote_values.set(QuoteValues::of_quote(&quote)),
                        _ => {}
                    }
                }
                Err(failure) => quote_error.set(failure),
            }
            saving.set(false);
        });
    };

    let mut save_pdf = move |quote: Quote| {
        let company = settings.read().company().clone();
        let bytes = match quote_pdf(&quote, quote.document_company(&company)) {
            Ok(bytes) => bytes,
            Err(message) => {
                quote_error.set(message);
                return;
            }
        };

        saving.set(true);
        spawn(async move {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Guardar cotización en PDF")
                .set_file_name(quote_file_name(&quote))
                .add_filter("PDF", &["pdf"])
                .save_file()
                .await;

            if let Some(file) = picked {
                match write_pdf(file.path(), &bytes) {
                    Ok(path) => {
                        quote_error.set(String::new());
                        quote_notice_text.set(format!("PDF guardado en {}", path.display()));
                    }
                    Err(failure) => quote_error.set(failure),
                }
            }
            saving.set(false);
        });
    };

    let mut save_settings = move |next: Settings| {
        settings.set(next.clone());
        let Some(repository) = store.read().clone() else {
            return;
        };

        save_state.set(SaveState::Saving);
        spawn(async move {
            save_state.set(match repository.persist(&next).await {
                Ok(()) => SaveState::Saved,
                Err(failure) => SaveState::Failed(failure.to_string()),
            });
        });
    };

    let theme = settings.read().theme.code();

    rsx! {
        style { {STYLE} }
        div { class: "app", "data-theme": theme,
        if let Some(message) = startup_error() {
            p { class: "banner", role: "alert", "{message}" }
        }
        Shell {
            section: section(),
            on_section: move |next| {
                section.set(next);
                selected_case.set(None);
                reset_case_view();
            },
            if section() == Section::Settings {
                SettingsView {
                    key: "{settings_generation}",
                    settings: settings(),
                    database: database_path().map_or_else(|error| error, |path| path.display().to_string()),
                    save_state: save_state(),
                    backup_notice: backup_notice(),
                    pending_restore: pending_restore().map(|path: PathBuf| path.display().to_string()),
                    busy: busy(),
                    on_change: move |next: Settings| {
                        save_settings(next);
                        form.set(new_form());
                    },
                    on_pick_folder: move |_| {
                        spawn(async move {
                            let picked = rfd::AsyncFileDialog::new()
                                .set_title("Elegir carpeta para las copias")
                                .pick_folder()
                                .await;
                            if let Some(folder) = picked {
                                let mut next = settings();
                                next.set_backup_directory(&folder.path().display().to_string());
                                save_settings(next);
                            }
                        });
                    },
                    on_backup: move |_| {
                        let Some(repository) = store.read().clone() else {
                            return;
                        };
                        let Some(folder) = settings.read().backup_directory().map(str::to_owned) else {
                            return;
                        };
                        busy.set(true);
                        spawn(async move {
                            match create_backup(repository, folder).await {
                                Ok(path) => {
                                    let mut next = settings();
                                    next.record_backup(OffsetDateTime::now_utc().unix_timestamp());
                                    save_settings(next);
                                    backup_notice.set(format!("Copia creada en {}", path.display()));
                                }
                                Err(failure) => backup_notice.set(format!("No se pudo copiar: {failure}")),
                            }
                            busy.set(false);
                        });
                    },
                    on_pick_restore: move |_| {
                        spawn(async move {
                            let picked = rfd::AsyncFileDialog::new()
                                .set_title("Elegir copia de seguridad")
                                .add_filter("Copia de COMINORSA", &["db"])
                                .pick_file()
                                .await;
                            if let Some(file) = picked {
                                pending_restore.set(Some(file.path().to_path_buf()));
                            }
                        });
                    },
                    on_confirm_restore: move |_| {
                        let Some(backup) = pending_restore() else {
                            return;
                        };
                        let Some(repository) = store.read().clone() else {
                            return;
                        };
                        busy.set(true);
                        spawn(async move {
                            let restored = async {
                                let reopened = restore_backup(repository, backup).await?;
                                let list = reopened.list().await.map_err(|error| error.to_string())?;
                                let open = reopened.active_cases().await.map_err(|error| error.to_string())?;
                                let saved = reopened.load().await.map_err(|error| error.to_string())?;
                                let stored = reopened.quotes().await.map_err(|error| error.to_string())?;
                                Ok::<_, String>((reopened, list, open, saved, stored))
                            };

                            match restored.await {
                                Ok((reopened, list, open, saved, stored)) => {
                                    store.set(Some(reopened));
                                    leads.set(list);
                                    cases.set(open);
                                    quotes.set(stored);
                                    selected_case.set(None);
                                    settings.set(saved);
                                    settings_generation += 1;
                                    backup_notice.set("Copia restaurada".to_owned());
                                }
                                Err(failure) => backup_notice.set(format!("No se pudo restaurar: {failure}")),
                            }
                            pending_restore.set(None);
                            busy.set(false);
                        });
                    },
                    on_cancel_restore: move |_| pending_restore.set(None),
                }
            } else if section() == Section::Agenda {
                Agenda {
                    leads: leads(),
                    cases: cases(),
                    today: today_in_peru(),
                    whatsapp_template: settings.read().whatsapp_template().to_owned(),
                    on_open_pipeline: move |_| section.set(Section::Pipeline),
                    on_open_case: move |id: Uuid| {
                        reset_case_view();
                        selected_case.set(Some(id));
                        section.set(Section::Cases);
                    },
                }
            } else if section() == Section::Cases {
                if let Some(file) = selected_file() {
                    CaseDetail {
                        total_note: suggested_total_note(&file, &quotes.read()),
                        file,
                        today: today_in_peru(),
                        saving: saving(),
                        error: action_error(),
                        notice: case_notice(),
                        confirming_close: confirming_close(),
                        on_close: move |step: CloseStep| match step {
                            CloseStep::Ask => confirming_close.set(true),
                            CloseStep::Cancel => confirming_close.set(false),
                            CloseStep::Confirm => {
                                if let Some(file) = selected_file().filter(|_| !saving()) {
                                    close_case(file.case);
                                }
                            }
                        },
                        draft: milestone_draft(),
                        on_draft: move |values| milestone_draft.set(values),
                        on_billing: move |edit: BillingEdit| {
                            if let Some(file) = selected_file().filter(|_| !saving()) {
                                save_billing_edit(file, edit);
                            }
                        },
                        on_back: move |_| {
                            selected_case.set(None);
                            reset_case_view();
                        },
                        on_toggle: move |mut step: CaseStep| {
                            if saving() {
                                return;
                            }
                            if step.is_done() {
                                step.mark_undone();
                            } else {
                                step.mark_done(today_in_peru());
                            }
                            save_step(step);
                        },
                        on_due_date: move |(mut step, value): (CaseStep, String)| {
                            if saving() {
                                return;
                            }
                            match parse_step_date(&value) {
                                Ok(date) => {
                                    step.reschedule(date);
                                    save_step(step);
                                }
                                Err(message) => action_error.set(message),
                            }
                        },
                    }
                } else {
                    Cases {
                        cases: cases(),
                        notice: case_notice(),
                        on_open: move |id: Uuid| {
                            reset_case_view();
                            selected_case.set(Some(id));
                        },
                    }
                }
            } else if section() == Section::Pipeline {
                if !action_error().is_empty() {
                    p { class: "form-error", role: "alert", "{action_error}" }
                }
                Workspace {
                    leads: leads(),
                    today: today_in_peru(),
                    query: query(),
                    whatsapp_template: settings.read().whatsapp_template().to_owned(),
                    team: settings.read().team().to_vec(),
                    on_query: move |value| query.set(value),
                    on_assign: move |(id, owner): (Uuid, String)| {
                        if saving() {
                            return;
                        }
                        let Some(mut lead) = leads.read().iter().find(|lead| lead.id() == id).cloned() else {
                            return;
                        };
                        lead.assign(Some(&owner));
                        persist(lead, false);
                    },
                    on_new: move |_| {
                        form_error.set(String::new());
                        panel_open.set(true);
                    },
                    on_quote: move |id: Uuid| {
                        quote_error.set(String::new());
                        quote_notice_text.set(String::new());
                        confirming_send.set(None);
                        quote_values.set(QuoteValues::starting_on(today_in_peru()));
                        quote_lead.set(Some(id));
                    },
                    on_advance: move |id: Uuid| {
                        if saving() {
                            return;
                        }
                        let Some(lead) = leads.read().iter().find(|lead| lead.id() == id).cloned() else {
                            return;
                        };
                        advance(lead);
                    },
                }
            } else {
                match prospects() {
                    Some(Some(Ok((total, found)))) => rsx! {
                        Prospects {
                            prospects: found,
                            total,
                            query: prospect_query(),
                            on_query: move |value| prospect_query.set(value),
                            on_convert: move |prospect: Prospect| {
                                form.set(FormValues::from_prospect(&prospect, today_in_peru()));
                                form_error.set(String::new());
                                section.set(Section::Pipeline);
                                panel_open.set(true);
                            },
                        }
                    },
                    Some(Some(Err(message))) => rsx! {
                        p { class: "banner", role: "alert", "{message}" }
                    },
                    _ => rsx! {
                        p { class: "loading", "Cargando prospectos…" }
                    },
                }
            }
        }
        if panel_open() {
            LeadForm {
                values: form(),
                saving: saving(),
                error: form_error(),
                team: settings.read().team().to_vec(),
                on_change: move |values| form.set(values),
                on_submit: move |_| {
                    if saving() {
                        return;
                    }
                    match parse_form(form()) {
                        Ok(lead) => persist(lead, true),
                        Err(message) => form_error.set(message),
                    }
                },
                on_close: move |_| panel_open.set(false),
            }
        }
        if let Some(lead) = quote_lead().and_then(|id| leads.read().iter().find(|lead| lead.id() == id).cloned()) {
            QuotePanel {
                quotes: lead_quotes(&quotes.read(), lead.id()),
                lead: lead.clone(),
                values: quote_values(),
                company: settings.read().company().clone(),
                igv_rate: settings.read().igv_rate(),
                saving: saving(),
                error: quote_error(),
                notice: quote_notice_text(),
                confirming_send: confirming_send(),
                on_change: move |values| quote_values.set(values),
                on_edit: move |edit: QuoteEdit| {
                    if !saving() {
                        save_quote_edit(lead.clone(), edit);
                    }
                },
                on_load: move |quote: Quote| {
                    quote_error.set(String::new());
                    quote_values.set(QuoteValues::of_quote(&quote));
                },
                on_pdf: move |quote: Quote| {
                    if !saving() {
                        save_pdf(quote);
                    }
                },
                on_confirm_send: move |id: Option<Uuid>| confirming_send.set(id),
                on_close: move |_| quote_lead.set(None),
            }
        }
        }
    }
}
