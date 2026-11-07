use std::future::Future;

use crate::{DomainError, IgvRate, RepositoryError, Ruc, whatsapp};

const DEFAULT_FOLLOW_UP_DAYS: u32 = 3;
const DEFAULT_WHATSAPP_TEMPLATE: &str =
    "Hola {nombre}, te escribimos de COMINORSA sobre {servicio}. ¿Podemos conversar?";
const FOLLOW_UP_DAYS: std::ops::RangeInclusive<u32> = 1..=90;
const DEFAULT_BACKUP_INTERVAL_DAYS: u32 = 1;
const DEFAULT_BACKUP_KEEP: u32 = 14;
const BACKUP_INTERVAL_DAYS: std::ops::RangeInclusive<u32> = 1..=30;
const BACKUP_KEEP: std::ops::RangeInclusive<u32> = 1..=90;
const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// Stable value used for persistence and as the UI theme attribute.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Sistema",
            Self::Light => "Claro",
            Self::Dark => "Oscuro",
        }
    }
}

/// The company that issues quotes. Nothing is prefilled: a blank field stays
/// blank and is left out of the quote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompanyProfile {
    pub name: String,
    pub ruc: String,
    pub address: String,
    pub phone: String,
    pub email: String,
}

impl CompanyProfile {
    pub fn has_name(&self) -> bool {
        !self.name.trim().is_empty()
    }

    fn trimmed(self) -> Self {
        Self {
            name: self.name.trim().to_owned(),
            ruc: self.ruc.trim().to_owned(),
            address: self.address.trim().to_owned(),
            phone: self.phone.trim().to_owned(),
            email: self.email.trim().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemePreference,
    team: Vec<String>,
    follow_up_days: u32,
    whatsapp_template: String,
    backup_directory: Option<String>,
    last_backup_at: Option<i64>,
    backup_interval_days: u32,
    backup_keep: u32,
    company: CompanyProfile,
    igv_rate: IgvRate,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            team: Vec::new(),
            follow_up_days: DEFAULT_FOLLOW_UP_DAYS,
            whatsapp_template: DEFAULT_WHATSAPP_TEMPLATE.to_owned(),
            backup_directory: None,
            last_backup_at: None,
            backup_interval_days: DEFAULT_BACKUP_INTERVAL_DAYS,
            backup_keep: DEFAULT_BACKUP_KEEP,
            company: CompanyProfile::default(),
            igv_rate: IgvRate::default(),
        }
    }
}

