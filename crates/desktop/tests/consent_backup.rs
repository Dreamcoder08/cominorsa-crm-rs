//! Consent and automatic backups in the window, over the same use cases the
//! CLI proves.

use std::{env, fs, path::PathBuf};

use cominorsa_application::{AppError, AutoBackup, AutoBackupStatus};
use cominorsa_desktop::{
    Agenda, FormValues, LeadForm, SaveState, SettingsView, Workspace, auto_backup_label,
    startup_backup,
};
use cominorsa_domain::{Lead, LeadOrigin, NewLead, Settings, SettingsRepository};
use cominorsa_storage::SqliteLeads;
use dioxus::prelude::*;
use time::{Date, macros::date};
use uuid::Uuid;

const TODAY: Date = date!(2026 - 11 - 06);

fn received(name: &str, origin: LeadOrigin) -> Lead {
    Lead::receive(
        NewLead {
            name: name.to_owned(),
            phone: "900000000".to_owned(),
            ruc: None,
            service: "DAC".to_owned(),
            owner: None,
            follow_up_on: TODAY,
        },
        origin,
        TODAY,
    )
    .unwrap()
}

fn render(component: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(component);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

fn workspace(leads: Vec<Lead>) -> Element {
    rsx! {
        Workspace {
            leads,
            today: TODAY,
            query: String::new(),
            whatsapp_template: "Hola {nombre}".to_owned(),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

fn without_consent() -> Element {
    workspace(vec![received("Referida", LeadOrigin::Referral)])
}

fn with_consent() -> Element {
    workspace(vec![received("Entrante", LeadOrigin::WebForm)])
}

#[test]
fn the_whatsapp_button_refuses_without_consent_with_its_code() {
    let html = render(without_consent);

    assert!(!html.contains("wa.me"), "{html}");
    assert!(html.contains("data-code=\"no_consent\""), "{html}");
    assert!(html.contains("Sin consentimiento"), "{html}");
    assert!(html.contains("Registrar consentimiento"), "{html}");
    assert!(html.contains("Referido"), "{html}");
}

fn agenda_without_consent() -> Element {
    rsx! {
        Agenda {
            leads: vec![received("Referida", LeadOrigin::Referral)],
            today: TODAY,
            whatsapp_template: "Hola {nombre}".to_owned(),
            on_open_pipeline: |_| {},
        }
    }
}

fn agenda_with_consent() -> Element {
    rsx! {
        Agenda {
            leads: vec![received("Entrante", LeadOrigin::WebForm)],
            today: TODAY,
            whatsapp_template: "Hola {nombre}".to_owned(),
            on_open_pipeline: |_| {},
        }
    }
}

/// Neither the board nor the agenda prepares a chat link or shows the prepared
/// message for a lead without consent; the same lead with consent gets both.
#[test]
fn the_whatsapp_link_and_its_message_are_refused_without_consent() {
    for html in [render(without_consent), render(agenda_without_consent)] {
        assert!(html.contains("Referida"), "{html}");
        assert!(!html.contains("wa.me"), "{html}");
        assert!(!html.contains("text="), "{html}");
        assert!(!html.contains("Hola%20Referida"), "{html}");
        assert!(!html.contains("Hola Referida"), "{html}");
    }
    for html in [render(with_consent), render(agenda_with_consent)] {
        assert!(
            html.contains("https://wa.me/51900000000?text=Hola%20Entrante"),
            "{html}"
        );
    }
}

#[test]
fn a_lead_with_consent_opens_the_chat_and_can_revoke_it() {
    let html = render(with_consent);

    assert!(html.contains("https://wa.me/51900000000?text=Hola%20Entrante"));
    assert!(!html.contains("data-code=\"no_consent\""));
    assert!(html.contains("Consentimiento: 06/11/2026"), "{html}");
    assert!(html.contains("Retirar consentimiento"), "{html}");
    assert!(html.contains("Formulario web"), "{html}");
}

fn form() -> Element {
    rsx! {
        LeadForm {
            values: FormValues {
                origin: "inbound_call".to_owned(),
                ..FormValues::default()
            },
            saving: false,
            error: String::new(),
            on_change: |_| {},
            on_submit: |_| {},
            on_close: |_| {},
        }
    }
}

#[test]
fn the_lead_form_asks_for_the_origin() {
    let html = render(form);

    assert!(html.contains("Origen"));
    for origin in LeadOrigin::ALL {
        assert!(html.contains(origin.label()), "{}", origin.label());
    }
    assert!(html.contains("value=\"inbound_call\" selected"), "{html}");
}

fn settings_view() -> Element {
    let mut settings = Settings::default();
    settings.set_backup_directory("/media/usb/copias");
    settings.set_backup_interval_days(3).unwrap();
    settings.set_backup_keep(20).unwrap();

    rsx! {
        SettingsView {
            settings,
            database: "/datos/crm.db".to_owned(),
            save_state: SaveState::Idle,
            backup_notice: String::new(),
            auto_backup_notice: "Copia automática creada en /media/usb/copias/crm-20261106-090000.db".to_owned(),
            pending_restore: None,
            busy: false,
            on_change: |_| {},
            on_pick_folder: |_| {},
            on_backup: |_| {},
            on_pick_restore: |_| {},
            on_confirm_restore: |_| {},
            on_cancel_restore: |_| {},
        }
    }
}

#[test]
fn settings_show_the_automatic_backup_and_its_limits() {
    let html = render(settings_view);

    assert!(html.contains("Copia automática creada en /media/usb/copias/crm-20261106-090000.db"));
    assert!(html.contains("Días entre copias automáticas"), "{html}");
    assert!(html.contains("Copias que se conservan"), "{html}");
    assert!(html.contains("value=\"3\""), "{html}");
    assert!(html.contains("value=\"20\""), "{html}");
}

fn outcome(status: AutoBackupStatus, pruned: usize) -> AutoBackup {
    AutoBackup {
        status,
        dry_run: false,
        folder: Some("/copias".to_owned()),
        path: Some(PathBuf::from("/copias/crm-20261106-090000.db")),
        pruned: (0..pruned)
            .map(|index| PathBuf::from(format!("/copias/vieja-{index}.db")))
            .collect(),
        next_due_at: None,
    }
}

#[test]
fn labels_every_automatic_backup_outcome() {
    assert_eq!(
        auto_backup_label(&Ok(outcome(AutoBackupStatus::NotConfigured, 0))),
        "Copia automática desactivada: elige una carpeta"
    );
    assert_eq!(
        auto_backup_label(&Ok(outcome(AutoBackupStatus::NotDue, 0))),
        "Copia automática: la última copia es reciente"
    );
    assert_eq!(
        auto_backup_label(&Ok(outcome(AutoBackupStatus::Created, 0))),
        "Copia automática creada en /copias/crm-20261106-090000.db"
    );
    assert_eq!(
        auto_backup_label(&Ok(outcome(AutoBackupStatus::Created, 2))),
        "Copia automática creada en /copias/crm-20261106-090000.db. Se borraron 2 copias antiguas"
    );
    assert_eq!(
        auto_backup_label(&Err(AppError::storage(
            "backup_folder_unavailable",
            "No se puede usar la carpeta de copias /usb"
        ))),
        "No se pudo hacer la copia automática: No se puede usar la carpeta de copias /usb"
    );
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn startup_makes_the_due_copy_through_the_use_case() {
    let scratch = Scratch(env::temp_dir().join(format!("cominorsa-auto-{}", Uuid::now_v7())));
    let folder = scratch.0.join("copias");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("notas.txt"), "no es una copia").unwrap();
    let database = scratch.0.join("crm.db");
    let store = SqliteLeads::open(&database).await.unwrap();
    let mut settings = store.load().await.unwrap();
    settings.set_backup_directory(&folder.display().to_string());
    store.persist(&settings).await.unwrap();

    let label = startup_backup(&store, &database).await;

    assert!(label.starts_with("Copia automática creada en "), "{label}");
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
    assert_eq!(
        fs::read_to_string(folder.join("notas.txt")).unwrap(),
        "no es una copia"
    );
    assert!(store.load().await.unwrap().last_backup_at().is_some());
    assert_eq!(
        startup_backup(&store, &database).await,
        "Copia automática: la última copia es reciente"
    );
    store.close().await;
}
