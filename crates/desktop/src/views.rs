//! Presentational components. State and persistence live in `App`.

use cominorsa_domain::{Lead, Prospect, Stage};
use dioxus::prelude::*;
use time::Date;
use uuid::Uuid;

use crate::{FollowUp, FormValues, PipelineSummary, format_date, matches_query, profile_label};

const SERVICES: [&str; 5] = ["REINFO", "IGAFOM", "DAC", "ESTAMIN", "Plan de minado"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pipeline,
    Prospects,
}

#[component]
pub fn Shell(
    section: Section,
    today: Date,
    on_section: EventHandler<Section>,
    children: Element,
) -> Element {
    let nav = |target: Section| {
        if section == target {
            "nav-item active"
        } else {
            "nav-item"
        }
    };

    rsx! {
        div { class: "shell",
            aside { class: "sidebar",
                div { class: "brand",
                    span { class: "brand-mark", "C" }
                    span { "COMINORSA" }
                }
                nav { aria_label: "Secciones",
                    button {
                        class: nav(Section::Pipeline),
                        onclick: move |_| on_section.call(Section::Pipeline),
                        "Embudo"
                    }
                    button {
                        class: nav(Section::Prospects),
                        onclick: move |_| on_section.call(Section::Prospects),
                        "Prospectos"
                    }
                }
                p { class: "sidebar-date", "{format_date(today)}" }
            }
            main { class: "content", {children} }
        }
    }
}

