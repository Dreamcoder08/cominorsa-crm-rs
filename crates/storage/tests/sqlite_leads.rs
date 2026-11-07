use std::{borrow::Cow, env, fs, path::PathBuf};

use cominorsa_domain::{
    Lead, LeadRepository, NewLead, Prospect, ProspectKind, ProspectRepository, Settings,
    SettingsRepository, Stage, ThemePreference,
};
use cominorsa_storage::SqliteLeads;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use time::macros::date;
use uuid::Uuid;

struct TempDatabase(PathBuf);

impl TempDatabase {
    fn new() -> Self {
        let dir = env::temp_dir().join(format!("cominorsa-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> PathBuf {
        self.0.join("nested").join("crm.db")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn lead(name: &str, follow_up_on: time::Date) -> Lead {
    Lead::create(NewLead {
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: "IGAFOM".to_owned(),
        owner: None,
        follow_up_on,
    })
    .unwrap()
}

#[tokio::test]
async fn persists_leads_across_reopen_in_follow_up_order() {
    let database = TempDatabase::new();
    let later = lead("Later", date!(2026 - 12 - 01));
    let sooner = lead("Sooner", date!(2026 - 11 - 01));

    let store = SqliteLeads::open(&database.path()).await.unwrap();
    store.save(&later).await.unwrap();
    store.save(&sooner).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(reopened.list().await.unwrap(), vec![sooner, later]);
}

#[tokio::test]
async fn saving_an_existing_lead_updates_it() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut saved = lead("Minera", date!(2026 - 11 - 01));
    store.save(&saved).await.unwrap();

    saved.advance().unwrap();
    store.save(&saved).await.unwrap();

    let leads = store.list().await.unwrap();
    assert_eq!(leads.len(), 1);
    assert_eq!(leads[0].stage(), Stage::CoordinateEvaluation);
}

#[tokio::test]
async fn refuses_to_return_a_corrupt_record() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    store
        .save(&lead("Minera", date!(2026 - 11 - 01)))
        .await
        .unwrap();

    let options = SqliteConnectOptions::new().filename(database.path());
    let raw = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("UPDATE leads SET stage = 'unknown'")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;

    assert!(store.list().await.is_err());
}

fn prospect(name: &str, kind: ProspectKind, concessions: u32) -> Prospect {
    Prospect {
        name: name.to_owned(),
        kind,
        profile: "PERFIL_1_formalizacion".to_owned(),
        concessions,
        hectares: 100.5,
        departments: "PIURA".to_owned(),
        provinces: "SULLANA".to_owned(),
        substances: "M".to_owned(),
        concession_states: "TITULADA".to_owned(),
        potential_service: "IGAFOM + REINFO".to_owned(),
        needs_review: false,
        source: "Synthetic".to_owned(),
    }
}

#[tokio::test]
async fn imports_prospects_once_and_keeps_them_across_reopen() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let batch = vec![
        prospect("Empresa Uno", ProspectKind::Company, 2),
        prospect("Persona Dos", ProspectKind::Person, 5),
    ];

    assert_eq!(store.import(&batch).await.unwrap(), 2);
    assert_eq!(store.import(&batch).await.unwrap(), 0);
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(reopened.count().await.unwrap(), 2);
    assert_eq!(
        reopened.search("", 10).await.unwrap(),
        vec![batch[1].clone(), batch[0].clone()]
    );
}

#[tokio::test]
async fn searches_prospects_with_a_limit_and_literal_wildcards() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    store
        .import(&[
            prospect("Minera Alfa", ProspectKind::Company, 1),
            prospect("Minera Beta", ProspectKind::Company, 2),
            prospect("Otra 100% Gamma", ProspectKind::Person, 3),
        ])
        .await
        .unwrap();

