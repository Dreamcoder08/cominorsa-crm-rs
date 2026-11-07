//! `cominorsa`: every business flow without a window, for people and agents.

mod args;
mod commands;
mod views;

use std::{
    env,
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use clap::{
    Arg, ArgAction, CommandFactory, FromArgMatches,
    error::{ContextKind, ErrorKind as ClapKind},
};
use cominorsa_application::ErrorKind;
use serde_json::json;

use crate::{
    args::Cli,
    commands::{Failure, Reply},
};

const EXIT_REJECTED: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_STORAGE: u8 = 3;
const HELP_TEMPLATE: &str = "{about-with-newline}\nUso: {usage}\n\n{all-args}{after-help}";

/// Spanish headings and help flags at every level of the command tree.
fn localized(command: clap::Command) -> clap::Command {
    command
        .help_template(HELP_TEMPLATE)
        .subcommand_help_heading("Comandos")
        .subcommand_value_name("COMANDO")
        .disable_help_subcommand(true)
        .disable_help_flag(true)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("Muestra esta ayuda"),
        )
        .mut_args(|argument| argument.help_heading("Opciones"))
        .mut_subcommands(localized)
}

fn root() -> clap::Command {
    localized(
        Cli::command().disable_version_flag(true).arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help("Muestra la versión"),
        ),
    )
}

fn usage_message(error: &clap::Error) -> String {
    let named = |kind| {
        error
            .get(kind)
            .map_or_else(String::new, ToString::to_string)
    };
    let problem = match error.kind() {
        ClapKind::InvalidSubcommand => {
            format!(
                "Orden desconocida: {}",
                named(ContextKind::InvalidSubcommand)
            )
        }
        ClapKind::UnknownArgument => {
            format!("Opción desconocida: {}", named(ContextKind::InvalidArg))
        }
        ClapKind::MissingRequiredArgument => {
            format!(
                "Falta un dato obligatorio: {}",
                named(ContextKind::InvalidArg)
            )
        }
        ClapKind::InvalidValue | ClapKind::ValueValidation => format!(
            "Valor inválido para {}: {}",
            named(ContextKind::InvalidArg),
            named(ContextKind::InvalidValue)
        ),
        ClapKind::MissingSubcommand | ClapKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            "Falta una orden".to_owned()
        }
        _ => "Uso incorrecto".to_owned(),
    };

    format!("{problem}. Consulta cominorsa --help.")
}

fn print(stream: &mut dyn Write, text: &str) {
    // A closed pipe must not panic: the exit code still tells the outcome.
    let _ = writeln!(stream, "{text}");
}

fn succeed(reply: Reply, as_json: bool) -> ExitCode {
    let text = if as_json {
        json!({"data": reply.data}).to_string()
    } else {
        reply.text
    };
    print(&mut io::stdout(), &text);

    ExitCode::SUCCESS
}

fn fail(code: &str, message: &str, exit: u8, as_json: bool) -> ExitCode {
    let text = if as_json {
        json!({"error": {"code": code, "message": message}}).to_string()
    } else {
        message.to_owned()
    };
    print(&mut io::stderr(), &text);

    ExitCode::from(exit)
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = env::args_os().collect();
    let as_json = arguments
        .iter()
        .take_while(|argument| *argument != "--")
        .any(|argument| argument == "--json");

    let matches = match root().try_get_matches_from(&arguments) {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                ClapKind::DisplayHelp | ClapKind::DisplayVersion
            ) =>
        {
            print(&mut io::stdout(), error.to_string().trim_end());
            return ExitCode::SUCCESS;
        }
        Err(error) => return fail("usage", &usage_message(&error), EXIT_USAGE, as_json),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => return fail("usage", &usage_message(&error), EXIT_USAGE, as_json),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(error) => return fail("runtime_error", &error.to_string(), EXIT_STORAGE, as_json),
    };

    match runtime.block_on(commands::run(cli)) {
        Ok(reply) => succeed(reply, as_json),
        Err(Failure::Usage { code, message }) => fail(code, &message, EXIT_USAGE, as_json),
        Err(Failure::App(error)) => {
            let exit = match error.kind() {
                ErrorKind::Rejected => EXIT_REJECTED,
                ErrorKind::Storage => EXIT_STORAGE,
            };
            fail(error.code(), &error.to_string(), exit, as_json)
        }
    }
}
