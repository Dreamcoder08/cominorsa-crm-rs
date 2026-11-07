//! Black-box contract of `cominorsa-mcp`: a real stdio handshake against the
//! built binary, on synthetic databases only.

mod common;

use std::{env, fs, path::Path, process::Command};

use common::{
    Fixture, READ_TOOLS, Scratch, Session, WRITE_TOOLS, arguments, binary, evidence, synthetic,
};
use serde_json::{Value, json};

const NO_ID: &str = "00000000-0000-0000-0000-000000000000";

fn placeholder_arguments(tool: &str) -> Value {
    match tool {
        "lead_show" | "case_show" | "step_done" | "milestone_pay" => json!({"id": NO_ID}),
        "quote_show" => json!({"quote": "COT-2026-0001"}),
        "lead_add" => json!({"name": "Nuevo", "phone": "987000111", "service": "DAC"}),
        "lead_assign" => json!({"id": NO_ID, "owner": "Ana Torres"}),
        "lead_consent" | "lead_revoke_consent" => json!({"id": NO_ID, "source": "Llamada"}),
        "step_date" => json!({"id": NO_ID, "date": "2026-04-01"}),
        _ => json!({}),
    }
}

fn all_tools() -> Vec<String> {
    let mut tools: Vec<String> = READ_TOOLS
        .iter()
        .chain(WRITE_TOOLS.iter())
        .map(|name| (*name).to_owned())
        .collect();
    tools.sort();
    tools
}

#[test]
fn lists_only_read_only_tools_by_default() {
    let mut session = Session::start(&[]);
    let tools = session.tools();

    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, READ_TOOLS);
    for tool in &tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], json!(true), "{tool}");
    }
}

#[test]
fn allow_writes_adds_only_non_destructive_write_tools() {
    let mut session = Session::start(&["--allow-writes"]);
    let tools = session.tools();

    let names: Vec<String> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, all_tools());
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        if WRITE_TOOLS.contains(&name) {
            assert_eq!(tool["annotations"]["readOnlyHint"], json!(false), "{tool}");
            assert_eq!(
                tool["annotations"]["destructiveHint"],
                json!(false),
                "{tool}"
            );
        }
        for banned in [
            "restore", "delete", "prune", "migrate", "whatsapp", "message", "send",
        ] {
            assert!(!name.contains(banned), "{name}");
        }
    }
}

#[test]
fn no_environment_variable_enables_write_tools() {
    let mut command = binary();
    for variable in [
        "COMINORSA_MCP_ALLOW_WRITES",
        "COMINORSA_ALLOW_WRITES",
        "ALLOW_WRITES",
        "MCP_ALLOW_WRITES",
    ] {
        command.env(variable, "1");
    }
    let mut session = Session::start_with(&mut command);

    assert_eq!(session.tool_names(), READ_TOOLS);
}

#[test]
fn without_a_database_it_lists_tools_and_every_call_is_database_missing() {
    let mut session = Session::start(&["--allow-writes"]);

    assert_eq!(session.tool_names(), all_tools());
    for tool in all_tools() {
        let (code, _) = session.failure(&tool, placeholder_arguments(&tool));
        assert_eq!(code, "database_missing", "{tool}");
    }
}

#[test]
fn the_environment_variable_names_the_database() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let mut command = binary();
    command.env("COMINORSA_DB_PATH", &fixture.db);
    let mut session = Session::start_with(&mut command);

    let data = session.data("status", json!({}));
    assert_eq!(data["counts"]["leads"], json!(2));
}

