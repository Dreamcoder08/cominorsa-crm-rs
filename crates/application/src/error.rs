//! Failures of a use case, with a stable code for machines and a Spanish
//! message for people.

use std::fmt;

use cominorsa_domain::{DomainError, RepositoryError};

/// Who has to act: a person changes the request, or someone repairs storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Rejected,
    Storage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppError {
    kind: ErrorKind,
    code: &'static str,
    message: String,
}

impl AppError {
    pub fn rejected(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Rejected,
            code,
            message: message.into(),
        }
    }

    pub fn storage(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Storage,
            code,
            message: message.into(),
        }
    }

    /// An id seed that already produced a different record.
    pub(crate) fn seed_reused(message: &str) -> Self {
        Self::rejected("id_seed_reused", message)
    }

    pub(crate) fn invalid_date(message: &str) -> Self {
        Self::rejected("invalid_date", message)
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl PartialEq<&str> for AppError {
    fn eq(&self, other: &&str) -> bool {
        self.message == *other
    }
}

impl From<DomainError> for AppError {
    fn from(error: DomainError) -> Self {
        let code = match error {
            DomainError::MissingField(_) => "missing_field",
            DomainError::InvalidRuc => "invalid_ruc",
            DomainError::FinalStage => "final_stage",
            DomainError::CorruptRecord => {
                return Self::storage("corrupt_record", error.to_string());
            }
            DomainError::DuplicateMember => "duplicate_member",
            DomainError::InvalidFollowUpDays => "invalid_follow_up_days",
            DomainError::UnknownPlaceholder => "unknown_placeholder",
            DomainError::ContractNotSigned => "contract_not_signed",
            DomainError::InvalidAmount => "invalid_amount",
            DomainError::MissingContractTotal => "missing_contract_total",
            DomainError::MilestonesAlreadyPlanned => "milestones_already_planned",
            DomainError::EmptyQuote => "empty_quote",
            DomainError::InvalidQuantity => "invalid_quantity",
            DomainError::InvalidQuoteValidity => "invalid_quote_validity",
            DomainError::InvalidIgvRate => "invalid_igv_rate",
            DomainError::QuoteLocked => "quote_locked",
            DomainError::InvalidQuoteTransition => "invalid_quote_transition",
            DomainError::NoConsent => "no_consent",
            DomainError::NoWhatsappNumber => "no_whatsapp_number",
            DomainError::InvalidBackupInterval => "invalid_backup_interval",
            DomainError::InvalidBackupKeep => "invalid_backup_keep",
        };

        Self::rejected(code, error.to_string())
    }
}

impl From<RepositoryError> for AppError {
    fn from(error: RepositoryError) -> Self {
        Self::storage("storage_error", error.to_string())
    }
}
