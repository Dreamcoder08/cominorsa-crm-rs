//! Settings section. Rules live in the domain; this view only calls them.

use cominorsa_domain::{Settings, ThemePreference};
use dioxus::prelude::*;

#[component]
pub fn SettingsView(
    settings: Settings,
    database: String,
    notice: String,
    on_change: EventHandler<Settings>,
    on_backup: EventHandler<()>,
) -> Element {
    let mut member = use_signal(String::new);
    let mut error = use_signal(String::new);

    let add_member = {
        let settings = settings.clone();
        move |event: FormEvent| {
            event.prevent_default();
            let mut next = settings.clone();
            match next.add_member(&member()) {
                Ok(()) => {
                    member.set(String::new());
                    error.set(String::new());
                    on_change.call(next);
                }
                Err(failure) => error.set(failure.to_string()),
            }
        }
    };
    let days = settings.follow_up_days();
    let days_settings = settings.clone();
    let template_settings = settings.clone();

    rsx! {
        header { class: "topbar",
            div {
                h1 { "Configuración" }
                p { class: "subtitle", "Preferencias guardadas en esta laptop" }
            }
        }
        div { class: "settings",
            section { class: "setting-group",
                h2 { "Apariencia" }
                p { "Sistema sigue el tema de Windows." }
                div { class: "segmented", role: "group", aria_label: "Tema",
                    for theme in ThemePreference::ALL {
                        {
                            let settings = settings.clone();
                            rsx! {
                                button {
                                    key: "{theme.code()}",
                                    aria_pressed: if settings.theme == theme { "true" } else { "false" },
                                    onclick: move |_| {
                                        let mut next = settings.clone();
                                        next.theme = theme;
                                        on_change.call(next);
                                    },
                                    "{theme.label()}"
                                }
                            }
                        }
                    }
                }
            }
            section { class: "setting-group",
                h2 { "Equipo" }
                p { "Personas que trabajan con el CRM." }
                ul { class: "team",
                    for name in settings.team().iter().cloned() {
                        {
                            let settings = settings.clone();
                            let target = name.clone();
                            rsx! {
                                li { key: "{name}",
                                    span { "{name}" }
                                    button {
                                        class: "secondary",
                                        onclick: move |_| {
                                            let mut next = settings.clone();
                                            next.remove_member(&target);
                                            on_change.call(next);
                                        },
                                        "Quitar"
                                    }
                                }
                            }
                        }
                    }
                }
                form { class: "inline-form", onsubmit: add_member,
                    input {
                        aria_label: "Nombre de la persona",
                        placeholder: "Nombre",
                        value: "{member}",
                        oninput: move |event| member.set(event.value()),
                    }
                    button { class: "secondary", r#type: "submit", "Agregar" }
                }
                if !error().is_empty() {
                    p { class: "form-error", role: "alert", "{error}" }
                }
            }
            section { class: "setting-group",
                h2 { "Seguimiento" }
                p { "Días que se proponen para el próximo seguimiento de un lead nuevo." }
                input {
                    class: "days",
                    r#type: "number",
                    min: 1,
                    max: 90,
                    aria_label: "Días de seguimiento",
                    value: "{days}",
                    oninput: move |event| {
                        let mut next = days_settings.clone();
                        let applied = event
                            .value()
                            .parse()
                            .ok()
                            .and_then(|value| next.set_follow_up_days(value).ok());
                        if applied.is_some() {
                            on_change.call(next);
                        }
                    },
                }
            }
            section { class: "setting-group",
                h2 { "Mensaje de WhatsApp" }
                p { "Se abre listo para revisar; una persona lo envía. Usa {{nombre}} y {{servicio}}." }
                textarea {
                    aria_label: "Mensaje de WhatsApp",
                    rows: 3,
                    value: "{settings.whatsapp_template()}",
                    onchange: move |event| {
                        let mut next = template_settings.clone();
                        if next.set_whatsapp_template(&event.value()).is_ok() {
                            on_change.call(next);
                        }
                    },
                }
            }
            section { class: "setting-group",
                h2 { "Datos" }
                p { "La información vive solo en esta laptop." }
                p { class: "path", "{database}" }
                button {
                    class: "secondary start",
                    onclick: move |_| on_backup.call(()),
                    "Crear copia de seguridad"
                }
                if !notice.is_empty() {
                    p { class: "notice", role: "status", "{notice}" }
                }
            }
            section { class: "setting-group",
                h2 { "Acerca de" }
                p { "COMINORSA CRM {env!(\"CARGO_PKG_VERSION\")}" }
            }
        }
    }
}
