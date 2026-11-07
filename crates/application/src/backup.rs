//! Copies of the database. SQLite backup has no port, so this names the adapter.

use std::{
    fs,
    path::{Path, PathBuf},
};

use cominorsa_domain::{Settings, SettingsRepository};
use cominorsa_storage::SqliteLeads;
use time::Date;

use crate::{
    context::{Clock, Context},
    error::AppError,
    quotes::is_database_file,
};

const BACKUP_PREFIX: &str = "crm-";
const BACKUP_SUFFIX: &str = ".db";

/// The name of a copy made at the clock's instant, such as `crm-20260302-120000.db`.
fn backup_file_name(clock: &Clock) -> String {
    format!("{BACKUP_PREFIX}{}{BACKUP_SUFFIX}", clock.file_stamp())
}

/// The stamp of a name that follows this app's backup naming exactly: `crm-`,
/// a real instant as eight digits, `-`, six digits, then `.db`.
fn backup_stamp(name: &str) -> Option<&str> {
    let stamp = name
        .strip_prefix(BACKUP_PREFIX)?
        .strip_suffix(BACKUP_SUFFIX)?;
    let shaped = stamp.len() == 15
        && stamp.bytes().enumerate().all(|(index, byte)| match index {
            8 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });

    (shaped && Clock::is_file_stamp(stamp)).then_some(stamp)
}

/// Whether `name` follows this app's backup naming exactly, such as
/// `crm-20260302-120000.db`. Pruning deletes nothing else.
pub fn is_backup_file_name(name: &str) -> bool {
    backup_stamp(name).is_some()
}

/// Writes a timestamped copy to the chosen folder and returns its path.
pub async fn create_backup(
    store: &SqliteLeads,
    folder: &str,
    clock: &Clock,
) -> Result<PathBuf, AppError> {
    fs::create_dir_all(folder)
        .map_err(|error| AppError::storage("backup_folder_unavailable", error.to_string()))?;
    let path = Path::new(folder).join(backup_file_name(clock));

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

/// What an automatic backup did, or would do in a dry run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoBackupStatus {
    NotConfigured,
    NotDue,
    Created,
    WouldCreate,
}

impl AutoBackupStatus {
    /// Stable value for machines.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::NotDue => "not_due",
            Self::Created => "created",
            Self::WouldCreate => "would_create",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoBackup {
    pub status: AutoBackupStatus,
    pub dry_run: bool,
    pub folder: Option<String>,
    /// The copy made, or the one a dry run would make.
    pub path: Option<PathBuf>,
    /// Old copies deleted, or the ones a dry run would delete, newest first.
    pub pruned: Vec<PathBuf>,
    /// When the last backup stops being recent enough, in Unix seconds.
    pub next_due_at: Option<i64>,
}

impl AutoBackup {
    fn skipped(status: AutoBackupStatus, dry_run: bool, folder: Option<String>) -> Self {
        Self {
            status,
            dry_run,
            folder,
            path: None,
            pruned: Vec::new(),
            next_due_at: None,
        }
    }
}

fn folder_unavailable(folder: &str, cause: impl std::fmt::Display) -> AppError {
    AppError::storage(
        "backup_folder_unavailable",
        format!("No se puede usar la carpeta de copias {folder}: {cause}"),
    )
}

/// The saved folder must be absolute: a relative one would depend on where
/// the app was started.
fn absolute_folder(folder: &str) -> Result<(), AppError> {
    if Path::new(folder).is_absolute() {
        return Ok(());
    }

    Err(AppError::rejected(
        "backup_folder_not_absolute",
        format!("La carpeta de copias {folder} debe ser una ruta completa, como /media/usb/copias"),
    ))
}

/// The folder must exist, be a folder, and accept new files. It is never
/// created here, and a dry run checks exactly what a real run checks.
fn usable_folder(folder: &str) -> Result<(), AppError> {
    let metadata = fs::metadata(folder).map_err(|error| folder_unavailable(folder, error))?;
    if !metadata.is_dir() {
        return Err(folder_unavailable(folder, "no es una carpeta"));
    }
    if metadata.permissions().readonly() {
        return Err(folder_unavailable(folder, "no se puede escribir en ella"));
    }

    Ok(())
}

/// The copies pruning may delete, newest first: files in `folder` named
/// exactly like this app's copies, except the copy this run makes, copies
/// stamped after it (a clock was wrong; they are neither counted nor
/// deleted), and the live database or its sidecars by path or identity.
fn prunable_copies(folder: &str, database: &Path, new_name: &str) -> Result<Vec<String>, AppError> {
    let new_stamp = backup_stamp(new_name).unwrap_or_default();
    let entries = fs::read_dir(folder).map_err(|error| folder_unavailable(folder, error))?;
    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| folder_unavailable(folder, error))?;
        // A link is not a file here, so a link to elsewhere is never deleted.
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stamp) = backup_stamp(&name) else {
            continue;
        };
        if is_file
            && name != new_name
            && stamp < new_stamp
            && !is_database_file(&entry.path(), database)
        {
            names.push(name);
        }
    }
    // The stamp sorts by time, so the name order is the age order.
    names.sort_unstable_by(|left, right| right.cmp(left));

    Ok(names)
}

