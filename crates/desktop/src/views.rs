//! Presentational components. State and persistence live in `App`.

use cominorsa_application::AppError;
use cominorsa_domain::{CaseFile, Lead, LeadOrigin, Prospect, Stage};
use dioxus::prelude::*;
use time::Date;
use uuid::Uuid;

use crate::{
    AgendaGroups, AgendaItem, FollowUp, FormValues, PipelineSummary, format_date, format_soles,
    matches_query, milestone_status_label, owner_options, presentation::consent_label,
    profile_label, step_status_label, tax_status_class,
};

/// A consent decision a person records on a lead card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentEdit {
    pub id: Uuid,
    pub revoke: bool,
    pub source: String,
}

/// Every date on a case step is a suggestion, never a legal deadline.
pub(crate) const STEP_DATE_NOTE: &str = "Fecha sugerida, ajústala";

const SERVICES: [&str; 5] = ["REINFO", "IGAFOM", "DAC", "ESTAMIN", "Plan de minado"];

const BRAND_MARK: &str = include_str!("../assets/brand-mark.uri");
const ICON_PIPELINE: &str = "M4 4h4v16H4zM10 4h4v10h-4zM16 4h4v13h-4z";
const ICON_AGENDA: &str = "M4 6h16v14H4zM4 10h16M8 3v5M16 3v5";
const ICON_CASES: &str = "M3 6h6l2 3h10v11H3z";
const ICON_PROSPECTS: &str = "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-5-5";
const ICON_SETTINGS: &str = "M4 7h10M18 7h2M4 17h2M10 17h10M14 5h4v4h-4zM6 15h4v4H6z";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pipeline,
    Agenda,
    Cases,
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
                    NavItem { target: Section::Cases, current: section, label: "Expedientes", icon: ICON_CASES, on_section }
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
    #[props(default)] cases: Vec<CaseFile>,
    today: Date,
    #[props(default)] whatsapp_template: String,
    on_open_pipeline: EventHandler<()>,
    #[props(default)] on_open_case: EventHandler<Uuid>,
) -> Element {
    let groups = AgendaGroups::of(&leads, &cases, today);
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
                p { "No hay seguimientos, trámites ni cobros para los próximos 7 días." }
                button { class: "primary", onclick: move |_| on_open_pipeline.call(()), "Ver embudo" }
            }
        } else {
            AgendaGroup { title: "Vencidos", items: groups.overdue, today, whatsapp_template: whatsapp_template.clone(), on_open_case }
            AgendaGroup { title: "Hoy", items: groups.today, today, whatsapp_template: whatsapp_template.clone(), on_open_case }
            AgendaGroup { title: "Próximos 7 días", items: groups.upcoming, today, whatsapp_template: whatsapp_template.clone(), on_open_case }
        }
    }
}

