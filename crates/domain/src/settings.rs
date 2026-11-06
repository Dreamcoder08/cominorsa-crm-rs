use std::future::Future;

use crate::{DomainError, RepositoryError};

const DEFAULT_FOLLOW_UP_DAYS: u32 = 3;
const DEFAULT_WHATSAPP_TEMPLATE: &str =
    "Hola {nombre}, te escribimos de COMINORSA sobre {servicio}. ¿Podemos conversar?";
const FOLLOW_UP_DAYS: std::ops::RangeInclusive<u32> = 1..=90;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemePreference,
    team: Vec<String>,
    follow_up_days: u32,
    whatsapp_template: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            team: Vec::new(),
            follow_up_days: DEFAULT_FOLLOW_UP_DAYS,
            whatsapp_template: DEFAULT_WHATSAPP_TEMPLATE.to_owned(),
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

        self.whatsapp_template = template.to_owned();
        Ok(())
    }

    pub fn whatsapp_template(&self) -> &str {
        &self.whatsapp_template
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
