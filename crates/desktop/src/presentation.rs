//! Status colors of the desktop views.

use cominorsa_domain::QuoteStatus;

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
