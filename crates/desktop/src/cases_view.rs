//! Case list and detail. State and persistence live in `App`.

use cominorsa_domain::{CaseFile, CaseStep};
use dioxus::prelude::*;
use time::Date;
use uuid::Uuid;

use crate::{
    FollowUp, case_progress, format_date, responsible_label, step_status_label,
    views::STEP_DATE_NOTE,
};

#[component]
pub fn Cases(cases: Vec<CaseFile>, on_open: EventHandler<Uuid>) -> Element {
    rsx! {
        header { class: "topbar",
            div {
                h1 { "Expedientes" }
                p { class: "subtitle", "Trámites abiertos al firmar un contrato" }
            }
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
                                        td { "{case_progress(&file)}" }
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
    on_back: EventHandler<()>,
    on_toggle: EventHandler<CaseStep>,
    on_due_date: EventHandler<(CaseStep, String)>,
) -> Element {
    let case = file.case;

    rsx! {
        header { class: "topbar",
            div {
                h1 { "{case.client_name()}" }
                p { class: "subtitle", "Expediente abierto el {format_date(case.opened_on())}" }
            }
            div { class: "actions",
                button { class: "secondary", onclick: move |_| on_back.call(()), "Volver a expedientes" }
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
                for step in file.steps {
                    StepRow { key: "{step.id()}", step, today, saving, on_toggle, on_due_date }
                }
            }
        }
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