#[test]
fn read_tools_return_the_stored_state() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let before = evidence(&fixture.db);
    let mut session = Session::with_db(&fixture.db, &[]);

    let status = session.data("status", json!({}));
    assert_eq!(status["integrity_check"], json!("ok"));
    assert_eq!(status["counts"]["leads"], json!(2));
    assert_eq!(status["counts"]["cases"], json!(1));
    assert_eq!(status["counts"]["quotes"], json!(1));

    let leads = session.data("lead_list", json!({}));
    assert_eq!(leads["leads"].as_array().unwrap().len(), 2);
    let found = session.data("lead_list", json!({"query": "Lucía"}));
    assert_eq!(found["leads"][0]["name"], json!("Lucía Quispe"));
    assert!(found["leads"][0]["follow_up"]["status"].is_string());
    let limited = session.data("lead_list", json!({"limit": 1}));
    assert_eq!(limited["leads"].as_array().unwrap().len(), 1);

    let shown = session.data("lead_show", arguments("lead_show", &fixture));
    assert_eq!(shown["lead"]["id"], json!(fixture.open_lead.to_string()));
    assert!(shown["follow_up"]["label"].is_string());

    let cases = session.data("case_list", json!({}));
    assert_eq!(cases["cases"][0]["id"], json!(fixture.case.to_string()));
    let case = session.data("case_show", arguments("case_show", &fixture));
    assert_eq!(case["case"]["contract_total_cents"], json!(100_000));
    assert_eq!(case["billing"]["pending_cents"], json!(100_000));

    let quotes = session.data("quote_list", json!({"lead": fixture.open_lead.to_string()}));
    assert_eq!(quotes["quotes"][0]["number"], json!(fixture.quote_number));
    let quote = session.data("quote_show", arguments("quote_show", &fixture));
    assert_eq!(quote["quote"]["totals"]["subtotal_cents"], json!(50_000));

    assert_eq!(
        session.data("prospect_count", json!({})),
        json!({"total": 0})
    );
    assert_eq!(
        session.data("prospect_search", json!({"query": "x", "limit": 5})),
        json!({"prospects": []})
    );

    let agenda = session.data("agenda", json!({}));
    assert!(agenda["overdue"].is_array() && agenda["upcoming"].is_array());

    let settings = session.data("settings_show", json!({}));
    assert_eq!(settings["team"], json!(["Ana Torres"]));

    drop(session);
    assert_eq!(evidence(&fixture.db), before, "a read changed the database");
}

fn cli_data(db: &Path, args: &[&str]) -> Value {
    let cli = Path::new(env!("CARGO_BIN_EXE_cominorsa-mcp"))
        .with_file_name(format!("cominorsa{}", env::consts::EXE_SUFFIX));
    assert!(
        cli.exists(),
        "build the CLI first (cargo build -p cominorsa-cli); missing {}",
        cli.display()
    );
    let output = Command::new(cli)
        .env_remove("COMINORSA_DB_PATH")
        .arg("--db")
        .arg(db)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    parsed["data"].clone()
}

#[test]
fn read_tools_answer_with_the_cli_json_shapes() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let mut session = Session::with_db(&fixture.db, &[]);
    let open_lead = fixture.open_lead.to_string();
    let case = fixture.case.to_string();

    let pairs: [(&str, Value, Vec<&str>); 10] = [
        ("status", json!({}), vec!["status"]),
        ("agenda", json!({}), vec!["agenda"]),
        ("lead_list", json!({}), vec!["lead", "list"]),
        (
            "lead_show",
            json!({"id": open_lead}),
            vec!["lead", "show", &open_lead],
        ),
        ("prospect_search", json!({}), vec!["prospect", "search"]),
        ("prospect_count", json!({}), vec!["prospect", "count"]),
        ("case_list", json!({}), vec!["case", "list"]),
        (
            "case_show",
            json!({"id": case}),
            vec!["case", "show", &case],
        ),
        ("quote_list", json!({}), vec!["quote", "list"]),
        (
            "quote_show",
            json!({"quote": fixture.quote_number}),
            vec!["quote", "show", &fixture.quote_number],
        ),
    ];
    for (tool, arguments, command) in pairs {
        assert_eq!(
            session.data(tool, arguments),
            cli_data(&fixture.db, &command),
            "{tool}"
        );
    }

    let mut cli_settings = cli_data(&fixture.db, &["settings", "show"]);
    let settings = session.data("settings_show", json!({}));
    let object = cli_settings.as_object_mut().unwrap();
    object.remove("backup_directory");
    object.insert("backup_directory_set".to_owned(), json!(true));
    assert_eq!(settings, cli_settings);
}

