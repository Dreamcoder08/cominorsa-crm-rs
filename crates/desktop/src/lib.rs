//! Desktop adapter: Dioxus views over the application layer.

use std::{
    env,
    path::{Path, PathBuf},
};

mod billing_view;
mod cases_view;
mod presentation;
mod quote_view;
mod settings_view;
mod views;

use cominorsa_application::{auto_backup, open_at_startup, record_consent, revoke_consent};
use cominorsa_domain::{
    Case, CaseFile, CaseRepository, CaseStep, Lead, LeadRepository, Prospect, ProspectRepository,
    Quote, QuoteRepository, Settings, SettingsRepository,
};
use cominorsa_storage::SqliteLeads;
use dioxus::{
    desktop::{icon_from_memory, tao::window::Icon},
    prelude::*,
};
use directories::ProjectDirs;
use uuid::Uuid;

pub use billing_view::BillingPanel;
pub use cases_view::{CaseDetail, Cases, CloseStep};
pub use cominorsa_application::{
    AgendaGroups, AgendaItem, AppError, BillingChange, BillingEdit, Clock, Context, FollowUp,
    FormValues, MilestoneValues, PROSPECT_PAGE_SIZE, PdfTarget, PipelineSummary, QUOTE_TAX_NOTE,
    QuoteChange, QuoteEdit, QuoteLineValues, QuoteValues, SettingsChange, accepted_notice,
    add_lead, apply_billing, assign_lead, billing_change, case_progress, close_warning,
    closed_case_label, closing_notice, company_lines, company_warning, create_backup, format_date,
    format_igv_rate, format_soles, format_timestamp, igv_input, igv_label, lead_quotes,
    matches_query, milestone_gap_warning, milestone_status_label, money_input, next_follow_up,
    owner_options, parse_form, parse_step_date, preview_message, profile_label, quote_change,
    quote_file_name, quote_notice, quote_pdf, quote_preview, renewal_note, reschedule_step,
    responsible_label, restore_backup, run_backup, save_advance, save_billing, save_close,
    save_quote, save_quote_edit, save_quote_pdf, send_quote, set_step_done, step_status_label,
    suggested_total_note, today_in_peru, toggled_step, update_settings, write_pdf,
};
pub use presentation::{auto_backup_label, consent_label, quote_status_class, tax_status_class};
pub use quote_view::QuotePanel;
pub use settings_view::{SaveState, SettingsView};
pub use views::{Agenda, ConsentEdit, LeadForm, Prospects, Section, Shell, Workspace};

const STYLE: &str = include_str!("style.css");
const ICON: &[u8] = include_bytes!("../assets/icon.png");
const DATABASE_PATH_VARIABLE: &str = "COMINORSA_DB_PATH";

pub fn window_icon() -> Result<Icon, String> {
    icon_from_memory(ICON).map_err(|error| error.to_string())
}

fn database_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os(DATABASE_PATH_VARIABLE) {
        return Ok(PathBuf::from(path));
    }

    ProjectDirs::from("pe", "COMINORSA", "CRM")
        .map(|dirs| dirs.data_dir().join("crm.db"))
        .ok_or_else(|| "No se encontró la carpeta de datos".to_owned())
}

async fn restore_copy(store: SqliteLeads, backup: PathBuf) -> Result<SqliteLeads, String> {
    let database = database_path()?;

    restore_backup(store, &database, &backup, &Clock::system())
        .await
        .map(|restored| restored.store)
        .map_err(|error| error.to_string())
}

/// Loads settings before the window opens, so the saved theme never flashes.
pub fn load_initial_settings() -> Settings {
    database_path().map_or_else(|_| Settings::default(), |path| initial_settings(&path))
}

/// The settings stored at `path`, or the defaults when they cannot be read.
/// This only reads: the window reports what `open_database` finds.
pub fn initial_settings(path: &Path) -> Settings {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Settings::default();
    };

    runtime.block_on(async {
        let Ok(store) = SqliteLeads::open_read_only(path).await else {
            return Settings::default();
        };
        let settings = store.load().await.unwrap_or_default();
        store.close().await;
        settings
    })
}

