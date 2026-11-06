//! Desktop adapter: Dioxus views over the domain and storage crates.

use std::{env, path::PathBuf};

mod presentation;
mod views;

use cominorsa_domain::{Lead, LeadRepository, NewLead, Prospect, ProspectRepository};
use cominorsa_storage::SqliteLeads;
use dioxus::prelude::*;
use directories::ProjectDirs;
use time::{Date, OffsetDateTime, format_description::well_known::Iso8601, macros::offset};
use uuid::Uuid;

pub use presentation::{FollowUp, PipelineSummary, format_date, matches_query, profile_label};
pub use views::{LeadForm, Prospects, Section, Shell, Workspace};

const STYLE: &str = include_str!("style.css");
const DATABASE_PATH_VARIABLE: &str = "COMINORSA_DB_PATH";
const PROSPECT_PAGE_SIZE: u32 = 100;
const MANUAL_REVIEW_SERVICE: &str = "REVISAR_MANUAL";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormValues {
    pub name: String,
    pub phone: String,
    pub ruc: String,
    pub service: String,
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
        follow_up_on,
    })
    .map_err(|error| error.to_string())
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
    let mut form = use_signal(|| FormValues::starting_on(today_in_peru()));
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
            Ok::<_, String>((opened, list))
        };

        match loaded.await {
            Ok((opened, list)) => {
                store.set(Some(opened));
                leads.set(list);
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
                        form.set(FormValues::starting_on(today_in_peru()));
                        panel_open.set(false);
                    }
                }
                Err(failure) => form_error.set(failure.to_string()),
            }
            saving.set(false);
        });
    };

    rsx! {
        style { {STYLE} }
        if let Some(message) = startup_error() {
            p { class: "banner", role: "alert", "{message}" }
        }
        Shell {
            section: section(),
            today: today_in_peru(),
            on_section: move |next| section.set(next),
            if section() == Section::Pipeline {
                Workspace {
                    leads: leads(),
                    today: today_in_peru(),
                    query: query(),
                    on_query: move |value| query.set(value),
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
