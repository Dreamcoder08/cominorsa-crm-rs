//! SQLite adapter for the domain repository port.

use std::{fs, path::Path};

use cominorsa_domain::{
    Lead, LeadRecord, LeadRepository, Prospect, ProspectKind, ProspectRepository, RepositoryError,
    Settings, SettingsRepository, ThemePreference,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

const UPSERT: &str = "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
    ON CONFLICT (id) DO UPDATE SET
        name = excluded.name,
        phone = excluded.phone,
        ruc = excluded.ruc,
        service = excluded.service,
        stage = excluded.stage,
        follow_up_on = excluded.follow_up_on";

const SELECT_ALL: &str = "SELECT id, name, phone, ruc, service, stage, follow_up_on
    FROM leads ORDER BY follow_up_on, id";

const INSERT_PROSPECT: &str = "INSERT INTO prospects (kind, name, profile, concessions, hectares,
        departments, provinces, substances, concession_states, potential_service, needs_review,
        source)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
    ON CONFLICT (kind, name) DO NOTHING";

const SEARCH_PROSPECTS: &str = "SELECT kind, name, profile, concessions, hectares, departments,
        provinces, substances, concession_states, potential_service, needs_review, source
    FROM prospects
    WHERE name LIKE ?1 ESCAPE '\\' OR name LIKE ?3 ESCAPE '\\'
        OR departments LIKE ?1 ESCAPE '\\' OR departments LIKE ?3 ESCAPE '\\'
        OR provinces LIKE ?1 ESCAPE '\\' OR provinces LIKE ?3 ESCAPE '\\'
        OR potential_service LIKE ?1 ESCAPE '\\' OR potential_service LIKE ?3 ESCAPE '\\'
    ORDER BY needs_review, concessions DESC, name
    LIMIT ?2";

const THEME_KEY: &str = "theme";
const FOLLOW_UP_DAYS_KEY: &str = "follow_up_days";
const WHATSAPP_TEMPLATE_KEY: &str = "whatsapp_template";
const UPSERT_SETTING: &str = "INSERT INTO settings (key, value) VALUES (?1, ?2)
    ON CONFLICT (key) DO UPDATE SET value = excluded.value";

#[derive(Clone)]
pub struct SqliteLeads {
    pool: SqlitePool,
}

impl SqliteLeads {
    /// Opens the database file, creating it and applying pending migrations.
    pub async fn open(path: &Path) -> Result<Self, RepositoryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(failure)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(failure)?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(failure)?;