/// The first second, in Peru, of the day a copy becomes due after one made on
/// `last_backup_on`.
fn next_due_at(settings: &Settings, last_backup_on: Option<Date>) -> Option<i64> {
    settings
        .next_backup_on(last_backup_on)
        .map(Clock::start_of_day_in_peru)
}

/// The copies beyond the newest `keep`, counting the copy this run makes.
fn beyond(names: Vec<String>, keep: u32) -> Vec<String> {
    let others = usize::try_from(keep)
        .unwrap_or(usize::MAX)
        .saturating_sub(1);

    names.into_iter().skip(others).collect()
}

/// Copies the database to the saved folder when the last backup is from a
/// calendar day at least the interval before today, in Peru, then deletes this
/// app's oldest copies beyond the kept count. The copy just made, the live
/// `database` and its sidecars are never deleted. A dry run checks and reports
/// the same without writing anything. A relative, missing, or unwritable
/// folder is an error and deletes nothing.
pub async fn auto_backup(
    store: &SqliteLeads,
    database: &Path,
    context: &Context,
    dry_run: bool,
) -> Result<AutoBackup, AppError> {
    let settings = store.load().await?;
    let Some(folder) = settings.backup_directory().map(str::to_owned) else {
        return Ok(AutoBackup::skipped(
            AutoBackupStatus::NotConfigured,
            dry_run,
            None,
        ));
    };
    absolute_folder(&folder)?;
    let last_backup_on = settings.last_backup_at().and_then(Clock::day_in_peru);
    if !settings.backup_due(context.clock.today(), last_backup_on) {
        return Ok(AutoBackup {
            next_due_at: next_due_at(&settings, last_backup_on),
            ..AutoBackup::skipped(AutoBackupStatus::NotDue, dry_run, Some(folder))
        });
    }

    usable_folder(&folder)?;
    let new_name = backup_file_name(&context.clock);
    let in_folder = |name: &String| Path::new(&folder).join(name);
    let keep = settings.backup_keep();

    if dry_run {
        let pruned = beyond(prunable_copies(&folder, database, &new_name)?, keep);
        return Ok(AutoBackup {
            path: Some(in_folder(&new_name)),
            pruned: pruned.iter().map(in_folder).collect(),
            next_due_at: next_due_at(&settings, Clock::day_in_peru(context.clock.now_unix())),
            ..AutoBackup::skipped(AutoBackupStatus::WouldCreate, true, Some(folder.clone()))
        });
    }

    // Only the copy itself reports the folder; recording it reports the store.
    let path = create_backup(store, &folder, &context.clock)
        .await
        .map_err(|error| folder_unavailable(&folder, error))?;
    let mut settings = settings;
    settings.record_backup(context.clock.now_unix());
    store.persist(&settings).await?;
    let copied_on = Clock::day_in_peru(context.clock.now_unix());
    let mut pruned = Vec::new();
    for name in beyond(prunable_copies(&folder, database, &new_name)?, keep) {
        let old = in_folder(&name);
        fs::remove_file(&old).map_err(|error| {
            AppError::storage(
                "backup_prune_failed",
                format!(
                    "La copia se creó en {}, pero no se pudo borrar {}: {error}",
                    path.display(),
                    old.display()
                ),
            )
        })?;
        pruned.push(old);
    }

    Ok(AutoBackup {
        path: Some(path),
        pruned,
        next_due_at: next_due_at(&settings, copied_on),
        ..AutoBackup::skipped(AutoBackupStatus::Created, false, Some(folder))
    })
}

/// A store back on a restored database, and where the previous data went.
#[derive(Clone)]
pub struct Restored {
    pub store: SqliteLeads,
    pub safety_copy: PathBuf,
}

/// Replaces `database` with a validated backup, saving a safety copy beside it,
/// and returns the reopened store.
pub async fn restore_backup(
    store: SqliteLeads,
    database: &Path,
    backup: &Path,
    clock: &Clock,
) -> Result<Restored, AppError> {
    let safety_copy =
        database.with_extension(format!("antes-de-restaurar-{}.db", clock.file_stamp()));
    let store = store.restore(database, backup, &safety_copy).await?;

    Ok(Restored { store, safety_copy })
}