#[test]
fn results_and_errors_never_name_file_system_paths() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let mut session = Session::with_db(&fixture.db, &["--allow-writes"]);
    let mut texts = Vec::new();

    for tool in all_tools() {
        let response = session.call_raw(&tool, arguments(&tool, &fixture));
        texts.push(response.to_string());
    }
    let settings = session.data("settings_show", json!({}));
    assert_eq!(settings["backup_directory_set"], json!(true));
    assert!(settings.get("backup_directory").is_none(), "{settings}");
    drop(session);

    let older = scratch.database_at(8, "vieja.db");
    let missing = scratch.file("no-existe.db");
    for db in [&older, &missing] {
        let mut session = Session::with_db(db, &["--allow-writes"]);
        texts.push(session.call_raw("status", json!({})).to_string());
        texts.push(
            session
                .call_raw("lead_add", arguments("lead_add", &fixture))
                .to_string(),
        );
    }

    let folder = scratch.path().display().to_string();
    let folder_name = scratch
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for text in texts {
        assert!(!text.contains(&folder), "{text}");
        assert!(!text.contains(&folder_name), "{text}");
        assert!(!text.contains("copias-secretas"), "{text}");
        assert!(!text.contains("panicked"), "{text}");
        assert!(!text.contains("src/"), "{text}");
    }
}

#[test]
fn errors_are_categorical() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let mut session = Session::with_db(&fixture.db, &[]);

    let (code, message) = session.failure("lead_show", json!({"id": NO_ID}));
    assert_eq!(code, "lead_not_found");
    assert_eq!(message, "No se encontró el lead");

    let (code, _) = session.failure("lead_show", json!({"id": "not-a-uuid"}));
    assert_eq!(code, "usage");
}

#[test]
fn inputs_are_bounded() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let before = evidence(&fixture.db);
    let mut session = Session::with_db(&fixture.db, &["--allow-writes"]);
    let long = "x".repeat(10_000);

    for (tool, arguments) in [
        ("prospect_search", json!({"limit": 0})),
        ("prospect_search", json!({"limit": 1_000_000})),
        ("prospect_search", json!({"query": long})),
        ("lead_list", json!({"limit": 0})),
        ("lead_list", json!({"query": long})),
        ("case_list", json!({"limit": 100_000})),
        ("quote_list", json!({"limit": 0})),
        ("quote_show", json!({"quote": long})),
        (
            "lead_add",
            json!({"name": long, "phone": "987000111", "service": "DAC"}),
        ),
        (
            "lead_consent",
            json!({"id": fixture.open_lead.to_string(), "source": long}),
        ),
    ] {
        let (code, _) = session.failure(tool, arguments.clone());
        assert_eq!(code, "usage", "{tool} {arguments}");
    }
    drop(session);
    assert_eq!(evidence(&fixture.db), before);
}

fn refused(response: &Value) -> bool {
    response.get("error").is_some() || response["result"]["isError"] == json!(true)
}

#[test]
fn write_tools_are_refused_without_the_flag_and_change_nothing() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let before = evidence(&fixture.db);
    let mut session = Session::with_db(&fixture.db, &[]);

    for tool in WRITE_TOOLS {
        let response = session.call_raw(tool, arguments(tool, &fixture));
        assert!(refused(&response), "{tool}: {response}");
    }
    drop(session);
    assert_eq!(evidence(&fixture.db), before);
}