impl Settings {
    pub fn add_member(&mut self, name: &str) -> Result<(), DomainError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DomainError::MissingField("nombre"));
        }
        if self
            .team
            .iter()
            .any(|member| member.eq_ignore_ascii_case(name))
        {
            return Err(DomainError::DuplicateMember);
        }

        self.team.push(name.to_owned());
        Ok(())
    }

    pub fn remove_member(&mut self, name: &str) {
        self.team.retain(|member| member != name);
    }

    pub fn set_follow_up_days(&mut self, days: u32) -> Result<(), DomainError> {
        if !FOLLOW_UP_DAYS.contains(&days) {
            return Err(DomainError::InvalidFollowUpDays);
        }

        self.follow_up_days = days;
        Ok(())
    }

    pub fn set_whatsapp_template(&mut self, template: &str) -> Result<(), DomainError> {
        let template = template.trim();
        if template.is_empty() {
            return Err(DomainError::MissingField("mensaje"));
        }
        if !whatsapp::has_only_known_placeholders(template) {
            return Err(DomainError::UnknownPlaceholder);
        }

        self.whatsapp_template = template.to_owned();
        Ok(())
    }

    /// Rehydrates a stored template. Validation applies to edits only, so a
    /// template saved under older rules still loads and can be corrected.
    pub fn restore_whatsapp_template(&mut self, template: String) {
        if !template.trim().is_empty() {
            self.whatsapp_template = template;
        }
    }

    pub fn whatsapp_template(&self) -> &str {
        &self.whatsapp_template
    }

    pub fn set_backup_directory(&mut self, directory: &str) {
        let directory = directory.trim();
        self.backup_directory = (!directory.is_empty()).then(|| directory.to_owned());
    }

    pub fn backup_directory(&self) -> Option<&str> {
        self.backup_directory.as_deref()
    }

    /// Stores when the last backup finished, in Unix seconds.
    pub fn record_backup(&mut self, unix_seconds: i64) {
        self.last_backup_at = Some(unix_seconds);
    }

    pub fn last_backup_at(&self) -> Option<i64> {
        self.last_backup_at
    }

    pub fn set_backup_interval_days(&mut self, days: u32) -> Result<(), DomainError> {
        if !BACKUP_INTERVAL_DAYS.contains(&days) {
            return Err(DomainError::InvalidBackupInterval);
        }

        self.backup_interval_days = days;
        Ok(())
    }

    /// Rehydrates a stored interval. An unreadable or out-of-range value keeps
    /// the default, to be corrected in Settings, and never blocks the app.
    pub fn restore_backup_interval_days(&mut self, stored: &str) {
        if let Ok(days) = stored.parse() {
            let _ = self.set_backup_interval_days(days);
        }
    }

    pub fn backup_interval_days(&self) -> u32 {
        self.backup_interval_days
    }

    pub fn set_backup_keep(&mut self, copies: u32) -> Result<(), DomainError> {
        if !BACKUP_KEEP.contains(&copies) {
            return Err(DomainError::InvalidBackupKeep);
        }

        self.backup_keep = copies;
        Ok(())
    }

    /// Rehydrates a stored count, like `restore_backup_interval_days`.
    pub fn restore_backup_keep(&mut self, stored: &str) {
        if let Ok(copies) = stored.parse() {
            let _ = self.set_backup_keep(copies);
        }
    }

    /// How many automatic copies stay in the folder, newest first.
    pub fn backup_keep(&self) -> u32 {
        self.backup_keep
    }

    /// When the last backup stops being recent enough, in Unix seconds. `None`
    /// without a folder or before the first backup.
    pub fn next_backup_due_at(&self) -> Option<i64> {
        self.backup_directory.as_ref()?;
        let interval = i64::from(self.backup_interval_days) * SECONDS_PER_DAY;

        self.last_backup_at
            .map(|last| last.saturating_add(interval))
    }

    /// An automatic backup is due when a folder is set and the last backup is
    /// older than the interval, or there is none yet.
    pub fn backup_due(&self, now_unix: i64) -> bool {
        if self.backup_directory.is_none() {
            return false;
        }

        self.next_backup_due_at()
            .is_none_or(|due_at| now_unix > due_at)
    }

    /// Trims every field. A RUC, when given, must be valid.
    pub fn set_company(&mut self, company: CompanyProfile) -> Result<(), DomainError> {
        let company = company.trimmed();
        if !company.ruc.is_empty() {
            Ruc::parse(&company.ruc)?;
        }

        self.company = company;
        Ok(())
    }

    /// Rehydrates a stored profile. Validation applies to edits only, so
    /// values saved under older rules still load and can be corrected.
    pub fn restore_company(&mut self, company: CompanyProfile) {
        self.company = company;
    }

    pub fn company(&self) -> &CompanyProfile {
        &self.company
    }

    pub fn set_igv_rate(&mut self, rate: IgvRate) {
        self.igv_rate = rate;
    }

    /// Rehydrates a stored rate. An unreadable value keeps the default, to be
    /// corrected in Settings, and never blocks the app from opening.
    pub fn restore_igv_rate(&mut self, stored: &str) {
        let stored = stored.parse().map(IgvRate::from_basis_points);
        if let Ok(Ok(rate)) = stored {
            self.igv_rate = rate;
        }
    }

    pub fn igv_rate(&self) -> IgvRate {
        self.igv_rate
    }

    pub fn team(&self) -> &[String] {
        &self.team
    }

    pub fn follow_up_days(&self) -> u32 {
        self.follow_up_days
    }
}

/// Settings persistence port.
pub trait SettingsRepository: Send + Sync {
    fn load(&self) -> impl Future<Output = Result<Settings, RepositoryError>> + Send;

    fn persist(
        &self,
        settings: &Settings,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}
