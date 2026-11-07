//! Quote panel of a lead. State and persistence live in `App`.

use cominorsa_domain::{CompanyProfile, IgvRate, Lead, Quote, QuoteStatus, is_superseded};
use dioxus::prelude::*;
use uuid::Uuid;

use crate::{
    QUOTE_TAX_NOTE, QuoteEdit, QuoteLineValues, QuoteValues, company_warning, format_date,
    format_soles, igv_label, quote_preview, quote_status_class,
};

#[component]
pub fn QuotePanel(
    lead: Lead,
    quotes: Vec<Quote>,
    values: QuoteValues,
    company: CompanyProfile,
    igv_rate: IgvRate,
    saving: bool,
    error: String,
    notice: String,
    confirming_send: Option<Uuid>,
    on_change: EventHandler<QuoteValues>,
    on_edit: EventHandler<QuoteEdit>,
    on_load: EventHandler<Quote>,
    on_pdf: EventHandler<Quote>,
    on_confirm_send: EventHandler<Option<Uuid>>,
    on_close: EventHandler<()>,
) -> Element {
    let title = format!("Cotizaciones de {}", lead.client_name());
    let warning = company_warning(&company);
    let has_company_name = company.has_name();
    let preview = quote_preview(&values, igv_rate);
    let editing = values
        .editing
        .and_then(|id| quotes.iter().find(|quote| quote.id() == id))
        .map(Quote::number);
    let edit_values = |update: fn(&mut QuoteValues, String)| {
        let values = values.clone();
        move |event: FormEvent| {
            let mut next = values.clone();
            update(&mut next, event.value());
            on_change.call(next);
        }
    };
    let add_line = {
        let values = values.clone();
        move |_| {
            let mut next = values.clone();
            next.add_line();
            on_change.call(next);
        }
    };
    let clear = {
        let values = values.clone();
        move |_| {
            let mut next = values.clone();
            next.clear();
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
                class: "panel quote-panel",
                role: "dialog",
                aria_modal: "true",
                aria_label: "{title}",
                onclick: move |event| event.stop_propagation(),
                header { class: "panel-header",
                    h2 { "{title}" }
                    button {
                        class: "ghost",
                        r#type: "button",
                        aria_label: "Cerrar",
                        onclick: move |_| on_close.call(()),
                        "✕"
                    }
                }
                p { class: "muted-note", "{QUOTE_TAX_NOTE}" }
                if let Some(warning) = warning {
                    p { class: "warning-note", role: "status", "{warning}" }
                }
                if !notice.is_empty() {
                    p { class: "notice", role: "status", "{notice}" }
                }
                if quotes.is_empty() {
                    p { class: "muted-note", "Aún no hay cotizaciones para este lead." }
                } else {
                    div { class: "quote-list",
                        for quote in quotes.clone() {
                            QuoteRow {
                                key: "{quote.id()}",
                                superseded: is_superseded(&quote, &quotes),
                                confirming: confirming_send == Some(quote.id()),
                                quote,
                                has_company_name,
                                saving,
                                on_edit,
                                on_load,
                                on_pdf,
                                on_confirm_send,
                            }
                        }
                    }
                }
                form {
                    onsubmit: move |event| {
                        event.prevent_default();
                        on_edit.call(QuoteEdit::Save);
                    },
                    if let Some(number) = editing {
                        h3 { "Editar {number}" }
                    } else {
                        h3 { "Nueva cotización" }
                    }
                    div { class: "quote-dates",
                        label { "Fecha de emisión"
                            input {
                                r#type: "date",
                                value: "{values.issued_on}",
                                disabled: saving,
                                oninput: edit_values(|values, text| values.issued_on = text),
                            }
                        }
                        label { "Válida hasta"
                            input {
                                r#type: "date",
                                value: "{values.valid_until}",
                                disabled: saving,
                                oninput: edit_values(|values, text| values.valid_until = text),
                            }
                        }
                    }
                    for (index, line) in values.lines.iter().cloned().enumerate() {
                        QuoteLineFields {
                            key: "{index}",
                            index,
                            line,
                            values: values.clone(),
                            saving,
                            on_change,
                        }
                    }
                    div { class: "quote-actions",
                        button {
                            class: "secondary",
                            r#type: "button",
                            disabled: saving,
                            onclick: add_line,
                            "Agregar línea"
                        }
                    }
                    label { "Notas"
                        textarea {
                            rows: 2,
                            value: "{values.notes}",
                            disabled: saving,
                            oninput: edit_values(|values, text| values.notes = text),
                        }
                    }
                    if let Some(totals) = preview {
                        dl { class: "quote-totals",
                            div { dt { "Subtotal" } dd { "{format_soles(totals.subtotal)}" } }
                            div { dt { "{igv_label(igv_rate)}" } dd { "{format_soles(totals.igv)}" } }
                            div { dt { "Total" } dd { "{format_soles(totals.total)}" } }
                        }
                    } else {
                        p { class: "muted-note", "Completa las líneas para ver el total." }
                    }
                    if !error.is_empty() {
                        p { class: "form-error", role: "alert", "{error}" }
                    }
                    div { class: "quote-actions",
                        button {
                            class: "primary",
                            r#type: "submit",
                            disabled: saving,
                            if saving { "Guardando…" } else { "Guardar borrador" }
                        }
                        if editing.is_some() {
                            button {
                                class: "secondary",
                                r#type: "button",
                                disabled: saving,
                                onclick: clear,
                                "Cancelar edición"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn QuoteLineFields(
    index: usize,
    line: QuoteLineValues,
    values: QuoteValues,
    saving: bool,
    on_change: EventHandler<QuoteValues>,
) -> Element {
    let edit = |update: fn(&mut QuoteLineValues, String)| {
        let values = values.clone();
        move |event: FormEvent| {
            let mut next = values.clone();
            if let Some(line) = next.lines.get_mut(index) {
                update(line, event.value());
            }
            on_change.call(next);
        }
    };
    let can_remove = values.lines.len() > 1;
    let remove = {
        let values = values.clone();
        move |_| {
            let mut next = values.clone();
            next.remove_line(index);
            on_change.call(next);
        }
    };
    let position = index + 1;

    rsx! {
        div { class: "quote-line",
            label { class: "description", "Descripción"
                input {
                    value: "{line.description}",
                    disabled: saving,
                    oninput: edit(|line, text| line.description = text),
                }
            }
            label { "Cantidad"
                input {
                    inputmode: "numeric",
                    value: "{line.quantity}",
                    disabled: saving,
                    oninput: edit(|line, text| line.quantity = text),
                }
            }
            label { "Precio sin IGV"
                input {
                    inputmode: "decimal",
                    placeholder: "1,234.50",
                    value: "{line.unit_price}",
                    disabled: saving,
                    oninput: edit(|line, text| line.unit_price = text),
                }
            }
            if can_remove {
                button {
                    class: "secondary",
                    r#type: "button",
                    aria_label: "Quitar línea {position}",
                    disabled: saving,
                    onclick: remove,
                    "Quitar"
                }
            }
        }
    }
}

#[component]
fn QuoteRow(
    quote: Quote,
    superseded: bool,
    confirming: bool,
    has_company_name: bool,
    saving: bool,
    on_edit: EventHandler<QuoteEdit>,
    on_load: EventHandler<Quote>,
    on_pdf: EventHandler<Quote>,
    on_confirm_send: EventHandler<Option<Uuid>>,
) -> Element {
    let id = quote.id();
    let status = quote.status();
    let is_draft = status == QuoteStatus::Draft;
    let can_decide = status == QuoteStatus::Sent && !superseded;
    let can_version = !is_draft && !superseded;
    let on_click = |edit: fn(Quote) -> QuoteEdit| {
        let quote = quote.clone();
        move |_| on_edit.call(edit(quote.clone()))
    };
    let send = on_click(QuoteEdit::Send);
    let confirmed_send = on_click(QuoteEdit::Send);
    let load = {
        let quote = quote.clone();
        move |_| on_load.call(quote.clone())
    };
    let pdf = {
        let quote = quote.clone();
        move |_| on_pdf.call(quote.clone())
    };

    rsx! {
        div { class: "quote-row",
            div { class: "quote-title",
                strong { "{quote.number()}" }
                span { class: "status {quote_status_class(status)}", "{status.label()}" }
            }
            span { class: "detail",
                "{format_soles(quote.totals().total)} · Válida hasta {format_date(quote.valid_until())}"
            }
            if superseded {
                span { class: "muted-note", "Reemplazada por una versión más reciente" }
            }
            div { class: "quote-actions",
                if is_draft {
                    button {
                        class: "secondary",
                        r#type: "button",
                        disabled: saving,
                        onclick: load,
                        "Editar"
                    }
                    if has_company_name {
                        button {
                            class: "secondary",
                            r#type: "button",
                            disabled: saving,
                            onclick: send,
                            "Marcar enviada"
                        }
                    } else {
                        button {
                            class: "secondary",
                            r#type: "button",
                            disabled: saving,
                            onclick: move |_| on_confirm_send.call(Some(id)),
                            "Marcar enviada"
                        }
                    }
                }
                if can_decide {
                    button {
                        class: "secondary",
                        r#type: "button",
                        disabled: saving,
                        onclick: on_click(QuoteEdit::Accept),
                        "Marcar aceptada"
                    }
                    button {
                        class: "secondary",
                        r#type: "button",
                        disabled: saving,
                        onclick: on_click(QuoteEdit::Reject),
                        "Marcar rechazada"
                    }
                }
                if can_version {
                    button {
                        class: "secondary",
                        r#type: "button",
                        disabled: saving,
                        onclick: on_click(QuoteEdit::NewVersion),
                        "Nueva versión"
                    }
                }
                button {
                    class: "secondary",
                    r#type: "button",
                    disabled: saving,
                    onclick: pdf,
                    "Guardar PDF"
                }
            }
            if confirming {
                div { class: "confirm", role: "alertdialog", aria_label: "Confirmar envío",
                    p { strong { "¿Marcar como enviada sin nombre de empresa?" } }
                    p { "El nombre quedará en blanco en esta versión." }
                    div { class: "button-row",
                        button {
                            class: "danger",
                            r#type: "button",
                            disabled: saving,
                            onclick: confirmed_send,
                            "Enviar sin nombre"
                        }
                        button {
                            class: "secondary",
                            r#type: "button",
                            disabled: saving,
                            onclick: move |_| on_confirm_send.call(None),
                            "Cancelar"
                        }
                    }
                }
            }
        }
    }
}
