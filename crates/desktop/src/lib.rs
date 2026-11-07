//! Desktop adapter: Dioxus views over the domain and storage crates.

use std::{env, fs, path::PathBuf};

mod presentation;
mod settings_view;
mod views;

use cominorsa_domain::{
    Lead, LeadRepository, NewLead, Prospect, ProspectRepository, Settings, SettingsRepository,
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

pub use presentation::{
    AgendaGroups, FollowUp, PipelineSummary, format_date, format_timestamp, matches_query,
    next_follow_up, owner_options, profile_label,
};
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

    /// Phone and RUC stay blank: prospect sources carry no contact data.
    pub fn from_prospect(prospect: &Prospect, follow_up_on: Date) -> Self {
        let service = match prospect.potential_service.as_str() {
            MANUAL_REVIEW_SERVICE => "",
            service => service,
        };

        Self {
            name: prospect.name.clone(),
            service: service.to_owned(),
            ..Self::starting_on(follow_up_on)
        }
    }
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
    let mut startup_error = use_signal(|| None::<String>);
    let mut form_error = use_signal(String::new);
    let mut settings = use_signal(|| try_consume_context::<Settings>().unwrap_or_default());
    let mut backup_notice = use_signal(String::new);
    let mut save_state = use_signal(SaveState::default);
    let mut pending_restore = use_signal(|| None::<PathBuf>);
    let mut busy = use_signal(|| false);
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
            let saved = opened.load().await.map_err(|error| error.to_string())?;
            Ok::<_, String>((opened, list, saved))
        };

        match loaded.await {
            Ok((opened, list, saved)) => {
                store.set(Some(opened));
                leads.set(list);
                settings.set(saved);
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
            on_section: move |next| section.set(next),
            if section() == Section::Settings {
                SettingsView {
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
                                let saved = reopened.load().await.map_err(|error| error.to_string())?;
                                Ok::<_, String>((reopened, list, saved))
                            };

                            match restored.await {
                                Ok((reopened, list, saved)) => {
                                    store.set(Some(reopened));
                                    leads.set(list);
                                    settings.set(saved);
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
                    today: today_in_peru(),
                    whatsapp_template: settings.read().whatsapp_template().to_owned(),
                    on_open_pipeline: move |_| section.set(Section::Pipeline),
                }
            } else if section() == Section::Pipeline {
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
                    on_advance: move |id: Uuid| {
                        if saving() {
                            return;
                        }
                        let Some(mut lead) = leads.read().iter().find(|lead| lead.id() == id).cloned() else {
                            return;
                        };
                        if lead.advance().is_ok() {
                            persist(lead, false);
                        }
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
        }
    }
}