async fn open_store() -> Result<SqliteLeads, String> {
    open_database(&database_path()?).await
}

/// Opens the app's database, creating or upgrading it safely. The error is what
/// the window shows, with the location of the copy when an upgrade fails.
pub async fn open_database(path: &Path) -> Result<SqliteLeads, String> {
    open_at_startup(path, &Clock::system())
        .await
        .map_err(|error| error.to_string())
}

/// Runs the automatic backup use case once on `database`, as the app does
/// after it opens it, and returns what Settings shows about it.
pub async fn startup_backup(store: &SqliteLeads, database: &Path) -> String {
    auto_backup_label(&auto_backup(store, database, &Context::system(), false).await)
}

/// What a person changed on one case step.
enum StepChange {
    Done(bool),
    Due(String),
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
    let mut auto_backup_notice = use_signal(String::new);
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
                let backup_store = opened.clone();
                store.set(Some(opened));
                leads.set(list);
                cases.set(open);
                settings.set(saved);
                quotes.set(stored);
                form.set(new_form());
                // The window is already usable; the copy reports when it ends.
                spawn(async move {
                    let notice = match database_path() {
                        Ok(database) => startup_backup(&backup_store, &database).await,
                        Err(error) => error,
                    };
                    auto_backup_notice.set(notice);
                    if let Ok(saved) = backup_store.load().await
                        && saved.last_backup_at() != settings.read().last_backup_at()
                    {
                        settings.set(saved);
                        settings_generation += 1;
                    }
                });
            }
            Err(message) => startup_error.set(Some(message)),
        }
    });

    let mut save_consent = move |edit: ConsentEdit| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                if edit.revoke {
                    revoke_consent(&repository, edit.id, &edit.source, today_in_peru()).await?;
                } else {
                    record_consent(&repository, edit.id, &edit.source, today_in_peru()).await?;
                }
                Ok::<_, AppError>(repository.list().await?)
            };

            match saved.await {
                Ok(list) => {
                    leads.set(list);
                    action_error.set(String::new());
                }
                Err(failure) => action_error.set(failure.to_string()),
            }
            saving.set(false);
        });
    };

    let mut add_from_form = move || {
        let Some(repository) = store.read().clone() else {
            form_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };
        let values = form();

        saving.set(true);
        spawn(async move {
            let saved = async {
                add_lead(&repository, values, &mut Context::system()).await?;
                Ok::<_, AppError>(repository.list().await?)
            };

            match saved.await {
                Ok(list) => {
                    leads.set(list);
                    form_error.set(String::new());
                    form.set(new_form());
                    panel_open.set(false);
                }
                Err(failure) => form_error.set(failure.to_string()),
            }
            saving.set(false);
        });
    };

    let mut assign = move |id: Uuid, owner: String| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                assign_lead(&repository, id, &owner).await?;
                Ok::<_, AppError>(repository.list().await?)
            };

            match saved.await {
                Ok(list) => {
                    leads.set(list);
                    action_error.set(String::new());
                }
                Err(failure) => action_error.set(failure.to_string()),
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
                save_advance(&repository, lead, today_in_peru())
                    .await
                    .map_err(|error| error.to_string())?;
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

    let mut save_step = move |id: Uuid, change: StepChange| {
        let Some(repository) = store.read().clone() else {
            action_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };

        saving.set(true);
        spawn(async move {
            let saved = async {
                match change {
                    StepChange::Done(done) => {
                        set_step_done(&repository, id, done, today_in_peru()).await?;
                    }
                    StepChange::Due(value) => {
                        reschedule_step(&repository, id, &value).await?;
                    }
                }
                Ok::<_, AppError>(repository.active_cases().await?)
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
        let draft = milestone_draft.read().clone();

        saving.set(true);
        spawn(async move {
            let saved = async {
                apply_billing(&repository, &file, edit, &draft, &mut Context::system()).await?;
                Ok::<_, AppError>(repository.active_cases().await?)
            };

            match saved.await {
                Ok(open) => {
                    cases.set(open);
                    action_error.set(String::new());
                    if clears_draft {
                        milestone_draft.set(MilestoneValues::starting_on(today_in_peru()));
                    }
                }
                Err(failure) => action_error.set(failure.to_string()),
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
                let renewal = save_close(&repository, case, today_in_peru())
                    .await
                    .map_err(|error| error.to_string())?;
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

    let mut run_quote_edit = move |lead: Lead, edit: QuoteEdit| {
        let Some(repository) = store.read().clone() else {
            quote_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };
        let values = quote_values.read().clone();

        saving.set(true);
        spawn(async move {
            let saved = async {
                let saved = match edit.clone() {
                    // The panel already asked about a blank company name.
                    QuoteEdit::Send(quote) => {
                        send_quote(&repository, &lead, quote, true, today_in_peru()).await?
                    }
                    edit => {
                        save_quote_edit(&repository, &lead, edit, &values, &mut Context::system())
                            .await?
                    }
                };
                let list = repository.quotes().await?;
                Ok::<_, AppError>((saved, list))
            };

            match saved.await {
                Ok((saved, list)) => {
                    quotes.set(list);
                    quote_error.set(String::new());
                    quote_notice_text.set(saved.notice);
                    confirming_send.set(None);
                    match edit {
                        QuoteEdit::Save => {
                            quote_values.set(QuoteValues::starting_on(today_in_peru()));
                        }
                        QuoteEdit::NewVersion(_) => {
                            quote_values.set(QuoteValues::of_quote(&saved.quote));
                        }
                        _ => {}
                    }
                }
                Err(failure) => quote_error.set(failure.to_string()),
            }
            saving.set(false);
        });
    };

    let mut save_pdf = move |quote: Quote| {
        let Some(repository) = store.read().clone() else {
            quote_error.set("La base de datos todavía no está lista".to_owned());
            return;
        };
        let database = match database_path() {
            Ok(database) => database,
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
                // The save dialog already asked before replacing a file.
                let target = PdfTarget {
                    path: file.path(),
                    database: &database,
                    overwrite: true,
                };
                match save_quote_pdf(&repository, &quote, target).await {
                    Ok(path) => {
                        quote_error.set(String::new());
                        quote_notice_text.set(format!("PDF guardado en {}", path.display()));
                    }
                    Err(failure) => quote_error.set(failure.to_string()),
                }
            }
            saving.set(false);
        });
    };

    let mut save_settings = move |next: Settings| {
        let change = SettingsChange::between(&settings.read(), &next);
        settings.set(next);
        let Some(repository) = store.read().clone() else {
            return;
        };

        save_state.set(SaveState::Saving);
        spawn(async move {
            save_state.set(match update_settings(&repository, change).await {
                Ok(_) => SaveState::Saved,
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
                    auto_backup_notice: auto_backup_notice(),
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
                            match run_backup(&repository, Some(&folder), &Context::system()).await {
                                Ok(path) => {
                                    if let Ok(saved) = repository.load().await {
                                        settings.set(saved);
                                    }
                                    save_state.set(SaveState::Saved);
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
                                let reopened = restore_copy(repository, backup).await?;
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
                        on_toggle: move |step: CaseStep| {
                            if saving() {
                                return;
                            }
                            save_step(step.id(), StepChange::Done(!step.is_done()));
                        },
                        on_due_date: move |(step, value): (CaseStep, String)| {
                            if saving() {
                                return;
                            }
                            save_step(step.id(), StepChange::Due(value));
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
                        assign(id, owner);
                    },
                    on_new: move |_| {
                        form_error.set(String::new());
                        panel_open.set(true);
                    },
                    on_consent: move |edit: ConsentEdit| {
                        if saving() {
                            return;
                        }
                        save_consent(edit);
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
                    add_from_form();
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
                        run_quote_edit(lead.clone(), edit);
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
