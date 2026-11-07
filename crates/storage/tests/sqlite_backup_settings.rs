use std::{env, fs, path::PathBuf};

use cominorsa_domain::{Settings, SettingsRepository};
use cominorsa_storage::SqliteLeads;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use uuid::Uuid;

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn new() -> Self {
        let dir = env::temp_dir().join(format!("cominorsa-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> PathBuf {
        self.0.join("crm.db")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn persists_the_backup_interval_and_kept_copies_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut settings = Settings::default();
    settings.set_backup_interval_days(7).unwrap();
    settings.set_backup_keep(30).unwrap();
    store.persist(&settings).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let loaded = reopened.load().await.unwrap();
    reopened.close().await;

    assert_eq!(loaded.backup_interval_days(), 7);
    assert_eq!(loaded.backup_keep(), 30);
}

#[tokio::test]
async fn an_unreadable_stored_value_keeps_the_default_and_never_blocks() {
    let database = TempDatabase::new();
    SqliteLeads::open(&database.path())
        .await
        .unwrap()
        .close()
        .await;
    let raw = SqlitePool::connect_with(SqliteConnectOptions::new().filename(database.path()))
        .await
        .unwrap();
    sqlx::query("INSERT INTO settings VALUES ('backup_interval_days', '99'), ('backup_keep', 'x')")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let loaded = reopened.load().await.unwrap();
    reopened.close().await;

    assert_eq!(loaded.backup_interval_days(), 1);
    assert_eq!(loaded.backup_keep(), 14);
}
