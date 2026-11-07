//! Presentational components. State and persistence live in `App`.

use cominorsa_domain::{Lead, Prospect, Stage};
use dioxus::prelude::*;
use time::Date;
use uuid::Uuid;

use crate::{
    AgendaGroups, FollowUp, FormValues, PipelineSummary, format_date, matches_query, owner_options,
    profile_label,
};

const SERVICES: [&str; 5] = ["REINFO", "IGAFOM", "DAC", "ESTAMIN", "Plan de minado"];

const BRAND_MARK: &str = include_str!("../assets/brand-mark.uri");
const ICON_PIPELINE: &str = "M4 4h4v16H4zM10 4h4v10h-4zM16 4h4v13h-4z";
const ICON_AGENDA: &str = "M4 6h16v14H4zM4 10h16M8 3v5M16 3v5";
const ICON_PROSPECTS: &str = "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-5-5";
const ICON_SETTINGS: &str = "M4 7h10M18 7h2M4 17h2M10 17h10M14 5h4v4h-4zM6 15h4v4H6z";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pipeline,
    Agenda,
    Prospects,
    Settings,
}

#[component]
pub fn Shell(section: Section, on_section: EventHandler<Section>, children: Element) -> Element {
    rsx! {
        div { class: "shell",
            aside { class: "sidebar",
                div { class: "brand",
                    img { class: "brand-mark", src: BRAND_MARK, alt: "COMINORSA" }
                    span { class: "brand-name", "COMINORSA" }
                }
                nav { class: "nav", aria_label: "Secciones",
                    NavItem { target: Section::Pipeline, current: section, label: "Embudo", icon: ICON_PIPELINE, on_section }
                    NavItem { target: Section::Agenda, current: section, label: "Agenda", icon: ICON_AGENDA, on_section }
                    NavItem { target: Section::Prospects, current: section, label: "Prospectos", icon: ICON_PROSPECTS, on_section }
                }
                nav { class: "nav nav-bottom", aria_label: "Preferencias",
                    NavItem { target: Section::Settings, current: section, label: "Configuración", icon: ICON_SETTINGS, on_section }
                }
            }
            main { class: "content", {children} }
        }
    }
}

#[component]
fn NavItem(
    target: Section,
    current: Section,
    label: &'static str,
    icon: &'static str,
    on_section: EventHandler<Section>,
) -> Element {
    let is_current = target == current;

    rsx! {
        button {
            class: if is_current { "nav-item active" } else { "nav-item" },
            title: label,
            aria_current: if is_current { "page" },
            onclick: move |_| on_section.call(target),
            svg { class: "icon", view_box: "0 0 24 24", "aria-hidden": "true", path { d: icon } }
            span { class: "nav-label", "{label}" }
        }
    }
}

#[component]
pub fn Agenda(
    leads: Vec<Lead>,
    today: Date,
    #[props(default)] whatsapp_template: String,
    on_open_pipeline: EventHandler<()>,
) -> Element {
    let groups = AgendaGroups::of(&leads, today);
    let is_empty = groups.is_empty();

    rsx! {
        header { class: "topbar",
            div {
                h1 { "Agenda" }
                p { class: "subtitle", "Hoy es {format_date(today)}" }
            }
        }
        if is_empty {
            section { class: "empty",
                h2 { "Nada pendiente" }
                p { "No hay seguimientos para los próximos 7 días." }
                button { class: "primary", onclick: move |_| on_open_pipeline.call(()), "Ver embudo" }
            }
        } else {
            AgendaGroup { title: "Vencidos", leads: groups.overdue, today, whatsapp_template: whatsapp_template.clone() }
            AgendaGroup { title: "Hoy", leads: groups.today, today, whatsapp_template: whatsapp_template.clone() }
            AgendaGroup { title: "Próximos 7 días", leads: groups.upcoming, today, whatsapp_template: whatsapp_template.clone() }
        }
    }
}