#[component]
fn AgendaGroup(
    title: &'static str,
    items: Vec<AgendaItem>,
    today: Date,
    whatsapp_template: String,
    on_open_case: EventHandler<Uuid>,
) -> Element {
    if items.is_empty() {
        return rsx! {};
    }

    rsx! {
        section { class: "agenda-group",
            h2 { "{title} · {items.len()}" }
            div { class: "agenda-list",
                for item in items {
                    {
                        let kind = item.kind_label();
                        match item {
                            AgendaItem::FollowUp(lead) => {
                                let status = FollowUp::of(&lead, today);
                                rsx! {
                                    div { class: "agenda-row", key: "{lead.id()}",
                                        div {
                                            span { class: "agenda-kind", "{kind}" }
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
                            AgendaItem::Step { case, step } => {
                                let status = FollowUp::of_step(&step, today);
                                let id = case.id();
                                rsx! {
                                    div { class: "agenda-row", key: "{step.id()}",
                                        div {
                                            span { class: "agenda-kind step", "{kind}" }
                                            strong { "{case.client_name()}" }
                                            span { class: "detail", "{step.title()} · Fecha sugerida {format_date(step.due_on())}" }
                                        }
                                        div { class: "card-actions",
                                            span { class: "status {status.class()}", "{step_status_label(&step, today)}" }
                                            button {
                                                class: "secondary",
                                                aria_label: "Ver expediente de {case.client_name()}",
                                                onclick: move |_| on_open_case.call(id),
                                                "Ver expediente"
                                            }
                                        }
                                    }
                                }
                            }
                            AgendaItem::Milestone { case, milestone } => {
                                let status = FollowUp::of_milestone(&milestone, today);
                                let id = case.id();
                                rsx! {
                                    div { class: "agenda-row", key: "{milestone.id()}",
                                        div {
                                            span { class: "agenda-kind billing", "{kind}" }
                                            strong { "{case.client_name()}" }
                                            span { class: "detail",
                                                "{milestone.title()} · {format_soles(milestone.amount())} · Vence {format_date(milestone.due_on())}"
                                            }
                                        }
                                        div { class: "card-actions",
                                            span { class: "status {status.class()}", "{milestone_status_label(&milestone, today)}" }
                                            button {
                                                class: "secondary",
                                                aria_label: "Ver expediente de {case.client_name()}",
                                                onclick: move |_| on_open_case.call(id),
                                                "Ver expediente"
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
                            th { "SUNAT" }
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
                                    if let Some(tax) = &prospect.tax {
                                        span { class: "tax-line", "RUC {tax.ruc}" }
                                        span { class: "tax-line", "{tax.address}" }
                                    }
                                }
                                td { "{profile_label(&prospect.profile)}" }
                                td { class: "numeric", "{prospect.concessions}" }
                                td { class: "numeric hide-tablet", "{prospect.hectares:.0}" }
                                td {
                                    if let Some(tax) = &prospect.tax {
                                        span { class: "status {tax_status_class(&tax.status)}", "{tax.status}" }
                                        if tax.approximate {
                                            span { class: "tax-line", "Confirmar RUC" }
                                        }
                                    } else {
                                        span { class: "status", "Sin RUC" }
                                    }
                                }
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
    #[props(default)] on_quote: EventHandler<Uuid>,
    #[props(default)] on_consent: EventHandler<ConsentEdit>,
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
                    Board { leads: visible, today, whatsapp_template, team, on_advance, on_assign, on_quote, on_consent }
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
    on_quote: EventHandler<Uuid>,
    on_consent: EventHandler<ConsentEdit>,
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
                                        whatsapp: lead
                                            .whatsapp_link(&whatsapp_template)
                                            .map_err(|refusal| AppError::from(refusal).code().to_owned()),
                                        owners: owner_options(&team, lead.owner()),
                                        lead,
                                        on_advance,
                                        on_assign,
                                        on_quote,
                                        on_consent,
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
    /// The chat link, or the code of the rule that refuses it.
    whatsapp: Result<String, String>,
    owners: Vec<String>,
    on_advance: EventHandler<Uuid>,
    on_assign: EventHandler<(Uuid, String)>,
    on_quote: EventHandler<Uuid>,
    on_consent: EventHandler<ConsentEdit>,
) -> Element {
    let id = lead.id();
    let revoke = lead.has_consent();
    let mut editing_consent = use_signal(|| false);
    let mut consent_source = use_signal(String::new);
    let consent = consent_label(&lead);

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
            if lead.renewal_of().is_some() {
                p { class: "agenda-kind step", "Cliente existente" }
            } else {
                p { class: "detail", "Origen: {lead.origin().label()}" }
            }
            p { class: "detail consent", "{consent}" }
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
                    match &whatsapp {
                        Ok(url) => rsx! {
                            a {
                                class: "secondary",
                                href: "{url}",
                                aria_label: "Preparar mensaje de WhatsApp para {lead.name()}",
                                "WhatsApp"
                            }
                        },
                        Err(code) if code == "no_consent" => rsx! {
                            button {
                                class: "secondary",
                                disabled: true,
                                "data-code": "{code}",
                                title: "Registra el consentimiento del cliente para abrir el chat",
                                "WhatsApp"
                            }
                        },
                        Err(_) => rsx! {},
                    }
                    button {
                        class: "secondary",
                        aria_label: if revoke { "Retirar consentimiento de {lead.name()}" } else { "Registrar consentimiento de {lead.name()}" },
                        onclick: move |_| editing_consent.set(true),
                        if revoke { "Retirar consentimiento" } else { "Registrar consentimiento" }
                    }
                    button {
                        class: "secondary",
                        aria_label: "Cotizaciones de {lead.client_name()}",
                        onclick: move |_| on_quote.call(id),
                        "Cotizar"
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
            if editing_consent() {
                form {
                    class: "consent-form",
                    onsubmit: move |event| {
                        event.prevent_default();
                        on_consent.call(ConsentEdit { id, revoke, source: consent_source() });
                        editing_consent.set(false);
                        consent_source.set(String::new());
                    },
                    label {
                        if revoke { "¿Cómo retiró el consentimiento?" } else { "¿Cómo aceptó ser contactado?" }
                        input {
                            value: "{consent_source}",
                            oninput: move |event| consent_source.set(event.value()),
                        }
                    }
                    div { class: "button-row",
                        button { class: "primary", r#type: "submit", "Guardar" }
                        button {
                            class: "secondary",
                            r#type: "button",
                            onclick: move |_| editing_consent.set(false),
                            "Cancelar"
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
                    label { "Origen"
                        select {
                            onchange: edit(|v, text| v.origin = text),
                            for origin in LeadOrigin::ALL {
                                option {
                                    key: "{origin.code()}",
                                    value: "{origin.code()}",
                                    selected: values.origin == origin.code()
                                        || (values.origin.is_empty() && origin == LeadOrigin::Other),
                                    "{origin.label()}"
                                }
                            }
                        }
                        span { class: "hint", "Formulario web, WhatsApp entrante y llamada entrante registran el consentimiento." }
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