    assert_eq!(store.search("minera", 10).await.unwrap().len(), 2);
    assert_eq!(store.search("sullana", 2).await.unwrap().len(), 2);
    assert_eq!(store.search("100%", 10).await.unwrap().len(), 1);
    assert!(store.search("%", 10).await.unwrap().len() == 1);
    assert!(store.search("inexistente", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn keeps_existing_leads_when_upgrading_from_the_first_schema() {
    let database = TempDatabase::new();
    fs::create_dir_all(database.path().parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(database.path())
        .create_if_missing(true);
    let old = SqlitePool::connect_with(options).await.unwrap();
    let mut first_schema = sqlx::migrate!("./migrations");
    first_schema.migrations = Cow::Owned(
        first_schema
            .migrations
            .iter()
            .filter(|migration| migration.version == 1)
            .cloned()
            .collect(),
    );
    first_schema.run(&old).await.unwrap();
    sqlx::query(
        "INSERT INTO leads VALUES ('00000000-0000-7000-8000-000000000001', 'Antiguo',
            '900000000', NULL, 'DAC', 'quote_sent', '2026-11-01')",
    )
    .execute(&old)
    .await
    .unwrap();
    old.close().await;

    let upgraded = SqliteLeads::open(&database.path()).await.unwrap();

    let leads = upgraded.list().await.unwrap();
    assert_eq!(leads.len(), 1);
    assert_eq!(leads[0].name(), "Antiguo");
    assert_eq!(leads[0].stage(), Stage::QuoteSent);
    assert_eq!(upgraded.count().await.unwrap(), 0);
}

#[tokio::test]
async fn restores_data_from_a_backup() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let saved = lead("Minera", date!(2026 - 11 - 01));
    store.save(&saved).await.unwrap();
    let copy = database.0.join("backup.db");

    store.backup_to(&copy).await.unwrap();

    let restored = SqliteLeads::open(&copy).await.unwrap();
    assert_eq!(restored.list().await.unwrap(), vec![saved]);
    assert!(store.backup_to(&copy).await.is_err());
}

#[tokio::test]
async fn finds_uppercase_spanish_names_from_lowercase_queries() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    store
        .import(&[prospect("PEÑA ÁLVAREZ JUAN", ProspectKind::Person, 1)])
        .await
        .unwrap();

    assert_eq!(store.search("peña", 10).await.unwrap().len(), 1);
    assert_eq!(store.search("álvarez", 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn returns_default_settings_on_a_new_database() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();

    assert_eq!(store.load().await.unwrap(), Settings::default());
}

#[tokio::test]
async fn persists_settings_across_reopen_and_replaces_the_team() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut settings = Settings::default();
    settings.theme = ThemePreference::Dark;
    settings.set_follow_up_days(7).unwrap();
    settings.add_member("Ana").unwrap();
    settings.add_member("Luis").unwrap();
    settings.set_whatsapp_template("Hola {nombre}").unwrap();
    settings.set_backup_directory("/media/usb");
    settings.record_backup(1_800_000_000);
    store.persist(&settings).await.unwrap();

    settings.remove_member("Ana");
    store.persist(&settings).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(reopened.load().await.unwrap(), settings);
}

#[tokio::test]
async fn keeps_leads_and_prospects_when_upgrading_from_the_second_schema() {
    let database = TempDatabase::new();
    fs::create_dir_all(database.path().parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(database.path())
        .create_if_missing(true);
    let old = SqlitePool::connect_with(options).await.unwrap();
    let mut second_schema = sqlx::migrate!("./migrations");
    second_schema.migrations = Cow::Owned(
        second_schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= 2)
            .cloned()
            .collect(),
    );
    second_schema.run(&old).await.unwrap();
    sqlx::query(
        "INSERT INTO leads VALUES ('00000000-0000-7000-8000-000000000001', 'Antiguo',
            '900000000', NULL, 'DAC', 'quote_sent', '2026-11-01')",
    )
    .execute(&old)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO prospects VALUES ('person', 'Titular', 'OTRO_estado', 1, 2.5, 'PIURA',
            'SULLANA', 'M', 'TITULADA', 'REVISAR_MANUAL', 1, 'Synthetic')",
    )
    .execute(&old)
    .await
    .unwrap();
    old.close().await;

    let upgraded = SqliteLeads::open(&database.path()).await.unwrap();

    assert_eq!(upgraded.list().await.unwrap().len(), 1);
    assert_eq!(upgraded.count().await.unwrap(), 1);
    assert_eq!(upgraded.load().await.unwrap(), Settings::default());
}

#[tokio::test]
async fn saves_the_responsible_person() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut saved = lead("Minera", date!(2026 - 11 - 01));
    saved.assign(Some("Ana"));

    store.save(&saved).await.unwrap();
    assert_eq!(store.list().await.unwrap()[0].owner(), Some("Ana"));

    saved.assign(None);
    store.save(&saved).await.unwrap();
    assert_eq!(store.list().await.unwrap()[0].owner(), None);
}

