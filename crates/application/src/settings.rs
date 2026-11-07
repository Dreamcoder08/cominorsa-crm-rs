//! Changing several settings as one unit.

use cominorsa_domain::{
    CompanyProfile, IgvRate, Settings, SettingsRepository, ThemePreference, render_message,
};

use crate::{error::AppError, presentation::igv_input};

const PREVIEW_NAME: &str = "Minera Ejemplo";
const PREVIEW_SERVICE: &str = "REINFO";

/// What a template says to a sample client.
pub fn preview_message(template: &str) -> String {
    render_message(template, PREVIEW_NAME, PREVIEW_SERVICE)
}

/// Previews a candidate template, or the stored one, without saving anything.
pub async fn whatsapp_preview<R: SettingsRepository>(
    repository: &R,
    template: Option<&str>,
) -> Result<String, AppError> {
    let mut settings = repository.load().await?;
    if let Some(template) = template {
        settings.set_whatsapp_template(template)?;
    }

    Ok(preview_message(settings.whatsapp_template()))
}

/// The settings to change. Anything left out stays as stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsChange {
    pub theme: Option<String>,
    pub follow_up_days: Option<u32>,
    pub whatsapp_template: Option<String>,
    pub igv: Option<String>,
    pub backup_directory: Option<String>,
    pub company_name: Option<String>,
    pub company_ruc: Option<String>,
    pub company_address: Option<String>,
    pub company_phone: Option<String>,
    pub company_email: Option<String>,
    pub add_members: Vec<String>,
    pub remove_members: Vec<String>,
}

impl SettingsChange {
    /// What changed between two states, for an editor that holds whole settings.
    pub fn between(before: &Settings, after: &Settings) -> Self {
        let changed = |old: &str, new: &str| (old != new).then(|| new.to_owned());
        let missing_from = |team: &[String], others: &[String]| -> Vec<String> {
            team.iter()
                .filter(|name| !others.contains(name))
                .cloned()
                .collect()
        };
        let (old, new) = (before.company(), after.company());

        Self {
            theme: changed(before.theme.code(), after.theme.code()),
            follow_up_days: (before.follow_up_days() != after.follow_up_days())
                .then(|| after.follow_up_days()),
            whatsapp_template: changed(before.whatsapp_template(), after.whatsapp_template()),
            igv: (before.igv_rate() != after.igv_rate()).then(|| igv_input(after.igv_rate())),
            backup_directory: changed(
                before.backup_directory().unwrap_or_default(),
                after.backup_directory().unwrap_or_default(),
            ),
            company_name: changed(&old.name, &new.name),
            company_ruc: changed(&old.ruc, &new.ruc),
            company_address: changed(&old.address, &new.address),
            company_phone: changed(&old.phone, &new.phone),
            company_email: changed(&old.email, &new.email),
            add_members: missing_from(after.team(), before.team()),
            remove_members: missing_from(before.team(), after.team()),
        }
    }

    fn company(&self, current: &CompanyProfile) -> Option<CompanyProfile> {
        let fields = [
            &self.company_name,
            &self.company_ruc,
            &self.company_address,
            &self.company_phone,
            &self.company_email,
        ];
        if fields.iter().all(|field| field.is_none()) {
            return None;
        }

        let pick = |field: &Option<String>, stored: &str| {
            field.clone().unwrap_or_else(|| stored.to_owned())
        };

        Some(CompanyProfile {
            name: pick(&self.company_name, &current.name),
            ruc: pick(&self.company_ruc, &current.ruc),
            address: pick(&self.company_address, &current.address),
            phone: pick(&self.company_phone, &current.phone),
            email: pick(&self.company_email, &current.email),
        })
    }

    fn apply(&self, settings: &mut Settings) -> Result<(), AppError> {
        if let Some(code) = &self.theme {
            settings.theme = ThemePreference::from_code(code)
                .ok_or_else(|| AppError::rejected("invalid_theme", "Ese tema no existe"))?;
        }
        if let Some(days) = self.follow_up_days {
            settings.set_follow_up_days(days)?;
        }
        if let Some(template) = &self.whatsapp_template {
            settings.set_whatsapp_template(template)?;
        }
        if let Some(text) = &self.igv {
            settings.set_igv_rate(IgvRate::parse(text)?);
        }
        if let Some(directory) = &self.backup_directory {
            settings.set_backup_directory(directory);
        }
        if let Some(company) = self.company(settings.company()) {
            settings.set_company(company)?;
        }
        for name in &self.add_members {
            settings.add_member(name)?;
        }
        for name in &self.remove_members {
            if !settings.team().contains(name) {
                return Err(AppError::rejected(
                    "member_not_found",
                    format!("{name} no está en el equipo"),
                ));
            }
            settings.remove_member(name);
        }

        Ok(())
    }
}

/// Validates every change first, so a refused request stores nothing.
pub async fn update_settings<R: SettingsRepository>(
    repository: &R,
    change: SettingsChange,
) -> Result<Settings, AppError> {
    let mut settings = repository.load().await?;
    change.apply(&mut settings)?;
    repository.persist(&settings).await?;

    Ok(settings)
}
