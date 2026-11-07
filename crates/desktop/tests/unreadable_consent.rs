//! The app's lead writes go through the same use cases as the CLI, so a lead
//! whose stored consent cannot be read keeps it byte-for-byte.

use std::{env, fs, path::PathBuf, process::Command};

use cominorsa_application::{SettingsChange, assign_lead, save_advance, update_settings};
use cominorsa_desktop::open_database;
use cominorsa_domain::LeadRepository;
use time::macros::date;
use uuid::Uuid;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sqlite(database: &std::path::Path, sql: &str) -> String {
    let output = Command::new("sqlite3")
        .arg(database)
        .arg(sql)
        .output()
        .expect("the gate requires sqlite3");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

const LEAD: &str = "55555555-5555-7555-8555-555555555555";
const STORED: &str = "SELECT quote(consent_given) || '|' || quote(consent_on) || '|' ||
    quote(consent_source) FROM leads";

#[tokio::test]
async fn the_app_keeps_an_unreadable_consent_when_it_assigns_and_advances() {
    let scratch = Scratch(env::temp_dir().join(format!("cominorsa-consent-{}", Uuid::now_v7())));
    fs::create_dir_all(&scratch.0).unwrap();
    let db = scratch.0.join("crm.db");
    let store = open_database(&db).await.unwrap();
    let change = SettingsChange {
        add_members: vec!["Ana".to_owned()],
        ..SettingsChange::default()
    };
    update_settings(&store, change).await.unwrap();
    sqlite(
        &db,
        &format!(
            "INSERT INTO leads (id, name, phone, service, stage, follow_up_on, consent_given,
                consent_on, consent_source)
            VALUES ('{LEAD}', 'Lead del escritorio', '987650010', 'DAC', 'initial_contact',
                '2026-03-05', 1, '2026-02-30', 'Firmó formulario F-12');"
        ),
    );
    let stored = sqlite(&db, STORED);

    assign_lead(&store, Uuid::parse_str(LEAD).unwrap(), "Ana")
        .await
        .unwrap();
    assert_eq!(sqlite(&db, STORED), stored);
    for _ in 0..3 {
        let lead = store.list().await.unwrap().remove(0);
        save_advance(&store, lead, date!(2026 - 03 - 02))
            .await
            .unwrap();
        assert_eq!(sqlite(&db, STORED), stored);
    }
    assert_eq!(store.unreadable_consents().await.unwrap(), 1);
    store.close().await;
}
