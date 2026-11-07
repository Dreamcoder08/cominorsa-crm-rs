//! Copies of the database. SQLite backup has no port, so this names the adapter.

use std::{
    fs,
    path::{Path, PathBuf},
};

use cominorsa_domain::SettingsRepository;
use cominorsa_storage::SqliteLeads;

use crate::{
    context::{Clock, Context},
    error::AppError,
};

/// Writes a timestamped copy to the chosen folder and returns its path.
pub async fn create_backup(
    store: &SqliteLeads,
    folder: &str,
    clock: &Clock,
) -> Result<PathBuf, AppError> {
    fs::create_dir_all(folder)
        .map_err(|error| AppError::storage("backup_folder_unavailable", error.to_string()))?;
    let path = Path::new(folder).join(format!("crm-{}.db", clock.file_stamp()));

    store.backup_to(&path).await?;
    Ok(path)
}

/// Copies to `folder`, or to the saved folder, and records when it happened.
pub async fn run_backup(
    store: &SqliteLeads,
    folder: Option<&str>,
    context: &Context,
) -> Result<PathBuf, AppError> {
    let mut settings = store.load().await?;
    let folder = folder
        .map(str::to_owned)
        .or_else(|| settings.backup_directory().map(str::to_owned))
        .ok_or_else(|| {
            AppError::rejected(
                "backup_folder_missing",
                "Elige primero una carpeta para las copias",
            )
        })?;
    let path = create_backup(store, &folder, &context.clock).await?;

    settings.record_backup(context.clock.now_unix());
    store.persist(&settings).await?;
    Ok(path)
}

/// Replaces `database` with a validated backup, saving a safety copy beside it,
/// and returns the reopened store.
pub async fn restore_backup(
    store: SqliteLeads,
    database: &Path,
    backup: &Path,
    clock: &Clock,
) -> Result<SqliteLeads, AppError> {
    let safety = database.with_extension(format!("antes-de-restaurar-{}.db", clock.file_stamp()));

    Ok(store.restore(database, backup, &safety).await?)
}