#[component]
pub fn Prospects(
    prospects: Vec<Prospect>,
    total: usize,
    query: String,
    on_query: EventHandler<String>,
    on_convert: EventHandler<Prospect>,
) -> Element {
    let shown = prospects.len();

    rsx! {
        header { class: "topbar",
            div {
                h1 { "Prospectos" }
                p { class: "subtitle", "Titulares de concesiones, sin datos de contacto" }
            }
            div { class: "actions",
                input {
                    class: "search",
                    r#type: "search",
                    aria_label: "Buscar prospectos",
                    placeholder: "Buscar por titular, zona o servicio",
                    value: "{query}",
                    oninput: move |event| on_query.call(event.value()),
                }
            }
        }
        p { class: "result-count", "Mostrando {shown} de {total}" }
        if prospects.is_empty() {
            section { class: "empty",
                h2 { "Sin prospectos" }
                p { "No hay titulares que coincidan con la búsqueda." }
            }
        } else {
            div { class: "table-frame",
                table {
                    thead {
                        tr {
                            th { "Titular" }
                            th { "Perfil" }
                            th { class: "numeric", "Concesiones" }
                            th { class: "numeric", "Hectáreas" }
                            th { "Departamentos" }
                            th { "Servicio potencial" }
                            th { span { class: "visually-hidden", "Acción" } }
                        }
                    }
                    tbody {
                        for prospect in prospects {
                            tr { key: "{prospect.kind.code()}-{prospect.name}",
                                td {
                                    strong { "{prospect.name}" }
                                    span { class: "kind", "{prospect.kind.label()}" }
                                }
                                td { "{profile_label(&prospect.profile)}" }
                                td { class: "numeric", "{prospect.concessions}" }
                                td { class: "numeric", "{prospect.hectares:.0}" }
                                td { "{prospect.departments}" }
                                td { "{prospect.potential_service}" }
                                td {
                                    button {
                                        class: "secondary",
                                        onclick: move |_| on_convert.call(prospect.clone()),
                                        "Convertir en lead"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Workspace(
    leads: Vec<Lead>,
    today: Date,
    query: String,
    on_query: EventHandler<String>,
    on_new: EventHandler<()>,
    on_advance: EventHandler<Uuid>,
) -> Element {
    let summary = PipelineSummary::of(&leads, today);
    let is_empty = leads.is_empty();
    let visible: Vec<Lead> = leads
        .into_iter()
        .filter(|lead| matches_query(lead, &query))
        .collect();
    let has_no_results = !is_empty && visible.is_empty();

    rsx! {
                header { class: "topbar",
                    div {
                        h1 { "Embudo comercial" }
                        p { class: "subtitle", "Seguimiento de leads y trámites" }
                    }
                    div { class: "actions",
                        input {
                            class: "search",
                            r#type: "search",
                            aria_label: "Buscar leads",
                            placeholder: "Buscar por nombre, RUC, teléfono o servicio",
                            value: "{query}",
                            oninput: move |event| on_query.call(event.value()),
                        }
                        button {
                            class: "primary",
                            onclick: move |_| on_new.call(()),
                            "Nuevo lead"
                        }
                    }
                }
                section { class: "metrics", aria_label: "Resumen",
                    Metric { label: "Leads activos", value: summary.active, tone: "" }
                    Metric { label: "Vencidos", value: summary.overdue, tone: "overdue" }
                    Metric { label: "Vencen hoy", value: summary.due_today, tone: "today" }
                    Metric { label: "Contratos firmados", value: summary.signed, tone: "closed" }
                }
                if is_empty {
                    section { class: "empty",
                        h2 { "Registra tu primer lead" }
                        p { "Cada consulta que llegue por WhatsApp o por la web empieza aquí." }
                        button { class: "primary", onclick: move |_| on_new.call(()), "Nuevo lead" }
                    }
                } else if has_no_results {
                    section { class: "empty",
                        h2 { "Sin resultados" }
                        p { "Prueba con otro nombre, RUC, teléfono o servicio." }
                    }
                } else {
                    Board { leads: visible, today, on_advance }
                }
    }
}

#[component]
fn Metric(label: &'static str, value: usize, tone: &'static str) -> Element {
    rsx! {
        div { class: "metric {tone}",
            span { class: "metric-value", "{value}" }
            span { class: "metric-label", "{label}" }
        }
    }
}

#[component]
fn Board(leads: Vec<Lead>, today: Date, on_advance: EventHandler<Uuid>) -> Element {
    rsx! {
        section { class: "board", aria_label: "Etapas del embudo",
            for stage in Stage::ALL {
                {
                    let in_stage: Vec<Lead> = leads
                        .iter()
                        .filter(|lead| lead.stage() == stage)
                        .cloned()
                        .collect();
                    let count = in_stage.len();

                    rsx! {
                        div { class: "column", key: "{stage.code()}",
                            header { class: "column-header",
                                h2 { "{stage.label()}" }
                                span { class: "count", "{count}" }
                            }
                            div { class: "cards",
                                for lead in in_stage {
                                    LeadCard {
                                        key: "{lead.id()}",
                                        status: FollowUp::of(&lead, today),
                                        lead,
                                        on_advance,
                                    }
                                }
                                if count == 0 {
                                    p { class: "column-empty", "Sin leads" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LeadCard(lead: Lead, status: FollowUp, on_advance: EventHandler<Uuid>) -> Element {
    let id = lead.id();

    rsx! {
        article { class: "card {status.class()}",
            div { class: "card-top",
                h3 { "{lead.name()}" }
                span { class: "service", "{lead.service()}" }
            }
            p { class: "detail", "{lead.phone()}" }
            if let Some(ruc) = lead.ruc() {
                p { class: "detail", "RUC {ruc.as_str()}" }
            }
            div { class: "card-bottom",
                span { class: "status {status.class()}", "{status.label()}" }
                if lead.stage() != Stage::ContractSigned {
                    button {
                        class: "secondary",
                        aria_label: "Mover {lead.name()} a la siguiente etapa",
                        onclick: move |_| on_advance.call(id),
                        "Avanzar"
                    }
                }
            }
        }
    }
}

#[component]
pub fn LeadForm(
    values: FormValues,
    saving: bool,
    error: String,
    on_change: EventHandler<FormValues>,
    on_submit: EventHandler<()>,
    on_close: EventHandler<()>,
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
        div {
            class: "overlay",
            onclick: move |_| on_close.call(()),
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            aside {
                class: "panel",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Nuevo lead",
                onclick: move |event| event.stop_propagation(),
                header { class: "panel-header",
                    h2 { "Nuevo lead" }
                    button {
                        class: "ghost",
                        r#type: "button",
                        aria_label: "Cerrar",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                form {
                    onsubmit: move |event| {
                        event.prevent_default();
                        on_submit.call(());
                    },
                    label { "Nombre o razón social"
                        input {
                            autofocus: true,
                            value: "{values.name}",
                            oninput: edit(|v, text| v.name = text),
                        }
                    }
                    label { "Teléfono o WhatsApp"
                        input {
                            r#type: "tel",
                            value: "{values.phone}",
                            oninput: edit(|v, text| v.phone = text),
                        }
                    }
                    label { "RUC"
                        input {
                            inputmode: "numeric",
                            maxlength: 11,
                            placeholder: "Opcional, 11 dígitos",
                            value: "{values.ruc}",
                            oninput: edit(|v, text| v.ruc = text),
                        }
                    }
                    label { "Servicio"
                        input {
                            list: "services",
                            value: "{values.service}",
                            oninput: edit(|v, text| v.service = text),
                        }
                        datalist { id: "services",
                            for service in SERVICES {
                                option { value: service }
                            }
                        }
                    }
                    label { "Próximo seguimiento"
                        input {
                            r#type: "date",
                            value: "{values.follow_up_on}",
                            oninput: edit(|v, text| v.follow_up_on = text),
                        }
                    }
                    if !error.is_empty() {
                        p { class: "form-error", role: "alert", "{error}" }
                    }
                    button {
                        class: "primary",
                        r#type: "submit",
                        disabled: saving,
                        if saving { "Guardando…" } else { "Registrar lead" }
                    }
                }
            }
        }
    }
}
