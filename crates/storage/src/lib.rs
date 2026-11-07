//! SQLite adapter for the domain repository port.

use std::{fs, path::Path};

use cominorsa_domain::{
    Lead, LeadRecord, LeadRepository, Prospect, ProspectKind, ProspectRepository, RepositoryError,
    Settings, SettingsRepository, TaxRecord, ThemePreference,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

const UPSERT: &str = "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on, owner)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
    ON CONFLICT (id) DO UPDATE SET
        name = excluded.name,
        phone = excluded.phone,
        ruc = excluded.ruc,
        service = excluded.service,
        stage = excluded.stage,
        follow_up_on = excluded.follow_up_on,
        owner = excluded.owner";

const SELECT_ALL: &str = "SELECT id, name, phone, ruc, service, stage, follow_up_on, owner
    FROM leads ORDER BY follow_up_on, id";

const INSERT_PROSPECT: &str = "INSERT INTO prospects (kind, name, profile, concessions, hectares,
        departments, provinces, substances, concession_states, potential_service, needs_review,
        source)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
    ON CONFLICT (kind, name) DO NOTHING";

const APPLY_TAX_RECORD: &str = "UPDATE prospects SET ruc = ?2, tax_status = ?3,
        tax_condition = ?4, fiscal_address = ?5, tax_match_approximate = ?6
    WHERE kind = 'company' AND name = ?1";

const SEARCH_PROSPECTS: &str = "SELECT kind, name, profile, concessions, hectares, departments,
        provinces, substances, concession_states, potential_service, needs_review, source,
        ruc, tax_status, tax_condition, fiscal_address, tax_match_approximate
    FROM prospects
    WHERE name LIKE ?1 ESCAPE '\\' OR name LIKE ?3 ESCAPE '\\'
        OR departments LIKE ?1 ESCAPE '\\' OR departments LIKE ?3 ESCAPE '\\'
        OR provinces LIKE ?1 ESCAPE '\\' OR provinces LIKE ?3 ESCAPE '\\'
        OR potential_service LIKE ?1 ESCAPE '\\' OR potential_service LIKE ?3 ESCAPE '\\'
        OR ruc LIKE ?1 ESCAPE '\\'
    ORDER BY needs_review, concessions DESC, name
    LIMIT ?2";

const CRM_TABLES: i64 = 2;
const CRM_TABLE_COUNT: &str = "SELECT COUNT(*) FROM sqlite_master
    WHERE type = 'table' AND name IN ('leads', '_sqlx_migrations')";

const THEME_KEY: &str = "theme";
const FOLLOW_UP_DAYS_KEY: &str = "follow_up_days";
const WHATSAPP_TEMPLATE_KEY: &str = "whatsapp_template";
const BACKUP_DIRECTORY_KEY: &str = "backup_directory";
const LAST_BACKUP_AT_KEY: &str = "last_backup_at";
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

    /// Replaces the database with a backup and returns the reopened store.
    ///
    /// The backup is validated on a working copy, so an invalid file changes
    /// nothing. Current data is copied to `safety` before it is replaced.
    pub async fn restore(
        self,
        database: &Path,
        backup: &Path,
        safety: &Path,
    ) -> Result<Self, RepositoryError> {
        let working = database.with_extension("restoring.db");

        if let Err(error) = validate_backup(backup, &working).await {
            remove_with_sidecars(&working);
            return Err(error);
        }

        if let Err(error) = self.backup_to(safety).await {
            remove_with_sidecars(&working);
            return Err(error);
        }

        self.close().await;
        remove_sidecars(database);
        fs::rename(&working, database).map_err(|error| {
            failure(format!(
                "{error}. Los datos anteriores están en {}",
                safety.display()
            ))
        })?;

        Self::open(database).await
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// `open` would create the schema in any file, so the CRM tables must already
/// exist before migrations run.
async fn ensure_crm_schema(path: &Path) -> Result<(), RepositoryError> {
    let options = SqliteConnectOptions::new().filename(path);
    let pool = SqlitePool::connect_with(options).await.map_err(failure)?;
    let found = sqlx::query_scalar::<_, i64>(CRM_TABLE_COUNT)
        .fetch_one(&pool)
        .await;
    pool.close().await;

    match found.map_err(failure)? {
        CRM_TABLES => Ok(()),
        _ => Err(failure("El archivo no es una copia de COMINORSA CRM")),
    }
}

async fn validate_backup(backup: &Path, working: &Path) -> Result<(), RepositoryError> {
    fs::copy(backup, working).map_err(failure)?;
    ensure_crm_schema(working).await?;

    let store = SqliteLeads::open(working).await?;
    let checked = async {
        store.list().await?;
        store.count().await?;
        store.load().await
    }
    .await;
    store.close().await;

    checked.map(|_| ())
}

fn remove_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = fs::remove_file(name);
    }
}

fn remove_with_sidecars(path: &Path) {
    let _ = fs::remove_file(path);
    remove_sidecars(path);
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
            .bind(lead.owner())
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

    async fn apply_tax_records(
        &self,
        records: &[(String, TaxRecord)],
    ) -> Result<usize, RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let mut updated = 0;

        for (name, record) in records {
            let result = sqlx::query(APPLY_TAX_RECORD)
                .bind(name)
                .bind(&record.ruc)
                .bind(&record.status)
                .bind(&record.condition)
                .bind(&record.address)
                .bind(record.approximate)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
            updated += result.rows_affected() as usize;
        }

        transaction.commit().await.map_err(failure)?;
        Ok(updated)
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
                    settings.restore_whatsapp_template(value);
                }
                BACKUP_DIRECTORY_KEY => settings.set_backup_directory(&value),
                LAST_BACKUP_AT_KEY => settings.record_backup(value.parse().map_err(failure)?),
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

        let backup = [
            (
                BACKUP_DIRECTORY_KEY,
                settings.backup_directory().map(str::to_owned),
            ),
            (
                LAST_BACKUP_AT_KEY,
                settings.last_backup_at().map(|at| at.to_string()),
            ),
        ];
        for (key, value) in backup {
            let Some(value) = value else {
                sqlx::query("DELETE FROM settings WHERE key = ?1")
                    .bind(key)
                    .execute(&mut *transaction)
                    .await
                    .map_err(failure)?;
                continue;
            };
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
        tax: to_tax_record(row)?,
    })
}

fn to_tax_record(row: &SqliteRow) -> Result<Option<TaxRecord>, RepositoryError> {
    let ruc: Option<String> = row.try_get("ruc").map_err(failure)?;
    let Some(ruc) = ruc else {
        return Ok(None);
    };
    let text = |column: &str| -> Result<String, RepositoryError> {
        let value: Option<String> = row.try_get(column).map_err(failure)?;
        Ok(value.unwrap_or_default())
    };
    let approximate: Option<bool> = row.try_get("tax_match_approximate").map_err(failure)?;

    Ok(Some(TaxRecord {
        ruc,
        status: text("tax_status")?,
        condition: text("tax_condition")?,
        address: text("fiscal_address")?,
        approximate: approximate.unwrap_or(false),
    }))
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
        owner: row.try_get("owner").map_err(failure)?,
        stage: row.try_get("stage").map_err(failure)?,
        follow_up_on: Date::parse(&follow_up_on, &Iso8601::DATE).map_err(failure)?,
    })
    .map_err(failure)
}

fn failure(error: impl ToString) -> RepositoryError {
    RepositoryError(error.to_string())
}
