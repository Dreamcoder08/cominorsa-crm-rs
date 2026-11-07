//! Status colors and labels of the desktop views.

use cominorsa_application::{AppError, AutoBackup, AutoBackupStatus};
use cominorsa_domain::{Lead, QuoteStatus};

use crate::format_date;

pub fn tax_status_class(status: &str) -> &'static str {
    if status == "ACTIVO" {
        "closed"
    } else {
        "overdue"
    }
}

pub fn quote_status_class(status: QuoteStatus) -> &'static str {
    match status {
        QuoteStatus::Draft => "upcoming",
        QuoteStatus::Sent => "today",
        QuoteStatus::Accepted => "closed",
        QuoteStatus::Rejected => "overdue",
    }
}

/// What Settings says about the automatic backup made at startup.
pub fn auto_backup_label(outcome: &Result<AutoBackup, AppError>) -> String {
    let done = match outcome {
        Ok(done) => done,
        Err(failure) => return format!("No se pudo hacer la copia automática: {failure}"),
    };
    let path = done
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    match (done.status, done.pruned.len()) {
        (AutoBackupStatus::NotConfigured, _) => {
            "Copia automática desactivada: elige una carpeta".to_owned()
        }
        (AutoBackupStatus::NotDue, _) => "Copia automática: la última copia es reciente".to_owned(),
        (AutoBackupStatus::WouldCreate, _) => format!("Se crearía una copia en {path}"),
        (AutoBackupStatus::Created, 0) => format!("Copia automática creada en {path}"),
        (AutoBackupStatus::Created, 1) => {
            format!("Copia automática creada en {path}. Se borró 1 copia antigua")
        }
        (AutoBackupStatus::Created, count) => {
            format!("Copia automática creada en {path}. Se borraron {count} copias antiguas")
        }
    }
}

/// The consent line of a lead card.
pub fn consent_label(lead: &Lead) -> String {
    match lead.consent() {
        None => "Sin consentimiento".to_owned(),
        Some(consent) if consent.given => {
            format!("Consentimiento: {}", format_date(consent.recorded_on))
        }
        Some(consent) => format!(
            "Consentimiento retirado: {}",
            format_date(consent.recorded_on)
        ),
    }
}
