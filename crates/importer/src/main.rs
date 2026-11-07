use std::{env, path::PathBuf, process::ExitCode};

use cominorsa_importer::{ImportError, enrich, run};

const EXIT_STORAGE: u8 = 3;
const USAGE: &str = "Uso:\n  cominorsa-import --db <crm.db> <empresas.csv> <personas.csv>\n  cominorsa-import --db <crm.db> --sunat <empresas-sunat.csv>";

fn fail(error: &ImportError) -> ExitCode {
    eprintln!("{error}");

    if error.is_database_refusal() {
        ExitCode::from(EXIT_STORAGE)
    } else {
        ExitCode::FAILURE
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [flag, database, companies, people] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if flag != "--db" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    if companies == "--sunat" {
        return match enrich(&PathBuf::from(database), &PathBuf::from(people)).await {
            Ok(report) => {
                println!("Empresas actualizadas: {}", report.updated);
                println!("Copia previa: {}", report.backup.display());
                ExitCode::SUCCESS
            }
            Err(error) => fail(&error),
        };
    }

    let result = run(
        &PathBuf::from(database),
        &PathBuf::from(companies),
        &PathBuf::from(people),
    )
    .await;

    match result {
        Ok(report) => {
            println!("Prospectos nuevos: {}", report.added);
            println!("Prospectos totales: {}", report.total);
            println!("Copia previa: {}", report.backup.display());
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}
