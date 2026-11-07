//! Opening the database, and read-only views of the stored state for assertions
//! and diagnosis. These name the SQLite adapter because schema, integrity, and
//! counters have no port.

use std::path::{Path, PathBuf};

use cominorsa_domain::{
    CaseFile, CaseRepository, Lead, LeadRepository, Prospect, ProspectRepository, Quote,
    QuoteRepository, Settings, SettingsRepository,
};
use cominorsa_storage::{AsideFiles, QuoteCounter, SqliteLeads, UpgradeError};

use crate::{cases::is_active, context::Clock, error::AppError};

fn missing(path: &Path) -> AppError {
    AppError::storage(
        "database_missing",
        format!(
            "No existe la base de datos {}. Créala con: cominorsa init",
            path.display()
        ),
    )
}

/// Refuses a missing database whose file an interrupted upgrade moved aside,
/// so nothing creates a new one in its place.
fn refuse_aside(path: &Path) -> Result<(), AppError> {
    match AsideFiles::find(path) {
        Some(found) => Err(AppError::storage("database_aside", found.to_string())),
        None => Ok(()),
    }
}

fn newer_schema(found: i64, required: i64) -> AppError {
    AppError::storage(
        "schema_newer",
        format!(
            "La base está en la versión {found}, más nueva que la {required} que admite este programa. Actualiza el programa"
        ),
    )
}

/// Creates a new database at the current schema and returns that version. An
/// existing file is refused, whatever it holds.
pub async fn create_database(path: &Path) -> Result<i64, AppError> {
    refuse_aside(path)?;
    if path.exists() {
        return Err(AppError::rejected(
            "database_exists",
            format!("Ya existe {}", path.display()),
        ));
    }

    let store = SqliteLeads::open(path).await?;
    let version = store.schema_version().await?;
    store.close().await;

    Ok(version)
}

/// Opens a database to write in it, under the same checks as reading, so a
/// write can never create or migrate one.
pub async fn open_for_writing(path: &Path) -> Result<SqliteLeads, AppError> {
    open_for_reading(path).await?.close().await;

    Ok(SqliteLeads::open_existing(path).await?)
}

/// Opens a database to read it. It is never created or migrated: a schema that
/// does not match is an error that names the upgrade.
pub async fn open_for_reading(path: &Path) -> Result<SqliteLeads, AppError> {
    refuse_aside(path)?;
    if !path.exists() {
        return Err(missing(path));
    }

    let store = SqliteLeads::open_read_only(path).await?;
    let found = store.schema_version().await?;
    let required = SqliteLeads::required_schema_version();

    if found == required {
        return Ok(store);
    }
    store.close().await;

    Err(if found < required {
        AppError::storage(
            "schema_outdated",
            format!(
                "La base está en la versión {found} y este programa necesita la {required}. Actualízala con: cominorsa migrate"
            ),
        )
    } else {
        newer_schema(found, required)
    })
}

/// What `migrate_database` did. `safety_copy` holds the previous data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Migration {
    pub from: i64,
    pub to: i64,
    pub changed: bool,
    pub safety_copy: Option<PathBuf>,
}

/// Brings a database to the required schema, after copying it beside itself.
/// A database that is already current is not touched. The migrations run on a
/// working copy, so when one fails the database stays as it was. While another
/// program has the database open, nothing is written and the answer is
/// `database_busy`.
pub async fn migrate_database(path: &Path, clock: &Clock) -> Result<Migration, AppError> {
    refuse_aside(path)?;
    if !path.exists() {
        return Err(missing(path));
    }

    let reader = SqliteLeads::open_read_only(path).await?;
    let from = reader.schema_version().await?;
    let to = SqliteLeads::required_schema_version();

    if from > to {
        reader.close().await;
        return Err(newer_schema(from, to));
    }
    if from == to {
        reader.close().await;
        return Ok(Migration {
            from,
            to,
            changed: false,
            safety_copy: None,
        });
    }

    reader.close().await;
    let safety_copy = SqliteLeads::upgrade(path, &clock.file_stamp())
        .await
        .map_err(|error| match error {
            UpgradeError::Busy => AppError::storage(
                "database_busy",
                format!(
                    "Otro programa tiene abierta la base {}. Ciérralo y vuelve a intentar: cominorsa migrate",
                    path.display()
                ),
            ),
            UpgradeError::Failed {
                error,
                safety_copy: Some(copy),
            } => AppError::storage(
                "migration_failed",
                format!(
                    "No se pudo actualizar la base: {error}. Sigue en la versión {from} y su copia está en {}",
                    copy.display()
                ),
            ),
            UpgradeError::Failed {
                error,
                safety_copy: None,
            } => AppError::storage(
                "migration_failed",
                format!(
                    "No se pudo actualizar la base: {error}. Sigue en la versión {from} y no se hizo ninguna copia"
                ),
            ),
        })?;

    Ok(Migration {
        from,
        to,
        changed: true,
        safety_copy: Some(safety_copy),
    })
}

/// Opens the database the app works on. A missing file is created, and an older
/// schema goes through `migrate_database`, so the app has no upgrade of its own
/// and what `cominorsa migrate` proves holds here. A refused or failed upgrade
/// returns its error and leaves the file as it was.
pub async fn open_at_startup(path: &Path, clock: &Clock) -> Result<SqliteLeads, AppError> {
    if path.exists() {
        migrate_database(path, clock).await?;
    } else {
        create_database(path).await?;
    }

    open_for_writing(path).await
}

/// Everything stored, in a stable order, to compare full state.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub schema_version: i64,
    pub leads: Vec<Lead>,
    pub cases: Vec<CaseFile>,
    pub quotes: Vec<Quote>,
    pub quote_counters: Vec<QuoteCounter>,
    pub prospects: Vec<Prospect>,
    pub settings: Settings,
    /// Leads whose stored consent could not be read and load without one.
    pub unreadable_consents: usize,
}

pub async fn snapshot(store: &SqliteLeads) -> Result<Snapshot, AppError> {
    let mut prospects = store.search("", u32::MAX).await?;
    prospects.sort_by(|left, right| {
        (left.kind.code(), &left.name).cmp(&(right.kind.code(), &right.name))
    });

    Ok(Snapshot {
        schema_version: store.schema_version().await?,
        leads: store.list().await?,
        cases: store.all_cases().await?,
        quotes: store.quotes().await?,
        quote_counters: store.quote_counters().await?,
        prospects,
        settings: store.load().await?,
        unreadable_consents: store.unreadable_consents().await?,
    })
}

/// The health of the database and how much it holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub schema_version: i64,
    pub integrity_check: String,
    pub leads: usize,
    pub cases: usize,
    pub active_cases: usize,
    pub case_steps: usize,
    pub billing_milestones: usize,
    pub quotes: usize,
    pub prospects: usize,
    pub team_members: usize,
    /// Leads whose stored consent could not be read and load without one.
    pub unreadable_consents: usize,
}

pub async fn status(store: &SqliteLeads) -> Result<Status, AppError> {
    let cases = store.all_cases().await?;

    Ok(Status {
        schema_version: store.schema_version().await?,
        integrity_check: store.integrity_check().await?,
        leads: store.list().await?.len(),
        cases: cases.len(),
        active_cases: cases.iter().filter(|file| is_active(file)).count(),
        case_steps: cases.iter().map(|file| file.steps.len()).sum(),
        billing_milestones: cases.iter().map(|file| file.milestones.len()).sum(),
        quotes: store.quotes().await?.len(),
        prospects: store.count().await?,
        team_members: store.load().await?.team().len(),
        unreadable_consents: store.unreadable_consents().await?,
    })
}
