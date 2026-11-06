//! Desktop adapter: Dioxus views over the domain and storage crates.

use std::{env, path::PathBuf};

use cominorsa_domain::{Lead, LeadRepository, NewLead, Stage};
use cominorsa_storage::SqliteLeads;
use dioxus::prelude::*;
use directories::ProjectDirs;
use time::{Date, OffsetDateTime, format_description::well_known::Iso8601, macros::offset};
use uuid::Uuid;

const STYLE: &str = include_str!("style.css");
const DATABASE_PATH_VARIABLE: &str = "COMINORSA_DB_PATH";

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
    let mut error = use_signal(|| None::<String>);
    let mut form = use_signal(|| FormValues::starting_on(today_in_peru()));

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
            Err(message) => error.set(Some(message)),
        }
    });

    let mut persist = move |lead: Lead, clear_form: bool| {
        let Some(repository) = store.read().clone() else {
            error.set(Some("La base de datos todavía no está lista".to_owned()));
            return;
        };

        spawn(async move {
            let saved = async {
                repository.save(&lead).await?;
                repository.list().await
            };

            match saved.await {
                Ok(list) => {
                    leads.set(list);
                    error.set(None);
                    if clear_form {
                        form.set(FormValues::starting_on(today_in_peru()));
                    }
                }
                Err(failure) => error.set(Some(failure.to_string())),
            }
        });
    };

    rsx! {
        style { {STYLE} }
        main {
            h1 { "COMINORSA CRM" }
            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }
            LeadForm {
                values: form(),
                on_change: move |values| form.set(values),
                on_submit: move |_| match parse_form(form()) {
                    Ok(lead) => persist(lead, true),
                    Err(message) => error.set(Some(message)),
                },
            }
            Board {
                leads: leads(),
                today: today_in_peru(),
                on_advance: move |id: Uuid| {
                    let Some(mut lead) = leads.read().iter().find(|lead| lead.id() == id).cloned() else {
                        return;
                    };
                    match lead.advance() {
                        Ok(()) => persist(lead, false),
                        Err(failure) => error.set(Some(failure.to_string())),
                    }
                },
            }
        }
    }
}

#[component]
fn LeadForm(
    values: FormValues,
    on_change: EventHandler<FormValues>,
    on_submit: EventHandler<()>,
) -> Element {
    let edit = |update: fn(&mut FormValues, String)| {
        let values = values.clone();
        move |event: FormEvent| {
            let mut next = values.clone();
            update(&mut next, event.value());
            on_change.call(next);
        }
    };

    rsx! {
        form {
            onsubmit: move |event| {
                event.prevent_default();
                on_submit.call(());
            },
            label { "Nombre o razón social"
                input { value: "{values.name}", oninput: edit(|v, text| v.name = text) }
            }
            label { "Teléfono"
                input { value: "{values.phone}", oninput: edit(|v, text| v.phone = text) }
            }
            label { "RUC (opcional)"
                input { value: "{values.ruc}", oninput: edit(|v, text| v.ruc = text) }
            }
            label { "Servicio"
                input { value: "{values.service}", oninput: edit(|v, text| v.service = text) }
            }
            label { "Próximo seguimiento"
                input {
                    r#type: "date",
                    value: "{values.follow_up_on}",
                    oninput: edit(|v, text| v.follow_up_on = text),
                }
            }
            button { r#type: "submit", "Registrar lead" }
        }
    }
}

#[component]
pub fn Board(leads: Vec<Lead>, today: Date, on_advance: EventHandler<Uuid>) -> Element {
    let due = leads
        .iter()
        .filter(|lead| lead.is_follow_up_due(today))
        .count();
    let summary = match due {
        0 => "Sin seguimientos pendientes".to_owned(),
        1 => "1 seguimiento pendiente".to_owned(),
        count => format!("{count} seguimientos pendientes"),
    };

    rsx! {
        p { class: "summary", "{summary}" }
        section { class: "board",
            for stage in Stage::ALL {
                div { class: "column", key: "{stage.code()}",
                    h2 { "{stage.label()}" }
                    for lead in leads.iter().filter(|lead| lead.stage() == stage) {
                        LeadCard {
                            key: "{lead.id()}",
                            lead: lead.clone(),
                            is_due: lead.is_follow_up_due(today),
                            on_advance,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LeadCard(lead: Lead, is_due: bool, on_advance: EventHandler<Uuid>) -> Element {
    let id = lead.id();
    let date_label = if is_due { "Vence" } else { "Seguimiento" };

    rsx! {
        article { class: if is_due { "card due" } else { "card" },
            h3 { "{lead.name()}" }
            p { "{lead.service()} · {lead.phone()}" }
            if let Some(ruc) = lead.ruc() {
                p { "RUC {ruc.as_str()}" }
            }
            p { "{date_label}: {lead.follow_up_on()}" }
            if lead.stage() != Stage::ContractSigned {
                button { onclick: move |_| on_advance.call(id), "Avanzar" }
            }
        }
    }
}
