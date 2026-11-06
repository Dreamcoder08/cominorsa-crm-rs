use std::{env, path::PathBuf, process::ExitCode};

use cominorsa_importer::run;

const USAGE: &str = "Uso: cominorsa-import --db <crm.db> <empresas.csv> <personas.csv>";

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
            match report.backup {
                Some(path) => println!("Copia previa: {}", path.display()),
                None => println!("Copia previa: no hacía falta, la base era nueva"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
