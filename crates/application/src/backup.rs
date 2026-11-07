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

const BACKUP_PREFIX: &str = "crm-";
const BACKUP_SUFFIX: &str = ".db";

/// The name of a copy made at the clock's instant, such as `crm-20260302-120000.db`.
fn backup_file_name(clock: &Clock) -> String {
    format!("{BACKUP_PREFIX}{}{BACKUP_SUFFIX}", clock.file_stamp())
}

/// Whether `name` follows this app's backup naming exactly: `crm-`, eight
/// digits, `-`, six digits, `.db`. Pruning deletes nothing else.
pub fn is_backup_file_name(name: &str) -> bool {
    let Some(stamp) = name
        .strip_prefix(BACKUP_PREFIX)
        .and_then(|rest| rest.strip_suffix(BACKUP_SUFFIX))
    else {
        return false;
    };

    stamp.len() == 15
        && stamp.bytes().enumerate().all(|(index, byte)| match index {
            8 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
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

/// The names of this app's copies in `folder`, newest first. A missing or
/// unreadable folder is an error; it is never created here.
fn existing_copies(folder: &str) -> Result<Vec<String>, AppError> {
    let entries = fs::read_dir(folder).map_err(|error| folder_unavailable(folder, error))?;
    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| folder_unavailable(folder, error))?;
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_file && is_backup_file_name(&name) {
            names.push(name);
        }
    }
    // The stamp sorts by time, so the name order is the age order.
    names.sort_unstable_by(|left, right| right.cmp(left));

    Ok(names)
}

/// The copies beyond the newest `keep`, oldest last.
fn beyond(names: Vec<String>, keep: u32) -> Vec<String> {
    names
        .into_iter()
        .skip(usize::try_from(keep).unwrap_or(usize::MAX))
        .collect()
}

/// Copies the database to the saved folder when the last backup is older than
/// the interval, then deletes this app's oldest copies beyond the kept count.
/// A dry run reports the same without writing anything. A missing or
/// unwritable folder is an error and deletes nothing.
pub async fn auto_backup(
    store: &SqliteLeads,
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
    if !settings.backup_due(context.clock.now_unix()) {
        return Ok(AutoBackup {
            next_due_at: settings.next_backup_due_at(),
            ..AutoBackup::skipped(AutoBackupStatus::NotDue, dry_run, Some(folder))
        });
    }

    let mut names = existing_copies(&folder)?;
    let keep = settings.backup_keep();
    let in_folder = |name: &String| Path::new(&folder).join(name);

    if dry_run {
        let new_name = backup_file_name(&context.clock);
        names.retain(|name| *name != new_name);
        names.insert(0, new_name.clone());
        return Ok(AutoBackup {
            path: Some(in_folder(&new_name)),
            pruned: beyond(names, keep).iter().map(in_folder).collect(),
            ..AutoBackup::skipped(AutoBackupStatus::WouldCreate, true, Some(folder.clone()))
        });
    }

    let path = run_backup(store, Some(&folder), context).await?;
    let mut pruned = Vec::new();
    for name in beyond(existing_copies(&folder)?, keep) {
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
        next_due_at: store.load().await?.next_backup_due_at(),
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