        Ok(Self { pool })
    }

    /// Writes a consistent copy of the database to a path that must not exist.
    pub async fn backup_to(&self, path: &Path) -> Result<(), RepositoryError> {
        sqlx::query("VACUUM INTO ?1")
            .bind(path.to_string_lossy().into_owned())
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

impl LeadRepository for SqliteLeads {
    async fn save(&self, lead: &Lead) -> Result<(), RepositoryError> {
        let follow_up_on = lead.follow_up_on().to_string();

        sqlx::query(UPSERT)
            .bind(lead.id().to_string())
            .bind(lead.name())
            .bind(lead.phone())
            .bind(lead.ruc().map(|ruc| ruc.as_str()))
            .bind(lead.service())
            .bind(lead.stage().code())
            .bind(follow_up_on)
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        Ok(())
    }

    async fn list(&self) -> Result<Vec<Lead>, RepositoryError> {
        let rows = sqlx::query(SELECT_ALL)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        rows.iter().map(to_lead).collect()
    }
}

impl ProspectRepository for SqliteLeads {
    async fn import(&self, prospects: &[Prospect]) -> Result<usize, RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let mut added = 0;

        for prospect in prospects {
            let result = sqlx::query(INSERT_PROSPECT)
                .bind(prospect.kind.code())
                .bind(&prospect.name)
                .bind(&prospect.profile)
                .bind(i64::from(prospect.concessions))
                .bind(prospect.hectares)
                .bind(&prospect.departments)
                .bind(&prospect.provinces)
                .bind(&prospect.substances)
                .bind(&prospect.concession_states)
                .bind(&prospect.potential_service)
                .bind(prospect.needs_review)
                .bind(&prospect.source)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
            added += result.rows_affected() as usize;
        }

        transaction.commit().await.map_err(failure)?;
        Ok(added)
    }

    async fn count(&self) -> Result<usize, RepositoryError> {
        let row = sqlx::query("SELECT COUNT(*) AS total FROM prospects")
            .fetch_one(&self.pool)
            .await
            .map_err(failure)?;
        let total: i64 = row.try_get("total").map_err(failure)?;

        usize::try_from(total).map_err(failure)
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Prospect>, RepositoryError> {
        let rows = sqlx::query(SEARCH_PROSPECTS)
            .bind(like_pattern(query))
            .bind(i64::from(limit))
            // SQLite folds case for ASCII only, and source data is uppercase.
            .bind(like_pattern(&query.to_uppercase()))
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        rows.iter().map(to_prospect).collect()
    }
}

impl SettingsRepository for SqliteLeads {
    async fn load(&self) -> Result<Settings, RepositoryError> {
        let rows = sqlx::query("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        let mut settings = Settings::default();

        for row in &rows {
            let key: String = row.try_get("key").map_err(failure)?;
            let value: String = row.try_get("value").map_err(failure)?;

            match key.as_str() {
                THEME_KEY => {
                    settings.theme = ThemePreference::from_code(&value)
                        .ok_or_else(|| failure("Tema guardado desconocido"))?;
                }
                FOLLOW_UP_DAYS_KEY => {
                    let days = value.parse().map_err(failure)?;
                    settings.set_follow_up_days(days).map_err(failure)?;
                }
                WHATSAPP_TEMPLATE_KEY => {
                    settings.set_whatsapp_template(&value).map_err(failure)?;
                }
                _ => {}
            }
        }

        let members = sqlx::query("SELECT name FROM team_members ORDER BY position")
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        for member in &members {
            let name: String = member.try_get("name").map_err(failure)?;
            settings.add_member(&name).map_err(failure)?;
        }

        Ok(settings)
    }

    async fn persist(&self, settings: &Settings) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;

        for (key, value) in [
            (THEME_KEY, settings.theme.code().to_owned()),
            (FOLLOW_UP_DAYS_KEY, settings.follow_up_days().to_string()),
            (
                WHATSAPP_TEMPLATE_KEY,
                settings.whatsapp_template().to_owned(),
            ),
        ] {
            sqlx::query(UPSERT_SETTING)
                .bind(key)
                .bind(value)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
        }

        sqlx::query("DELETE FROM team_members")
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;
        for (position, name) in settings.team().iter().enumerate() {
            sqlx::query("INSERT INTO team_members (position, name) VALUES (?1, ?2)")
                .bind(i64::try_from(position).map_err(failure)?)
                .bind(name)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
        }

        transaction.commit().await.map_err(failure)
    }
}

fn like_pattern(query: &str) -> String {
    let escaped = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");

    format!("%{escaped}%")
}

fn to_prospect(row: &SqliteRow) -> Result<Prospect, RepositoryError> {
    let kind: String = row.try_get("kind").map_err(failure)?;
    let concessions: i64 = row.try_get("concessions").map_err(failure)?;

    Ok(Prospect {
        name: row.try_get("name").map_err(failure)?,
        kind: ProspectKind::from_code(&kind)
            .ok_or_else(|| failure("Tipo de prospecto desconocido"))?,
        profile: row.try_get("profile").map_err(failure)?,
        concessions: u32::try_from(concessions).map_err(failure)?,
        hectares: row.try_get("hectares").map_err(failure)?,
        departments: row.try_get("departments").map_err(failure)?,
        provinces: row.try_get("provinces").map_err(failure)?,
        substances: row.try_get("substances").map_err(failure)?,
        concession_states: row.try_get("concession_states").map_err(failure)?,
        potential_service: row.try_get("potential_service").map_err(failure)?,
        needs_review: row.try_get("needs_review").map_err(failure)?,
        source: row.try_get("source").map_err(failure)?,
    })
}

fn to_lead(row: &SqliteRow) -> Result<Lead, RepositoryError> {
    let id: String = row.try_get("id").map_err(failure)?;
    let follow_up_on: String = row.try_get("follow_up_on").map_err(failure)?;

    Lead::restore(LeadRecord {
        id: Uuid::parse_str(&id).map_err(failure)?,
        name: row.try_get("name").map_err(failure)?,
        phone: row.try_get("phone").map_err(failure)?,
        ruc: row.try_get("ruc").map_err(failure)?,
        service: row.try_get("service").map_err(failure)?,
        stage: row.try_get("stage").map_err(failure)?,
        follow_up_on: Date::parse(&follow_up_on, &Iso8601::DATE).map_err(failure)?,
    })
    .map_err(failure)
}

fn failure(error: impl ToString) -> RepositoryError {
    RepositoryError(error.to_string())
}
