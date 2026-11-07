//! Settings section. Rules live in the domain; this view only calls them.
//! Valid changes save automatically and every section reports the same state.

use cominorsa_domain::{Settings, ThemePreference, render_message};
use dioxus::prelude::*;

use crate::format_timestamp;

const PREVIEW_NAME: &str = "Minera Ejemplo";
const PREVIEW_SERVICE: &str = "REINFO";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SaveState {
    #[default]
    Idle,
    Saving,
    Saved,
    Failed(String),
}

#[component]
pub fn SettingsView(
    settings: Settings,
    database: String,
    save_state: SaveState,
    backup_notice: String,
    pending_restore: Option<String>,
    busy: bool,
    on_change: EventHandler<Settings>,
    on_pick_folder: EventHandler<()>,
    on_backup: EventHandler<()>,
    on_pick_restore: EventHandler<()>,
    on_confirm_restore: EventHandler<()>,
    on_cancel_restore: EventHandler<()>,
) -> Element {
    let mut member = use_signal(String::new);
    let mut member_error = use_signal(String::new);
    let mut days_text = use_signal({
        let days = settings.follow_up_days();
        move || days.to_string()
    });
    let mut days_error = use_signal(String::new);
    let mut template_text = use_signal({
        let template = settings.whatsapp_template().to_owned();
        move || template
    });
    let mut template_error = use_signal(String::new);

    let add_member = {
        let settings = settings.clone();
        move |event: FormEvent| {
            event.prevent_default();
            let mut next = settings.clone();
            match next.add_member(&member()) {
                Ok(()) => {
                    member.set(String::new());
                    member_error.set(String::new());
                    on_change.call(next);
                }
                Err(failure) => member_error.set(failure.to_string()),
            }
        }
    };
    let days_settings = settings.clone();
    let template_settings = settings.clone();
    let preview = render_message(&template_text(), PREVIEW_NAME, PREVIEW_SERVICE);
    let backup_status = settings.last_backup_at().map_or_else(
        || "Aún no hay copias".to_owned(),
        |at| format!("Última copia: {}", format_timestamp(at)),
    );
    let has_folder = settings.backup_directory().is_some();

    rsx! {
        header { class: "topbar",
            div {
                h1 { "Configuración" }
                p { class: "subtitle", "Los cambios se guardan solos" }
            }
            match &save_state {
                SaveState::Idle => rsx! {},
                SaveState::Saving => rsx! { p { class: "save-state", role: "status", "Guardando…" } },
                SaveState::Saved => rsx! { p { class: "save-state saved", role: "status", "Guardado" } },
                SaveState::Failed(reason) => rsx! {
                    p { class: "form-error", role: "alert", "No se pudo guardar: {reason}" }
                },
            }
        }
        div { class: "settings",
            SettingsGroup { title: "Apariencia",
                div { class: "setting-row",
                    div { class: "setting-info",
                        h3 { "Tema" }
                        p { "Sistema usa el modo claro u oscuro del equipo." }
                    }
                    fieldset { class: "segmented",
                        legend { class: "visually-hidden", "Tema" }
                        for theme in ThemePreference::ALL {
                            {
                                let settings = settings.clone();
                                rsx! {
                                    label { key: "{theme.code()}",
                                        input {
                                            r#type: "radio",
                                            name: "theme",
                                            checked: settings.theme == theme,
                                            onchange: move |_| {
                                                let mut next = settings.clone();
                                                next.theme = theme;
                                                on_change.call(next);
                                            },
                                        }
                                        span { "{theme.label()}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            SettingsGroup { title: "Equipo",
                div { class: "setting-row stacked",
                    div { class: "setting-info",
                        h3 { "Personas" }
                        p { "Aparecen como responsables de cada lead." }
                    }
                    if settings.team().is_empty() {
                        p { class: "muted-note", "Aún no hay nadie: todavía no agregaste personas." }
                    }
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
                    if !member_error().is_empty() {
                        p { class: "form-error", role: "alert", "{member_error}" }
                    }
                }
            }
            SettingsGroup { title: "Seguimiento",
                div { class: "setting-row",
                    div { class: "setting-info",
                        h3 { "Plazo sugerido" }
                        p { "Fecha que se propone al registrar un lead." }
                        if !days_error().is_empty() {
                            p { class: "form-error", role: "alert", "{days_error}" }
                        }
                    }
                    label { class: "days-field",
                        input {
                            class: "days",
                            r#type: "number",
                            min: 1,
                            max: 90,
                            aria_label: "Días para el próximo seguimiento",
                            value: "{days_text}",
                            oninput: move |event| {
                                days_text.set(event.value());
                                let mut next = days_settings.clone();
                                let result = event
                                    .value()
                                    .parse()
                                    .map_err(|_| "Escribe un número de 1 a 90".to_owned())
                                    .and_then(|days| {
                                        next.set_follow_up_days(days)
                                            .map_err(|failure| failure.to_string())
                                    });
                                match result {
                                    Ok(()) => {
                                        days_error.set(String::new());
                                        on_change.call(next);
                                    }
                                    Err(message) => days_error.set(message),
                                }
                            },
                        }
                        span { class: "days-unit", "días" }
                    }
                }
            }
            SettingsGroup { title: "Mensaje de WhatsApp",
                div { class: "setting-row stacked",
                    div { class: "setting-info",
                        h3 { "Plantilla" }
                        p { "Se abre lista para revisar; una persona la envía. Usa {{nombre}} y {{servicio}}." }
                    }
                    textarea {
                        aria_label: "Plantilla del mensaje de WhatsApp",
                        rows: 3,
                        value: "{template_text}",
                        oninput: move |event| {
                            template_text.set(event.value());
                            let mut next = template_settings.clone();
                            match next.set_whatsapp_template(&event.value()) {
                                Ok(()) => {
                                    template_error.set(String::new());
                                    on_change.call(next);
                                }
                                Err(failure) => template_error.set(failure.to_string()),
                            }
                        },
                    }
                    if template_error().is_empty() {
                        div { class: "preview",
                            span { "Vista previa" }
                            p { "{preview}" }
                        }
                    } else {
                        p { class: "form-error", role: "alert", "{template_error}" }
                    }
                }
            }
            SettingsGroup { title: "Datos",
                div { class: "setting-row stacked",
                    div { class: "setting-info",
                        h3 { "Copia de seguridad" }
                        p { "{backup_status}" }
                        p { "Usa un USB o disco externo: una copia en la misma laptop no sirve si esta se daña." }
                    }
                    if let Some(folder) = settings.backup_directory() {
                        p { class: "path", "{folder}" }
                    }
                    div { class: "button-row",
                        button {
                            class: "secondary",
                            disabled: busy,
                            onclick: move |_| on_pick_folder.call(()),
                            if has_folder { "Cambiar carpeta" } else { "Elegir carpeta" }
                        }
                        button {
                            class: "primary",
                            disabled: busy || !has_folder,
                            onclick: move |_| on_backup.call(()),
                            "Crear copia ahora"
                        }
                    }
                    if !backup_notice.is_empty() {
                        p { class: "notice", role: "status", "{backup_notice}" }
                    }
                }
                div { class: "setting-row stacked",
                    div { class: "setting-info",
                        h3 { "Restaurar" }
                        p { "Reemplaza los datos actuales. Antes se guarda una copia de seguridad." }
                    }
                    if let Some(path) = &pending_restore {
                        div { class: "confirm", role: "alertdialog", aria_label: "Confirmar restauración",
                            p { "Se reemplazarán todos los datos actuales por esta copia:" }
                            p { class: "path", "{path}" }
                            div { class: "button-row",
                                button {
                                    class: "danger",
                                    disabled: busy,
                                    onclick: move |_| on_confirm_restore.call(()),
                                    "Reemplazar datos actuales"
                                }
                                button {
                                    class: "secondary",
                                    disabled: busy,
                                    onclick: move |_| on_cancel_restore.call(()),
                                    "Cancelar"
                                }
                            }
                        }
                    } else {
                        button {
                            class: "secondary start",
                            disabled: busy,
                            onclick: move |_| on_pick_restore.call(()),
                            "Restaurar una copia"
                        }
                    }
                }
                details { class: "setting-row",
                    summary { "Ubicación de la base de datos" }
                    p { class: "path", "{database}" }
                }
            }
            SettingsGroup { title: "Acerca de",
                div { class: "setting-row",
                    div { class: "setting-info",
                        h3 { "COMINORSA CRM" }
                        p { "Versión {env!(\"CARGO_PKG_VERSION\")}" }
                    }
                }
            }
        }
    }
}

#[component]
fn SettingsGroup(title: &'static str, children: Element) -> Element {
    rsx! {
        section { class: "settings-group",
            h2 { "{title}" }
            div { class: "settings-list", {children} }
        }
    }
}
