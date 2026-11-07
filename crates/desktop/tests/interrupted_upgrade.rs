//! A first launch never creates a database where an interrupted upgrade left
//! the old one aside; the window names the files to recover.

use std::{env, fs, path::PathBuf};

use cominorsa_desktop::open_database;
use uuid::Uuid;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn a_first_launch_refuses_while_the_database_is_aside() {
    let scratch = Scratch(env::temp_dir().join(format!("cominorsa-aside-{}", Uuid::now_v7())));
    fs::create_dir_all(&scratch.0).unwrap();
    let built = scratch.0.join("built.db");
    open_database(&built).await.unwrap().close().await;
    let aside = scratch.0.join("crm.aparte-77-0.db");
    let copy = scratch
        .0
        .join("crm.antes-de-migrar-20270115-030000-77-0.db");
    fs::copy(&built, &copy).unwrap();
    fs::rename(&built, &aside).unwrap();
    let db = scratch.0.join("crm.db");

    let Err(refusal) = open_database(&db).await else {
        panic!("a new database was opened");
    };

    assert!(refusal.contains(&aside.display().to_string()), "{refusal}");
    assert!(refusal.contains(&copy.display().to_string()), "{refusal}");
    assert!(!db.exists());
}
