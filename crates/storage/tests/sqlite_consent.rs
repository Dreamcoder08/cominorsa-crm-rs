use std::{borrow::Cow, env, fs, path::PathBuf};

use cominorsa_domain::{Consent, Lead, LeadOrigin, LeadRepository, NewLead, SettingsRepository};
use cominorsa_storage::SqliteLeads;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
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
    for lead in &leads {
        assert_eq!(lead.origin(), LeadOrigin::Other);
        assert_eq!(lead.consent(), None);
        assert_eq!(lead.whatsapp_url("Hola"), None);
    }
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

fn received(origin: LeadOrigin) -> Lead {
    Lead::receive(
        NewLead {
            name: "Minera Ejemplo".to_owned(),
            phone: "987654321".to_owned(),
            ruc: None,
            service: "DAC".to_owned(),
            owner: None,
            follow_up_on: date!(2026 - 11 - 10),
        },
        origin,
        date!(2026 - 11 - 07),
    )
    .unwrap()
}

#[tokio::test]
async fn origin_and_consent_survive_a_reopen_and_a_revocation() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let inbound = received(LeadOrigin::InboundCall);
    let mut revoked = received(LeadOrigin::Referral);
    revoked
        .record_consent(date!(2026 - 11 - 08), "Aceptó por correo")
        .unwrap();
    revoked
        .revoke_consent(date!(2026 - 11 - 09), "Pidió no recibir mensajes")
        .unwrap();
    store.save(&inbound).await.unwrap();
    store.save(&revoked).await.unwrap();
    store.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let leads = reopened.list().await.unwrap();
    reopened.close().await;

    let found = |id| leads.iter().find(|lead| lead.id() == id).unwrap();
    assert_eq!(found(inbound.id()), &inbound);
    assert_eq!(found(inbound.id()).origin(), LeadOrigin::InboundCall);
    assert_eq!(found(revoked.id()), &revoked);
    assert_eq!(
        found(revoked.id()).consent(),
        Some(&Consent {
            given: false,
            recorded_on: date!(2026 - 11 - 09),
            source: "Pidió no recibir mensajes".to_owned(),
        })
    );
}

#[tokio::test]
async fn an_unreadable_consent_record_loads_as_no_consent_and_is_counted() {
    let database = TempDatabase::new();
    let store = SqliteLeads::open(&database.path()).await.unwrap();
    let mut healthy = received(LeadOrigin::Referral);
    healthy
        .record_consent(date!(2026 - 11 - 08), "Aceptó por correo")
        .unwrap();
    store.save(&healthy).await.unwrap();
    let corruptions = [
        "UPDATE leads SET consent_on = NULL WHERE id = ?1",
        "UPDATE leads SET consent_on = 'no es fecha' WHERE id = ?1",
        "UPDATE leads SET consent_source = '  ' WHERE id = ?1",
        "UPDATE leads SET consent_given = 'quizá' WHERE id = ?1",
    ];
    let mut broken = Vec::new();
    for _ in corruptions {
        let lead = received(LeadOrigin::WebForm);
        store.save(&lead).await.unwrap();
        broken.push(lead);
    }
    assert_eq!(store.unreadable_consents().await.unwrap(), 0);
    store.close().await;
    let raw = raw_pool(&database.path()).await;
    for (lead, corruption) in broken.iter().zip(corruptions) {
        sqlx::query(corruption)
            .bind(lead.id().to_string())
            .execute(&raw)
            .await
            .unwrap();
    }
    raw.close().await;

    let reopened = SqliteLeads::open(&database.path()).await.unwrap();
    let leads = reopened.list().await.unwrap();
    let unreadable = reopened.unreadable_consents().await.unwrap();
    reopened.close().await;

    assert_eq!(leads.len(), 1 + corruptions.len());
    let found = |id| leads.iter().find(|lead| lead.id() == id).unwrap();
    assert_eq!(found(healthy.id()), &healthy);
    for lead in &broken {
        assert_eq!(found(lead.id()).consent(), None);
        assert_eq!(found(lead.id()).name(), lead.name());
    }
    assert_eq!(unreadable, corruptions.len());
}
