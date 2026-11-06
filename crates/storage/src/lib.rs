//! SQLite adapter for the domain repository port.

use std::{fs, path::Path};

use cominorsa_domain::{Lead, LeadRecord, LeadRepository, RepositoryError};
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
