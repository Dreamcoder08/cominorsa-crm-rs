//! `cominorsa-mcp`: an MCP server over stdio for the COMINORSA CRM. It wraps
//! the application layer the `cominorsa` CLI proves. stdout carries only
//! protocol messages; anything for people goes to stderr.

mod server;
mod views;

use std::{env, path::PathBuf, process::ExitCode};

use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};

use crate::server::CrmServer;

const DATABASE_PATH_VARIABLE: &str = "COMINORSA_DB_PATH";

/// Servidor MCP del CRM COMINORSA por stdio. Sin --allow-writes solo ofrece
/// herramientas de lectura. Nunca crea ni actualiza la base, no envía mensajes
/// y no tiene base por defecto.
#[derive(Debug, Parser)]
#[command(name = "cominorsa-mcp", version)]
struct Arguments {
    /// Base de datos autorizada, o COMINORSA_DB_PATH. Sin ella toda llamada responde database_missing
    #[arg(long, value_name = "RUTA")]
    db: Option<PathBuf>,

    /// Agrega las herramientas que escriben, ninguna destructiva. Solo con esta opción
    #[arg(long)]
    allow_writes: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    // Usage errors and help go through clap: errors to stderr with exit 2,
    // before any protocol message exists.
    let arguments = Arguments::parse();
    let db = arguments
        .db
        .or_else(|| env::var_os(DATABASE_PATH_VARIABLE).map(PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty());

    eprintln!(
        "cominorsa-mcp: {}, {}",
        if arguments.allow_writes {
            "lectura y escritura"
        } else {
            "solo lectura"
        },
        if db.is_some() {
            "base indicada"
        } else {
            "sin base: toda llamada responde database_missing"
        }
    );

    let service = match CrmServer::new(db, arguments.allow_writes)
        .serve(stdio())
        .await
    {
        Ok(service) => service,
        Err(error) => {
            eprintln!("cominorsa-mcp: no se pudo iniciar la sesión: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = service.waiting().await {
        eprintln!("cominorsa-mcp: la sesión terminó con un error: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
