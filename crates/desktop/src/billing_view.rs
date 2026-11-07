//! Billing section of a case. State and persistence live in `App`.

use cominorsa_domain::{BillingMilestone, BillingSummary, CaseFile};
use dioxus::prelude::*;
use time::Date;

use crate::{
    BillingEdit, FollowUp, MilestoneValues, format_soles, milestone_gap_warning,
    milestone_status_label, money_input, views::STEP_DATE_NOTE,
};

const NO_INVOICES_NOTE: &str = "Registra cobros; no emite facturas ni comprobantes.";
const PLAN_NOTE: &str = "Sugerencia editable: el reparto viene de un ejemplo sin validar.";
const UNDEFINED_TOTAL: &str = "Sin definir";

#[component]
pub fn BillingPanel(
    file: CaseFile,
    today: Date,
    saving: bool,
    #[props(default)] total_note: Option<String>,
    draft: MilestoneValues,
    on_draft: EventHandler<MilestoneValues>,
    on_edit: EventHandler<BillingEdit>,
) -> Element {
    let summary = BillingSummary::of(&file);
    let gap_warning = milestone_gap_warning(&file);
    let total_label = summary
        .total
        .map_or_else(|| UNDEFINED_TOTAL.to_owned(), format_soles);
    let total_input = summary.total.map_or_else(String::new, money_input);
    let can_plan = summary.total.is_some() && file.milestones.is_empty();
    let edit_draft = |update: fn(&mut MilestoneValues, String)| {
        let draft = draft.clone();
        move |event: FormEvent| {
            let mut next = draft.clone();
            update(&mut next, event.value());
            on_draft.call(next);
        }
    };

    rsx! {
        section { class: "agenda-group",
            h2 { "Cobros" }
            p { class: "muted-note billing-note", "{NO_INVOICES_NOTE}" }
            dl { class: "case-facts",
                div { dt { "Total del contrato" } dd { "{total_label}" } }
                div { dt { "Cobrado" } dd { "{format_soles(summary.collected)}" } }
                div { dt { "Pendiente" } dd { "{format_soles(summary.pending)}" } }
            }
            if let Some(note) = total_note {
                p { class: "muted-note billing-note", "{note}" }
            }
            if let Some(warning) = gap_warning {
                p { class: "warning-note billing-note", role: "status", "{warning}" }
            }
            label { class: "step-date billing-total",
                span { "Editar total en soles" }
                input {
                    inputmode: "decimal",
                    placeholder: "1,234.50",
                    value: "{total_input}",
                    disabled: saving,
                    onchange: move |event| on_edit.call(BillingEdit::Total(event.value())),
                }
            }
            if can_plan {
                div { class: "billing-plan",
                    button {
                        class: "secondary",
                        disabled: saving,
                        onclick: move |_| on_edit.call(BillingEdit::Suggest),
                        "Aplicar plan sugerido 30 / 40 / 30"
                    }
                    span { class: "muted-note", "{PLAN_NOTE}" }
                }
            }
            if !file.milestones.is_empty() {
                div { class: "agenda-list",
                    for milestone in file.milestones {
                        MilestoneRow { key: "{milestone.id()}", milestone, today, saving, on_edit }
                    }
                }
            }
            form {
                class: "milestone-form",
                onsubmit: move |event| {
                    event.prevent_default();
                    on_edit.call(BillingEdit::Add);
                },
                label { class: "step-date",
                    span { "Concepto" }
                    input {
                        value: "{draft.title}",
                        disabled: saving,
                        oninput: edit_draft(|values, text| values.title = text),
                    }
                }
                label { class: "step-date",
                    span { "Monto en soles" }
                    input {
                        inputmode: "decimal",
                        value: "{draft.amount}",
                        disabled: saving,
                        oninput: edit_draft(|values, text| values.amount = text),
                    }
                }
                label { class: "step-date",
                    span { "Vence" }
                    input {
                        r#type: "date",
                        value: "{draft.due_on}",
                        disabled: saving,
                        oninput: edit_draft(|values, text| values.due_on = text),
                    }
                }
                button { class: "primary", r#type: "submit", disabled: saving, "Agregar hito" }
            }
        }
    }
}

#[component]
fn MilestoneRow(
    milestone: BillingMilestone,
    today: Date,
    saving: bool,
    on_edit: EventHandler<BillingEdit>,
) -> Element {
    let status = FollowUp::of_milestone(&milestone, today);
    let id = milestone.id();
    let toggled = milestone.clone();
    let titled = milestone.clone();
    let priced = milestone.clone();
    let moved = milestone.clone();

    rsx! {
        div { class: "agenda-row step-row",
            div { class: "milestone-main",
                input {
                    class: "milestone-paid",
                    r#type: "checkbox",
                    aria_label: "Marcar {milestone.title()} como pagado",
                    checked: milestone.is_paid(),
                    disabled: saving,
                    onchange: move |_| on_edit.call(BillingEdit::TogglePaid(toggled.clone())),
                }
                input {
                    aria_label: "Concepto del hito",
                    value: "{milestone.title()}",
                    disabled: saving,
                    onchange: move |event| on_edit.call(BillingEdit::Title(titled.clone(), event.value())),
                }
                input {
                    class: "milestone-amount",
                    inputmode: "decimal",
                    aria_label: "Monto de {milestone.title()} en soles",
                    value: "{money_input(milestone.amount())}",
                    disabled: saving,
                    onchange: move |event| on_edit.call(BillingEdit::Amount(priced.clone(), event.value())),
                }
            }
            div { class: "step-side",
                span { class: "status {status.class()}", "{milestone_status_label(&milestone, today)}" }
                label { class: "step-date",
                    span { "{STEP_DATE_NOTE}" }
                    input {
                        r#type: "date",
                        value: "{milestone.due_on()}",
                        disabled: saving,
                        onchange: move |event| on_edit.call(BillingEdit::DueDate(moved.clone(), event.value())),
                    }
                }
                button {
                    class: "secondary",
                    aria_label: "Eliminar {milestone.title()}",
                    disabled: saving,
                    onclick: move |_| on_edit.call(BillingEdit::Delete(id)),
                    "Eliminar"
                }
            }
        }
    }
}
