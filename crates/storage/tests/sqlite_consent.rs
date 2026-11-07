use std::{borrow::Cow, env, fs, path::PathBuf};

use cominorsa_domain::{LeadRepository, SettingsRepository};
use cominorsa_storage::SqliteLeads;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
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

async fn raw_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
    .unwrap()
}

/// A database at schema 8 with synthetic rows, as an older app left it.
async fn eighth_schema_database(path: &std::path::Path) {
    let old = raw_pool(path).await;
    let mut eighth_schema = sqlx::migrate!("./migrations");
    eighth_schema.migrations = Cow::Owned(
        eighth_schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= 8)
            .cloned()
            .collect(),
    );
    eighth_schema.run(&old).await.unwrap();
    for statement in [
        "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on, owner)
            VALUES ('00000000-0000-7000-8000-000000000001', 'Minera Antigua', '987654321',
            NULL, 'DAC', 'contract_signed', '2026-11-01', 'Ana')",
        "INSERT INTO cases (id, lead_id, client_name, ruc, service, owner, opened_on, status,
            contract_total_cents)
            VALUES ('00000000-0000-7000-8000-00000000000a',
            '00000000-0000-7000-8000-000000000001', 'Otra', NULL, 'DAC', 'Ana', '2026-11-01',
            'closed', NULL)",
        "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on, owner,
            renewal_of_case)
            VALUES ('00000000-0000-7000-8000-000000000002', 'Renovación: Otra', '987654322',
            '20123456786', 'DAC', 'initial_contact', '2026-11-02', NULL,
            '00000000-0000-7000-8000-00000000000a')",
        "INSERT INTO settings VALUES ('follow_up_days', '9'), ('last_backup_at', '1700000000')",
        "INSERT INTO team_members VALUES (0, 'Ana')",
    ] {
        sqlx::query(statement).execute(&old).await.unwrap();
    }
    old.close().await;
}

#[tokio::test]
async fn keeps_every_lead_when_upgrading_from_the_eighth_schema() {
    let database = TempDatabase::new();
    eighth_schema_database(&database.path()).await;

    let upgraded = SqliteLeads::open(&database.path()).await.unwrap();

    assert_eq!(upgraded.schema_version().await.unwrap(), 9);
    let leads = upgraded.list().await.unwrap();
    assert_eq!(leads.len(), 2);
    assert_eq!(leads[0].name(), "Minera Antigua");
    assert_eq!(leads[0].owner(), Some("Ana"));
    assert_eq!(leads[1].client_name(), "Otra");
    assert!(leads[1].renewal_of().is_some());
    let settings = upgraded.load().await.unwrap();
    assert_eq!(settings.follow_up_days(), 9);
    assert_eq!(settings.last_backup_at(), Some(1_700_000_000));
    assert_eq!(settings.team(), ["Ana"]);
    upgraded.close().await;

    let raw = raw_pool(&database.path()).await;
    let rows = sqlx::query(
        "SELECT origin, consent_given, consent_on, consent_source FROM leads ORDER BY id",
    )
    .fetch_all(&raw)
    .await
    .unwrap();
    for row in &rows {
        assert_eq!(row.get::<String, _>("origin"), "other");
        assert_eq!(row.get::<Option<i64>, _>("consent_given"), None);
        assert_eq!(row.get::<Option<String>, _>("consent_on"), None);
        assert_eq!(row.get::<Option<String>, _>("consent_source"), None);
    }
    raw.close().await;
}