#[tokio::test]
async fn keeps_data_and_settings_when_upgrading_from_the_third_schema() {
    let database = TempDatabase::new();
    fs::create_dir_all(database.path().parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(database.path())
        .create_if_missing(true);
    let old = SqlitePool::connect_with(options).await.unwrap();
    let mut third_schema = sqlx::migrate!("./migrations");
    third_schema.migrations = Cow::Owned(
        third_schema
            .migrations
            .iter()
            .filter(|migration| migration.version <= 3)
            .cloned()
            .collect(),
    );
    third_schema.run(&old).await.unwrap();
    sqlx::query(
        "INSERT INTO leads VALUES ('00000000-0000-7000-8000-000000000001', 'Antiguo',
            '900000000', NULL, 'DAC', 'quote_sent', '2026-11-01')",
    )
    .execute(&old)
    .await
    .unwrap();
    sqlx::query("INSERT INTO settings VALUES ('theme', 'dark')")
        .execute(&old)
        .await
        .unwrap();
    old.close().await;

    let upgraded = SqliteLeads::open(&database.path()).await.unwrap();

    let leads = upgraded.list().await.unwrap();
    assert_eq!(leads[0].name(), "Antiguo");
    assert_eq!(leads[0].owner(), None);
    assert_eq!(upgraded.load().await.unwrap().theme, ThemePreference::Dark);
}

#[tokio::test]
async fn restores_a_backup_and_keeps_a_safety_copy() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let original = lead("Original", date!(2026 - 11 - 01));
    store.save(&original).await.unwrap();
    let backup = database.0.join("backup.db");
    store.backup_to(&backup).await.unwrap();
    let later = lead("Posterior", date!(2026 - 12 - 01));
    store.save(&later).await.unwrap();
    let safety = database.0.join("safety.db");

    let restored = store
        .restore(&database.path(), &backup, &safety)
        .await
        .unwrap();

    assert_eq!(restored.list().await.unwrap(), vec![original.clone()]);
    let copy = SqliteLeads::open(&safety).await.unwrap();
    assert_eq!(copy.list().await.unwrap(), vec![original, later]);
}

#[tokio::test]
async fn refuses_to_restore_an_invalid_file_and_keeps_current_data() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let saved = lead("Actual", date!(2026 - 11 - 01));
    store.save(&saved).await.unwrap();
    let invalid = database.0.join("invalid.db");
    fs::write(&invalid, "esto no es una base de datos").unwrap();
    let safety = database.0.join("safety.db");

    let failed = store.restore(&database.path(), &invalid, &safety).await;

    assert!(failed.is_err());
    assert!(!safety.exists());
    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(reopened.list().await.unwrap(), vec![saved]);
}

async fn write_unrelated_database(path: &std::path::Path) {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("CREATE TABLE notes (body TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn refuses_to_restore_files_without_crm_data() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let saved = lead("Actual", date!(2026 - 11 - 01));
    store.save(&saved).await.unwrap();
    store.close().await;

    let empty = database.0.join("empty.db");
    fs::write(&empty, "").unwrap();
    let unrelated = database.0.join("unrelated.db");
    write_unrelated_database(&unrelated).await;
    let safety = database.0.join("safety.db");

    for candidate in [&empty, &unrelated] {
        let store = SqliteLeads::open(&database.path()).await.unwrap();

        assert!(
            store
                .clone()
                .restore(&database.path(), candidate, &safety)
                .await
                .is_err(),
            "{candidate:?} must be refused"
        );
        store.close().await;
    }

    assert!(!safety.exists());
    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    assert_eq!(reopened.list().await.unwrap(), vec![saved]);
    assert!(!database.path().with_extension("restoring.db").exists());
}

#[tokio::test]
async fn loads_templates_saved_before_placeholder_validation() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let options = SqliteConnectOptions::new().filename(database.path());
    let raw = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("INSERT INTO settings VALUES ('whatsapp_template', 'Hola {cliente}')")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;

    let loaded = store.load().await.unwrap();

    assert_eq!(loaded.whatsapp_template(), "Hola {cliente}");
}

#[tokio::test]
async fn forgets_backup_settings_that_were_cleared() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut settings = Settings::default();
    settings.set_backup_directory("/media/usb");
    store.persist(&settings).await.unwrap();

    settings.set_backup_directory("   ");
    store.persist(&settings).await.unwrap();

    assert_eq!(store.load().await.unwrap().backup_directory(), None);
}
