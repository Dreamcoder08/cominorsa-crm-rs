use std::{env, fs, path::PathBuf};

use cominorsa_domain::{Lead, LeadRepository, NewLead, Stage};
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
    store.save(&lead("Minera", date!(2026 - 11 - 01))).await.unwrap();

    let options = SqliteConnectOptions::new().filename(database.path());
    let raw = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("UPDATE leads SET stage = 'unknown'").execute(&raw).await.unwrap();
    raw.close().await;

    assert!(store.list().await.is_err());
}