#[component]
fn AgendaGroup(
    title: &'static str,
    leads: Vec<Lead>,
    today: Date,
    whatsapp_template: String,
) -> Element {
    if leads.is_empty() {
        return rsx! {};
    }

    rsx! {
        section { class: "agenda-group",
            h2 { "{title} · {leads.len()}" }
            div { class: "agenda-list",
                for lead in leads {
                    {
                        let status = FollowUp::of(&lead, today);
                        rsx! {
                            div { class: "agenda-row", key: "{lead.id()}",
                                div {
                                    strong { "{lead.name()}" }
                                    span { class: "detail", "{lead.service()} · {lead.phone()}" }
                                }
                                div { class: "card-actions",
                                    span { class: "status {status.class()}", "{status.label()}" }
                                    if let Some(url) = lead.whatsapp_url(&whatsapp_template) {
                                        a {
                                            class: "secondary",
                                            href: "{url}",
                                            aria_label: "Preparar mensaje de WhatsApp para {lead.name()}",
                                            "WhatsApp"
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
                            th { class: "numeric hide-tablet", "Hectáreas" }
                            th { class: "hide-phone", "Departamentos" }
                            th { class: "hide-tablet", "Servicio potencial" }
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
                                td { class: "numeric hide-tablet", "{prospect.hectares:.0}" }
                                td { class: "hide-phone", "{prospect.departments}" }
                                td { class: "hide-tablet", "{prospect.potential_service}" }
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
    #[props(default)] whatsapp_template: String,
    #[props(default)] team: Vec<String>,
    on_query: EventHandler<String>,
    on_new: EventHandler<()>,
    on_advance: EventHandler<Uuid>,
    on_assign: EventHandler<(Uuid, String)>,
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
                    Board { leads: visible, today, whatsapp_template, team, on_advance, on_assign }
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
fn Board(
    leads: Vec<Lead>,
    today: Date,
    whatsapp_template: String,
    team: Vec<String>,
    on_advance: EventHandler<Uuid>,
    on_assign: EventHandler<(Uuid, String)>,
) -> Element {
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
                                        whatsapp: lead.whatsapp_url(&whatsapp_template),
                                        owners: owner_options(&team, lead.owner()),
                                        lead,
                                        on_advance,
                                        on_assign,
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
fn LeadCard(
    lead: Lead,
    status: FollowUp,
    whatsapp: Option<String>,
    owners: Vec<String>,
    on_advance: EventHandler<Uuid>,
    on_assign: EventHandler<(Uuid, String)>,
) -> Element {
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
            if !owners.is_empty() {
                select {
                    class: "owner-select",
                    aria_label: "Responsable de {lead.name()}",
                    onchange: move |event| on_assign.call((id, event.value())),
                    option { value: "", selected: lead.owner().is_none(), "Sin responsable" }
                    for owner in owners {
                        option {
                            key: "{owner}",
                            value: "{owner}",
                            selected: lead.owner() == Some(owner.as_str()),
                            "{owner}"
                        }
                    }
                }
            }
            div { class: "card-bottom",
                span { class: "status {status.class()}", "{status.label()}" }
                div { class: "card-actions",
                    if let Some(url) = whatsapp {
                        a {
                            class: "secondary",
                            href: "{url}",
                            aria_label: "Preparar mensaje de WhatsApp para {lead.name()}",
                            "WhatsApp"
                        }
                    }
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
}

#[component]
pub fn LeadForm(
    values: FormValues,
    saving: bool,
    error: String,
    #[props(default)] team: Vec<String>,
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
                    if !team.is_empty() {
                        label { "Responsable"
                            select {
                                onchange: edit(|v, text| v.owner = text),
                                option { value: "", selected: values.owner.is_empty(), "Sin responsable" }
                                for member in team.iter() {
                                    option {
                                        key: "{member}",
                                        value: "{member}",
                                        selected: values.owner == *member,
                                        "{member}"
                                    }
                                }
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
