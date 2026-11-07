//! Case list and detail. State and persistence live in `App`.

use cominorsa_domain::{CaseFile, CaseStatus, CaseStep};
use dioxus::prelude::*;
use time::Date;
use uuid::Uuid;

use crate::{
    BillingEdit, BillingPanel, FollowUp, MilestoneValues, case_progress, close_warning,
    closed_case_label, format_date, renewal_note, responsible_label, step_status_label,
    views::STEP_DATE_NOTE,
};

/// The steps of the explicit closing action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseStep {
    Ask,
    Confirm,
    Cancel,
}

#[component]
pub fn Cases(
    cases: Vec<CaseFile>,
    #[props(default)] notice: String,
    on_open: EventHandler<Uuid>,
) -> Element {
    rsx! {
        header { class: "topbar",
            div {
                h1 { "Expedientes" }
                p { class: "subtitle", "Trámites abiertos al firmar un contrato" }
            }
        }
        if !notice.is_empty() {
            p { class: "notice case-notice", role: "status", "{notice}" }
        }
        if cases.is_empty() {
            section { class: "empty",
                h2 { "Sin expedientes" }
                p { "Se abren al avanzar un lead a Contrato firmado." }
            }
        } else {
            div { class: "table-frame",
                table {
                    thead {
                        tr {
                            th { "Cliente" }
                            th { "Servicio" }
                            th { class: "hide-phone", "Responsable" }
                            th { "Avance" }
                            th { span { class: "visually-hidden", "Acción" } }
                        }
                    }
                    tbody {
                        for file in cases {
                            {
                                let id = file.case.id();
                                rsx! {
                                    tr { key: "{id}",
                                        td { strong { "{file.case.client_name()}" } }
                                        td { "{file.case.service()}" }
                                        td { class: "hide-phone", "{responsible_label(file.case.owner())}" }
                                        td {
                                            "{case_progress(&file)}"
                                            if let Some(label) = closed_case_label(&file.case) {
                                                span { class: "tax-line", "{label}" }
                                            }
                                        }
                                        td {
                                            button {
                                                class: "secondary",
                                                onclick: move |_| on_open.call(id),
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
pub fn CaseDetail(
    file: CaseFile,
    today: Date,
    saving: bool,
    error: String,
    #[props(default)] notice: String,
    #[props(default)] total_note: Option<String>,
    #[props(default)] confirming_close: bool,
    #[props(default)] on_close: EventHandler<CloseStep>,
    #[props(default)] draft: MilestoneValues,
    #[props(default)] on_draft: EventHandler<MilestoneValues>,
    #[props(default)] on_billing: EventHandler<BillingEdit>,
    on_back: EventHandler<()>,
    on_toggle: EventHandler<CaseStep>,
    on_due_date: EventHandler<(CaseStep, String)>,
) -> Element {
    let case = file.case.clone();
    let is_open = case.status() == CaseStatus::Open;
    let warning = close_warning(&file);

    rsx! {
        header { class: "topbar",
            div {
                h1 { "{case.client_name()}" }
                p { class: "subtitle", "Expediente abierto el {format_date(case.opened_on())}" }
            }
            div { class: "actions",
                if let Some(label) = closed_case_label(&case) {
                    span { class: "status overdue", "{label}" }
                }
                if is_open && !confirming_close {
                    button {
                        class: "secondary",
                        disabled: saving,
                        onclick: move |_| on_close.call(CloseStep::Ask),
                        "Cerrar expediente"
                    }
                }
                button { class: "secondary", onclick: move |_| on_back.call(()), "Volver a expedientes" }
            }
        }
        if !notice.is_empty() {
            p { class: "notice case-notice", role: "status", "{notice}" }
        }
        if is_open && confirming_close {
            div { class: "confirm case-confirm", role: "alertdialog", aria_label: "Cerrar expediente",
                p { strong { "¿Cerrar el expediente de {case.client_name()}?" } }
                p { "{renewal_note(&case, today)}" }
                if let Some(warning) = warning {
                    p { "{warning}" }
                }
                div { class: "button-row",
                    button {
                        class: "danger",
                        disabled: saving,
                        onclick: move |_| on_close.call(CloseStep::Confirm),
                        "Sí, cerrar expediente"
                    }
                    button {
                        class: "secondary",
                        disabled: saving,
                        onclick: move |_| on_close.call(CloseStep::Cancel),
                        "Cancelar"
                    }
                }
            }
        }
        dl { class: "case-facts",
            div { dt { "Cliente" } dd { "{case.client_name()}" } }
            div { dt { "RUC" } dd { {case.ruc().map_or("Sin RUC", |ruc| ruc.as_str())} } }
            div { dt { "Servicio" } dd { "{case.service()}" } }
            div { dt { "Responsable" } dd { "{responsible_label(case.owner())}" } }
        }
        if !error.is_empty() {
            p { class: "form-error", role: "alert", "{error}" }
        }
        section { class: "agenda-group",
            h2 { "Pasos del trámite" }
            div { class: "agenda-list",
                for step in file.steps.clone() {
                    StepRow { key: "{step.id()}", step, today, saving, on_toggle, on_due_date }
                }
            }
        }
        BillingPanel { file, today, saving, total_note, draft, on_draft, on_edit: on_billing }
    }
}

#[component]
fn StepRow(
    step: CaseStep,
    today: Date,
    saving: bool,
    on_toggle: EventHandler<CaseStep>,
    on_due_date: EventHandler<(CaseStep, String)>,
) -> Element {
    let status = FollowUp::of_step(&step, today);
    let toggled = step.clone();
    let edited = step.clone();

    rsx! {
        div { class: "agenda-row step-row",
            label { class: "step-check",
                input {
                    r#type: "checkbox",
                    checked: step.is_done(),
                    disabled: saving,
                    onchange: move |_| on_toggle.call(toggled.clone()),
                }
                span { "{step.title()}" }
            }
            div { class: "step-side",
                span { class: "status {status.class()}", "{step_status_label(&step, today)}" }
                label { class: "step-date",
                    span { "{STEP_DATE_NOTE}" }
                    input {
                        r#type: "date",
                        value: "{step.due_on()}",
                        disabled: saving,
                        onchange: move |event| on_due_date.call((edited.clone(), event.value())),
                    }
                }
            }
        }
    }
}