#[test]
fn write_tools_with_the_flag_go_through_the_use_cases() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let mut session = Session::with_db(&fixture.db, &["--allow-writes"]);
    let open_lead = fixture.open_lead.to_string();

    let added = session.data(
        "lead_add",
        json!({"name": "Nuevo Cliente", "phone": "987000111", "service": "DAC",
               "origin": "inbound_call", "owner": "Ana Torres", "follow_up": "2026-12-01"}),
    );
    assert_eq!(added["created"], json!(true));
    assert_eq!(added["lead"]["owner"], json!("Ana Torres"));
    assert_eq!(added["lead"]["consent"]["given"], json!(true));

    let (code, _) = session.failure(
        "lead_assign",
        json!({"id": open_lead, "owner": "Nadie del equipo"}),
    );
    assert_eq!(code, "owner_not_in_team");
    let assigned = session.data("lead_assign", arguments("lead_assign", &fixture));
    assert_eq!(assigned["lead"]["owner"], json!("Ana Torres"));

    let (code, _) = session.failure(
        "lead_revoke_consent",
        json!({"id": open_lead, "source": "x"}),
    );
    assert_eq!(code, "no_consent");
    let consented = session.data("lead_consent", arguments("lead_consent", &fixture));
    assert_eq!(consented["lead"]["consent"]["given"], json!(true));
    let revoked = session.data(
        "lead_revoke_consent",
        json!({"id": open_lead, "source": "Pidió no recibir mensajes"}),
    );
    assert_eq!(revoked["lead"]["consent"]["given"], json!(false));

    assert_eq!(
        session.data("step_done", arguments("step_done", &fixture)),
        json!({"changed": true})
    );
    assert_eq!(
        session.data("step_done", arguments("step_done", &fixture)),
        json!({"changed": false})
    );
    let dated = session.data("step_date", arguments("step_date", &fixture));
    assert_eq!(dated["step"]["due_on"], json!("2026-04-01"));
    let (code, _) = session.failure(
        "step_date",
        json!({"id": fixture.step.to_string(), "date": "mañana"}),
    );
    assert_eq!(code, "invalid_date");

    assert_eq!(
        session.data("milestone_pay", arguments("milestone_pay", &fixture)),
        json!({"changed": true})
    );
    let case = session.data("case_show", arguments("case_show", &fixture));
    assert!(case["case"]["milestones"][0]["paid_on"].is_string());
}

fn refuses_without_changes(db: &Path, code: &str) {
    let before = fs::read(db).ok();
    let fixture = Fixture {
        db: db.to_path_buf(),
        backup_folder: db.to_path_buf(),
        signed_lead: uuid::Uuid::nil(),
        open_lead: uuid::Uuid::nil(),
        case: uuid::Uuid::nil(),
        step: uuid::Uuid::nil(),
        milestone: uuid::Uuid::nil(),
        quote_number: "COT-2026-0001".to_owned(),
    };
    let mut session = Session::with_db(db, &["--allow-writes"]);

    for tool in all_tools() {
        let (found, _) = session.failure(&tool, arguments(&tool, &fixture));
        assert_eq!(found, code, "{tool} on {}", db.display());
    }
    drop(session);
    assert_eq!(fs::read(db).ok(), before, "{}", db.display());
}

#[test]
fn older_newer_and_absent_databases_are_refused_without_changes() {
    let scratch = Scratch::new();
    let older = scratch.database_at(8, "vieja.db");
    let newer = scratch.newer_database("nueva.db");
    let absent = scratch.file("ausente.db");
    let names = scratch.names();

    refuses_without_changes(&older, "schema_outdated");
    refuses_without_changes(&newer, "schema_newer");
    refuses_without_changes(&absent, "database_missing");

    assert!(!absent.exists(), "a call created the database");
    let after: Vec<String> = scratch
        .names()
        .into_iter()
        .filter(|name| !name.ends_with("-wal") && !name.ends_with("-shm"))
        .collect();
    assert_eq!(
        after, names,
        "a call left a new file, such as a migration copy"
    );
}

#[test]
fn stdout_carries_only_protocol_messages() {
    let scratch = Scratch::new();
    let fixture = synthetic(&scratch);
    let mut session = Session::with_db(&fixture.db, &["--allow-writes"]);
    for tool in all_tools() {
        session.call_raw(&tool, arguments(&tool, &fixture));
    }

    let (stdout, _stderr) = session.finish();
    assert!(!stdout.is_empty());
    for line in stdout {
        let message: Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("{line}"));
        assert_eq!(message["jsonrpc"], json!("2.0"), "{line}");
    }
}

#[test]
fn an_unknown_flag_is_a_usage_error_on_stderr() {
    let output = binary().arg("--write-everything").output().unwrap();

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(!output.stderr.is_empty());
}
