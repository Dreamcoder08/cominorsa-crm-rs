//! Black-box tests of the `cominorsa` binary. Each asserts stdout, stderr, the
//! exit code, and, through `export`, what was persisted.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};
use uuid::Uuid;

const TODAY: &str = "2026-03-02";
const HEADER: &str = "titular,perfil_icp,n_concesiones,hectareas_totales,departamentos,provincias,sustancias,estados_concesion,servicio_potencial,revisar_manual,fuente_dato";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-cli-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// A database made the only way the CLI allows, on first use.
    fn db(&self) -> PathBuf {
        let db = self.0.join("crm.db");
        if !db.exists() {
            let created = run(&db, &["init"]);
            assert!(created.status.success(), "{created:?}");
        }
        db
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cominorsa"));
    command.env_remove("COMINORSA_DB_PATH");
    command
}

fn run_raw(args: &[&str]) -> Output {
    binary().args(args).output().unwrap()
}

/// Runs against `db` in JSON mode, at the fixed day.
fn run(db: &Path, args: &[&str]) -> Output {
    binary()
        .arg("--db")
        .arg(db)
        .args(["--json", "--today", TODAY])
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// Asserts success with one JSON document on stdout and nothing on stderr.
#[track_caller]
fn data(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(output));
    assert_eq!(stderr(output), "");
    let parsed: Value = serde_json::from_str(&stdout(output)).unwrap();
    assert_eq!(parsed.as_object().unwrap().len(), 1, "only `data`");
    parsed["data"].clone()
}

/// Asserts failure with one machine-readable error on stderr and an empty stdout.
#[track_caller]
fn failure(output: &Output, exit: i32) -> (String, String) {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stderr: {}",
        stderr(output)
    );
    assert_eq!(stdout(output), "");
    let parsed: Value = serde_json::from_str(&stderr(output)).unwrap();
    let error = parsed["error"].as_object().unwrap();
    assert_eq!(error.len(), 2, "only `code` and `message`");

    (
        error["code"].as_str().unwrap().to_owned(),
        error["message"].as_str().unwrap().to_owned(),
    )
}

fn export(db: &Path) -> Value {
    data(&run(db, &["export"]))
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}

fn add_lead(db: &Path, seed: &str, name: &str, service: &str) -> String {
    let output = run(
        db,
        &[
            "--id-seed",
            seed,
            "lead",
            "add",
            "--name",
            name,
            "--phone",
            "987654321",
            "--service",
            service,
            "--follow-up",
            "2026-03-10",
        ],
    );

    text(&data(&output)["lead"]["id"]).to_owned()
}

/// Signs a lead and returns its id and its case.
fn signed(db: &Path, seed: &str, service: &str) -> (String, Value) {
    let lead = add_lead(db, seed, "Minera Sintética", service);
    data(&run(
        db,
        &["--id-seed", &format!("{seed}-1"), "lead", "advance", &lead],
    ));
    data(&run(
        db,
        &["--id-seed", &format!("{seed}-2"), "lead", "advance", &lead],
    ));
    let last = data(&run(
        db,
        &["--id-seed", &format!("{seed}-3"), "lead", "advance", &lead],
    ));

    (lead, last["case"].clone())
}

fn quote(db: &Path, lead: &str) -> Value {
    data(&run(
        db,
        &[
            "quote",
            "create",
            "--lead",
            lead,
            "--line",
            "Estudio|2|100.00",
            "--notes",
            "Sintética",
        ],
    ))["quote"]
        .clone()
}

fn name_company(db: &Path) {
    data(&run(
        db,
        &["settings", "set", "--company-name", "Sintética SAC"],
    ));
}

fn write_csv(scratch: &Scratch) -> (String, String) {
    let company = "Empresa Demo,PERFIL_1_formalizacion,3,120.5,PIURA,SULLANA,M,TITULADA,IGAFOM + REINFO,FALSE,Synthetic";
    let person =
        "Ana Demo,PERFIL_2_cumplimiento,1,10,LIMA,HUAROCHIRI,M,TITULADA,DAC,FALSE,Synthetic";
    let (companies, people) = (scratch.file("c.csv"), scratch.file("p.csv"));
    fs::write(&companies, format!("{HEADER}\n{company}\n")).unwrap();
    fs::write(&people, format!("{HEADER}\n{person}\n")).unwrap();

    (
        companies.display().to_string(),
        people.display().to_string(),
    )
}

// Safety: the database path.

#[test]
fn without_a_database_path_it_refuses_and_writes_nothing() {
    let output = run_raw(&["--json", "lead", "list"]);

    let (code, message) = failure(&output, 2);
    assert_eq!(code, "missing_database");
    assert!(message.contains("--db"), "{message}");
}

#[test]
fn the_database_can_come_from_the_environment() {
    let scratch = Scratch::new();

    let output = binary()
        .env("COMINORSA_DB_PATH", scratch.file("crm.db"))
        .args(["--json", "settings", "show"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let set = binary()
        .env("COMINORSA_DB_PATH", scratch.db())
        .args(["--json", "settings", "set", "--follow-up-days", "5"])
        .output()
        .unwrap();
    assert_eq!(data(&set)["settings"]["follow_up_days"], 5);
    assert!(scratch.db().exists());
}

#[test]
fn the_flag_wins_over_the_environment() {
    let scratch = Scratch::new();
    let ignored = scratch.file("ignored.db");

    let output = binary()
        .env("COMINORSA_DB_PATH", &ignored)
        .arg("--db")
        .arg(scratch.db())
        .args(["--json", "settings", "set", "--follow-up-days", "5"])
        .output()
        .unwrap();

    data(&output);
    assert!(scratch.db().exists());
    assert!(!ignored.exists());
}

#[test]
fn a_blank_environment_path_counts_as_missing() {
    let output = binary()
        .env("COMINORSA_DB_PATH", "")
        .args(["--json", "lead", "list"])
        .output()
        .unwrap();

    assert_eq!(failure(&output, 2).0, "missing_database");
}

#[test]
fn a_read_never_creates_the_database() {
    let scratch = Scratch::new();

    for command in [
        vec!["status"],
        vec!["export"],
        vec!["agenda"],
        vec!["lead", "list"],
        vec!["settings", "show"],
    ] {
        let (code, _) = failure(&run(&scratch.file("crm.db"), &command), 3);
        assert_eq!(code, "database_missing");
    }
    assert!(!scratch.file("crm.db").exists());
}

// Output contract and exit codes.

#[test]
fn help_is_in_spanish_and_needs_no_database() {
    let output = run_raw(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), "");
    let help = stdout(&output);
    assert!(help.contains("Comandos"), "{help}");
    assert!(help.contains("Opciones"), "{help}");
    assert!(help.contains("--db"), "{help}");
    assert!(!help.contains("Print help"), "{help}");
}

#[test]
fn every_subcommand_has_help() {
    let output = run_raw(&["quote", "pdf", "--help"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("--out"));
    assert!(stdout(&output).contains("Guarda"), "{}", stdout(&output));
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let scratch = Scratch::new();

    let (code, message) = failure(&run(&scratch.file("crm.db"), &["nonsense"]), 2);

    assert_eq!(code, "usage");
    assert!(message.contains("nonsense"), "{message}");
    assert!(!scratch.file("crm.db").exists());
}

#[test]
fn a_missing_argument_is_a_usage_error_in_spanish() {
    let scratch = Scratch::new();

    let output = binary()
        .arg("--db")
        .arg(scratch.db())
        .args(["lead", "add", "--name", "Solo nombre"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("Falta"), "{}", stderr(&output));
}

#[test]
fn no_command_is_a_usage_error() {
    assert_eq!(failure(&run_raw(&["--json"]), 2).0, "usage");
}

#[test]
fn an_invalid_day_is_a_usage_error() {
    let scratch = Scratch::new();

    let output = binary()
        .arg("--db")
        .arg(scratch.db())
        .args(["--json", "--today", "ayer", "lead", "list"])
        .output()
        .unwrap();

    assert_eq!(failure(&output, 2).0, "usage");
}

#[test]
fn a_rejection_exits_1_with_a_stable_code_and_spanish_message() {
    let scratch = Scratch::new();

    let output = run(
        &scratch.db(),
        &[
            "lead",
            "add",
            "--name",
            "",
            "--phone",
            "1",
            "--service",
            "DAC",
        ],
    );

    let (code, message) = failure(&output, 1);
    assert_eq!(code, "missing_field");
    assert_eq!(message, "El campo nombre es obligatorio");
}

#[test]
fn human_output_is_in_spanish() {
    let scratch = Scratch::new();

    let output = binary()
        .arg("--db")
        .arg(scratch.db())
        .args([
            "--today",
            TODAY,
            "lead",
            "add",
            "--name",
            "Lead Uno",
            "--phone",
            "987654321",
            "--service",
            "DAC",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout(&output).starts_with("Lead creado: Lead Uno"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_human_error_goes_to_stderr_as_plain_text() {
    let scratch = Scratch::new();
    add_lead(&scratch.db(), "a", "Lead", "DAC");

    let output = binary()
        .arg("--db")
        .arg(scratch.db())
        .args(["lead", "advance", &Uuid::nil().to_string()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "No se encontró el lead\n");
}

#[test]
fn storage_failures_exit_3() {
    let scratch = Scratch::new();

    let output = run(&scratch.0, &["settings", "set", "--follow-up-days", "5"]);

    assert_eq!(failure(&output, 3).0, "storage_error");
}

#[test]
fn a_file_that_is_not_a_database_exits_3() {
    let scratch = Scratch::new();
    fs::write(scratch.db(), "esto no es sqlite, solo texto de prueba").unwrap();

    assert_eq!(
        failure(&run(&scratch.db(), &["status"]), 3).0,
        "storage_error"
    );
}

// Determinism.

#[test]
fn the_same_seed_gives_the_same_id() {
    let (one, two) = (Scratch::new(), Scratch::new());

    assert_eq!(
        add_lead(&one.db(), "s", "Lead", "DAC"),
        add_lead(&two.db(), "s", "Lead", "DAC")
    );
}

#[test]
fn without_a_seed_ids_are_time_ordered() {
    let scratch = Scratch::new();

    let output = run(
        &scratch.db(),
        &[
            "lead",
            "add",
            "--name",
            "L",
            "--phone",
            "1",
            "--service",
            "DAC",
        ],
    );

    let id: Uuid = text(&data(&output)["lead"]["id"]).parse().unwrap();
    assert_eq!(id.get_version_num(), 7);
}

#[test]
fn the_injected_day_decides_default_dates() {
    let scratch = Scratch::new();

    let output = run(
        &scratch.db(),
        &[
            "lead",
            "add",
            "--name",
            "L",
            "--phone",
            "1",
            "--service",
            "DAC",
        ],
    );

    assert_eq!(data(&output)["lead"]["follow_up_on"], "2026-03-05");
}

#[test]
fn the_same_commands_give_byte_identical_exports() {
    let (one, two) = (Scratch::new(), Scratch::new());
    let flow = |db: &Path| {
        name_company(db);
        let (lead, _) = signed(db, "flow", "DAC");
        let case = first_case(db);
        let create = [
            "--id-seed",
            "quote",
            "quote",
            "create",
            "--lead",
            &lead,
            "--line",
            "A|2|100",
        ];
        data(&run(db, &create));
        data(&run(
            db,
            &[
                "--id-seed",
                "total",
                "billing",
                "total",
                &case,
                "--amount",
                "1000",
            ],
        ));
        data(&run(db, &["--id-seed", "plan", "billing", "plan", &case]));
        data(&run(db, &["--id-seed", "close", "case", "close", &case]));
        stdout(&run(db, &["export"]))
    };

    assert_eq!(flow(&one.db()), flow(&two.db()));
}

#[test]
fn repeating_a_seeded_quote_uses_no_number() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    let args = [
        "--id-seed",
        "q",
        "quote",
        "create",
        "--lead",
        &lead,
        "--line",
        "Estudio|1|100",
    ];

    let first = data(&run(&scratch.db(), &args));
    let state = export(&scratch.db());
    let repeat = data(&run(&scratch.db(), &args));

    assert_eq!(repeat["quote"]["id"], first["quote"]["id"]);
    assert_eq!(repeat["quote"]["number"], "COT-2026-0001");
    assert_eq!(export(&scratch.db()), state);
    assert_eq!(
        state["quote_counters"],
        json!([{"year": 2026, "last_sequence": 1}])
    );
}

fn first_case(db: &Path) -> String {
    text(&export(db)["cases"][0]["id"]).to_owned()
}

// Status and export.

#[test]
fn status_reports_the_schema_integrity_and_counts() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "REINFO");
    data(&run(&scratch.db(), &["case", "close", text(&case["id"])]));

    let found = data(&run(&scratch.db(), &["status"]));

    assert_eq!(found["schema_version"], 9);
    assert_eq!(found["integrity_check"], "ok");
    assert_eq!(found["counts"]["leads"], 1);
    assert_eq!(found["counts"]["cases"], 1);
    assert_eq!(found["counts"]["active_cases"], 0);
    assert!(found["counts"]["case_steps"].as_u64().unwrap() > 0);
}

#[test]
fn export_is_stable_between_reads() {
    let scratch = Scratch::new();
    signed(&scratch.db(), "s", "DAC");

    let first = stdout(&run(&scratch.db(), &["export"]));
    let second = stdout(&run(&scratch.db(), &["export"]));

    assert_eq!(first, second);
    let state: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(
        state["data"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [
            "cases",
            "leads",
            "prospects",
            "quote_counters",
            "quotes",
            "schema_version",
            "settings",
            "unreadable_rows"
        ]
    );
}

// Leads.

#[test]
fn a_lead_moves_through_the_stages_and_opens_one_case() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "l", "Lead", "IGAFOM + REINFO");

    let first = data(&run(&scratch.db(), &["lead", "advance", &lead]));
    assert_eq!(first["lead"]["stage"], "coordinate_evaluation");
    assert_eq!(first["case"], Value::Null);
    data(&run(&scratch.db(), &["lead", "advance", &lead]));
    let last = data(&run(&scratch.db(), &["lead", "advance", &lead]));

    assert_eq!(last["lead"]["stage"], "contract_signed");
    assert_eq!(last["case"]["lead_id"], lead.as_str());
    let state = export(&scratch.db());
    assert_eq!(state["cases"].as_array().unwrap().len(), 1);
    let steps = state["cases"][0]["steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|step| text(&step["title"]).starts_with("IGAFOM"))
    );
    assert!(
        steps
            .iter()
            .any(|step| text(&step["title"]).starts_with("REINFO"))
    );
}

#[test]
fn advancing_past_the_end_is_refused_without_changes() {
    let scratch = Scratch::new();
    let (lead, _) = signed(&scratch.db(), "s", "DAC");
    let before = export(&scratch.db());

    let (code, _) = failure(&run(&scratch.db(), &["lead", "advance", &lead]), 1);

    assert_eq!(code, "final_stage");
    assert_eq!(export(&scratch.db()), before);
}

#[test]
fn a_lead_list_can_be_searched() {
    let scratch = Scratch::new();
    add_lead(&scratch.db(), "a", "Minera Alfa", "DAC");
    add_lead(&scratch.db(), "b", "Minera Beta", "DAC");

    let found = data(&run(&scratch.db(), &["lead", "list", "--query", "beta"]));

    assert_eq!(found["leads"].as_array().unwrap().len(), 1);
    assert_eq!(found["leads"][0]["name"], "Minera Beta");
}

#[test]
fn a_lead_can_be_assigned() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "a", "Lead", "DAC");
    data(&run(
        &scratch.db(),
        &["settings", "set", "--add-member", "Ana"],
    ));

    data(&run(
        &scratch.db(),
        &["lead", "assign", &lead, "--owner", "Ana"],
    ));

    assert_eq!(export(&scratch.db())["leads"][0]["owner"], "Ana");
}

#[test]
fn the_pipeline_summary_uses_the_injected_day() {
    let scratch = Scratch::new();
    add_lead(&scratch.db(), "a", "Lead", "DAC");

    let found = binary()
        .arg("--db")
        .arg(scratch.db())
        .args(["--json", "--today", "2026-03-11", "lead", "summary"])
        .output()
        .unwrap();

    assert_eq!(
        data(&found),
        json!({"active": 1, "overdue": 1, "due_today": 0, "signed": 0})
    );
}

#[test]
fn whatsapp_only_prints_a_prepared_url() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "a", "Lead", "DAC");
    data(&run(
        &scratch.db(),
        &["lead", "consent", &lead, "--source", "Aceptó por llamada"],
    ));

    let found = data(&run(&scratch.db(), &["lead", "whatsapp", &lead]));

    assert!(text(&found["url"]).starts_with("https://wa.me/51987654321?text="));
    assert_eq!(found.as_object().unwrap().len(), 1);
}

#[test]
fn a_phone_without_a_chat_number_gets_no_url() {
    let scratch = Scratch::new();
    let output = run(
        &scratch.db(),
        &[
            "lead",
            "add",
            "--name",
            "L",
            "--phone",
            "12",
            "--service",
            "DAC",
            "--origin",
            "inbound_call",
        ],
    );
    let lead = text(&data(&output)["lead"]["id"]).to_owned();

    let (code, _) = failure(&run(&scratch.db(), &["lead", "whatsapp", &lead]), 1);

    assert_eq!(code, "no_whatsapp_number");
}

#[test]
fn the_cli_offers_no_outreach() {
    let help = stdout(&run_raw(&["lead", "--help"]));

    assert!(help.contains("whatsapp"), "{help}");
    assert!(help.contains("No envía"), "{help}");
    let (code, _) = failure(&run_raw(&["--json", "lead", "send"]), 2);
    assert_eq!(code, "usage");
}

// Prospects.

#[test]
fn prospects_import_through_the_existing_importer_and_repeat_safely() {
    let scratch = Scratch::new();
    let (companies, people) = write_csv(&scratch);
    let args = [
        "prospect",
        "import",
        "--companies",
        &companies,
        "--people",
        &people,
    ];

    let first = data(&run(&scratch.db(), &args));
    let state = export(&scratch.db());
    let repeat = data(&run(&scratch.db(), &args));

    assert_eq!(first["added"], 2);
    assert_eq!(first["total"], 2);
    assert!(Path::new(text(&first["backup"])).exists());
    assert_eq!(repeat["added"], 0);
    assert_eq!(export(&scratch.db()), state);
}

#[test]
fn an_invalid_import_is_a_rejection_that_names_no_value() {
    let scratch = Scratch::new();
    let (companies, people) = write_csv(&scratch);
    data(&run(
        &scratch.db(),
        &[
            "prospect",
            "import",
            "--companies",
            &companies,
            "--people",
            &people,
        ],
    ));
    let before = export(&scratch.db());
    let bad = scratch.file("bad.csv");
    fs::write(
        &bad,
        format!("{HEADER}\nSecreto,X,tres,1,A,B,C,D,E,FALSE,S\n"),
    )
    .unwrap();

    let output = run(
        &scratch.db(),
        &[
            "prospect",
            "import",
            "--companies",
            bad.to_str().unwrap(),
            "--people",
            &people,
        ],
    );

    let (code, message) = failure(&output, 1);
    assert_eq!(code, "import_invalid");
    assert!(!message.contains("Secreto"));
    assert_eq!(export(&scratch.db()), before);
}

#[test]
fn prospects_can_be_counted_searched_and_converted() {
    let scratch = Scratch::new();
    let (companies, people) = write_csv(&scratch);
    data(&run(
        &scratch.db(),
        &[
            "prospect",
            "import",
            "--companies",
            &companies,
            "--people",
            &people,
        ],
    ));

    let counted = data(&run(&scratch.db(), &["prospect", "count"]));
    let found = data(&run(
        &scratch.db(),
        &["prospect", "search", "--query", "piura"],
    ));
    let lead = data(&run(
        &scratch.db(),
        &[
            "prospect",
            "convert",
            "--name",
            "Empresa Demo",
            "--phone",
            "987654321",
        ],
    ));

    assert_eq!(counted["total"], 2);
    assert_eq!(found["prospects"].as_array().unwrap().len(), 1);
    assert_eq!(found["prospects"][0]["kind"], "company");
    assert_eq!(lead["lead"]["service"], "IGAFOM + REINFO");
    assert_eq!(lead["lead"]["follow_up_on"], TODAY);
}

// Steps and cases.

#[test]
fn steps_are_marked_undone_and_rescheduled() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let step = text(&case["steps"][0]["id"]).to_owned();

    let done = data(&run(&scratch.db(), &["step", "done", &step]));
    let repeat = data(&run(&scratch.db(), &["step", "done", &step]));
    let saved = export(&scratch.db());
    data(&run(&scratch.db(), &["step", "undo", &step]));
    data(&run(
        &scratch.db(),
        &["step", "date", &step, "--date", "2026-06-01"],
    ));

    assert_eq!(done["changed"], true);
    assert_eq!(repeat["changed"], false);
    assert_eq!(saved["cases"][0]["steps"][0]["done_on"], TODAY);
    let stored = export(&scratch.db());
    assert_eq!(stored["cases"][0]["steps"][0]["done_on"], Value::Null);
    assert_eq!(stored["cases"][0]["steps"][0]["due_on"], "2026-06-01");
}

#[test]
fn a_bad_step_date_is_refused_without_changes() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let before = export(&scratch.db());

    let output = run(
        &scratch.db(),
        &[
            "step",
            "date",
            text(&case["steps"][0]["id"]),
            "--date",
            "mañana",
        ],
    );

    assert_eq!(failure(&output, 1).0, "invalid_date");
    assert_eq!(export(&scratch.db()), before);
}

#[test]
fn a_case_shows_its_billing_summary() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let id = text(&case["id"]);
    data(&run(
        &scratch.db(),
        &["billing", "total", id, "--amount", "1000"],
    ));

    let shown = data(&run(&scratch.db(), &["case", "show", id]));
    let listed = data(&run(&scratch.db(), &["case", "list"]));

    assert_eq!(shown["case"]["contract_total_cents"], 100_000);
    assert_eq!(shown["billing"]["pending_cents"], 100_000);
    assert_eq!(shown["billing"]["collected_cents"], 0);
    assert_eq!(listed["cases"].as_array().unwrap().len(), 1);
}

#[test]
fn closing_a_recurring_case_creates_one_renewal_lead() {
    let scratch = Scratch::new();
    let (lead, case) = signed(&scratch.db(), "s", "DAC");
    let id = text(&case["id"]);
    data(&run(
        &scratch.db(),
        &[
            "billing",
            "add",
            id,
            "--title",
            "Saldo",
            "--amount",
            "100",
            "--due",
            "2026-03-20",
        ],
    ));

    let closed = data(&run(
        &scratch.db(),
        &["--id-seed", "close", "case", "close", id],
    ));
    let state = export(&scratch.db());
    let repeat = data(&run(
        &scratch.db(),
        &["--id-seed", "again", "case", "close", id],
    ));

    assert_eq!(closed["closed"], true);
    assert_eq!(closed["renewal"]["stage"], "initial_contact");
    assert_eq!(closed["renewal"]["renewal_of"], id);
    assert_eq!(closed["renewal"]["follow_up_on"], "2027-03-02");
    assert_ne!(closed["renewal"]["id"], lead.as_str());
    assert_eq!(repeat["closed"], false);
    assert_eq!(repeat["renewal"], Value::Null);
    assert_eq!(export(&scratch.db()), state);
}

#[test]
fn closing_a_one_off_case_creates_no_lead() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "REINFO");

    let closed = data(&run(&scratch.db(), &["case", "close", text(&case["id"])]));

    assert_eq!(closed["renewal"], Value::Null);
    assert_eq!(export(&scratch.db())["leads"].as_array().unwrap().len(), 1);
}

#[test]
fn a_settled_case_can_be_shown_and_closing_it_again_changes_nothing() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let id = text(&case["id"]);
    data(&run(
        &scratch.db(),
        &["--id-seed", "close", "case", "close", id],
    ));
    let state = export(&scratch.db());

    let shown = data(&run(&scratch.db(), &["case", "show", id]));
    let repeat = data(&run(
        &scratch.db(),
        &["--id-seed", "again", "case", "close", id],
    ));
    let listed = data(&run(&scratch.db(), &["case", "list"]));

    assert_eq!(shown["case"]["status"], "closed");
    assert_eq!(state["cases"][0]["status"], "closed");
    assert_eq!(repeat, json!({"closed": false, "renewal": null}));
    assert_eq!(listed["cases"], json!([]));
    assert_eq!(export(&scratch.db()), state);
}

#[test]
fn an_unknown_case_is_a_rejection() {
    let scratch = Scratch::new();
    add_lead(&scratch.db(), "a", "Lead", "DAC");
    let nil = Uuid::nil().to_string();

    assert_eq!(
        failure(&run(&scratch.db(), &["case", "show", &nil]), 1).0,
        "case_not_found"
    );
    assert_eq!(
        failure(&run(&scratch.db(), &["case", "close", &nil]), 1).0,
        "case_not_found"
    );
}

// Read commands never write.

#[test]
fn reads_leave_the_database_file_byte_identical() {
    let scratch = Scratch::new();
    let (companies, people) = write_csv(&scratch);
    let import = [
        "prospect",
        "import",
        "--companies",
        &companies,
        "--people",
        &people,
    ];
    data(&run(&scratch.db(), &import));
    let (lead, case) = signed(&scratch.db(), "s", "DAC");
    quote(&scratch.db(), &lead);
    data(&run(
        &scratch.db(),
        &["lead", "consent", &lead, "--source", "Aceptó por llamada"],
    ));
    let case = text(&case["id"]);
    let pdf = scratch.file("lectura.pdf");
    let before = fs::read(scratch.db()).unwrap();

    for command in [
        vec!["status"],
        vec!["export"],
        vec!["agenda"],
        vec!["lead", "list"],
        vec!["lead", "show", &lead],
        vec!["lead", "summary"],
        vec!["lead", "whatsapp", &lead],
        vec!["prospect", "search", "--query", "piura"],
        vec!["prospect", "count"],
        vec!["case", "list"],
        vec!["case", "show", case],
        vec!["quote", "list"],
        vec!["quote", "show", "COT-2026-0001"],
        vec![
            "quote",
            "pdf",
            "COT-2026-0001",
            "--out",
            pdf.to_str().unwrap(),
        ],
        vec!["settings", "show"],
    ] {
        data(&run(&scratch.db(), &command));
        assert_eq!(fs::read(scratch.db()).unwrap(), before, "{command:?} wrote");
    }
}

#[test]
fn migrating_a_current_database_reports_no_change() {
    let scratch = Scratch::new();
    add_lead(&scratch.db(), "a", "Lead", "DAC");
    let before = fs::read(scratch.db()).unwrap();

    let found = data(&run(&scratch.db(), &["migrate"]));

    assert_eq!(
        found,
        json!({"changed": false, "from": 9, "to": 9, "safety_copy": null})
    );
    assert_eq!(fs::read(scratch.db()).unwrap(), before);
}

#[test]
fn migrating_a_missing_database_creates_nothing() {
    let scratch = Scratch::new();

    assert_eq!(
        failure(&run(&scratch.file("crm.db"), &["migrate"]), 3).0,
        "database_missing"
    );
    assert!(!scratch.file("crm.db").exists());
}

// Billing.

#[test]
fn the_suggested_plan_splits_the_total_and_refuses_a_repeat() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let id = text(&case["id"]);
    data(&run(
        &scratch.db(),
        &["billing", "total", id, "--amount", "1000.00"],
    ));

    data(&run(&scratch.db(), &["billing", "plan", id]));
    let before = export(&scratch.db());
    let (code, _) = failure(&run(&scratch.db(), &["billing", "plan", id]), 1);

    let amounts: Vec<&Value> = before["cases"][0]["milestones"]
        .as_array()
        .unwrap()
        .iter()
        .map(|milestone| &milestone["amount_cents"])
        .collect();
    assert_eq!(amounts, [30_000, 40_000, 30_000]);
    assert_eq!(code, "milestones_already_planned");
    assert_eq!(export(&scratch.db()), before);
}

#[test]
fn a_plan_needs_a_contract_total() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");

    let output = run(&scratch.db(), &["billing", "plan", text(&case["id"])]);

    assert_eq!(failure(&output, 1).0, "missing_contract_total");
}

#[test]
fn paying_twice_keeps_the_first_date_and_undoing_clears_it() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let id = text(&case["id"]);
    let added = data(&run(
        &scratch.db(),
        &[
            "billing",
            "add",
            id,
            "--title",
            "Adelanto",
            "--amount",
            "100",
            "--due",
            "2026-03-20",
        ],
    ));
    let milestone = text(&added["milestones"][0]["id"]).to_owned();

    let paid = data(&run(&scratch.db(), &["billing", "pay", &milestone]));
    let later = binary()
        .arg("--db")
        .arg(scratch.db())
        .args([
            "--json",
            "--today",
            "2026-04-01",
            "billing",
            "pay",
            &milestone,
        ])
        .output()
        .unwrap();
    let repeat = data(&later);
    let stored = export(&scratch.db());
    data(&run(&scratch.db(), &["billing", "unpay", &milestone]));

    assert_eq!(paid["changed"], true);
    assert_eq!(repeat["changed"], false);
    assert_eq!(stored["cases"][0]["milestones"][0]["paid_on"], TODAY);
    assert_eq!(
        export(&scratch.db())["cases"][0]["milestones"][0]["paid_on"],
        Value::Null
    );
}

#[test]
fn a_milestone_edit_applies_every_field_or_none() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let added = data(&run(
        &scratch.db(),
        &[
            "billing",
            "add",
            text(&case["id"]),
            "--title",
            "Adelanto",
            "--amount",
            "100",
            "--due",
            "2026-03-20",
        ],
    ));
    let milestone = text(&added["milestones"][0]["id"]).to_owned();
    let before = export(&scratch.db());

    let refused = run(
        &scratch.db(),
        &[
            "billing", "edit", &milestone, "--title", "Nuevo", "--amount", "cero",
        ],
    );
    assert_eq!(failure(&refused, 1).0, "invalid_amount");
    assert_eq!(export(&scratch.db()), before);

    data(&run(
        &scratch.db(),
        &[
            "billing",
            "edit",
            &milestone,
            "--title",
            "Nuevo",
            "--amount",
            "250",
            "--due",
            "2026-04-01",
        ],
    ));
    let saved = &export(&scratch.db())["cases"][0]["milestones"][0];
    assert_eq!(saved["title"], "Nuevo");
    assert_eq!(saved["amount_cents"], 25_000);
    assert_eq!(saved["due_on"], "2026-04-01");
}

#[test]
fn a_milestone_can_be_deleted() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    let added = data(&run(
        &scratch.db(),
        &[
            "billing",
            "add",
            text(&case["id"]),
            "--title",
            "Adelanto",
            "--amount",
            "100",
            "--due",
            "2026-03-20",
        ],
    ));

    data(&run(
        &scratch.db(),
        &["billing", "delete", text(&added["milestones"][0]["id"])],
    ));

    assert_eq!(export(&scratch.db())["cases"][0]["milestones"], json!([]));
}

// Agenda.

#[test]
fn the_agenda_groups_items_around_the_injected_day() {
    let scratch = Scratch::new();
    let (_, case) = signed(&scratch.db(), "s", "DAC");
    add_lead(&scratch.db(), "other", "Lead Pendiente", "DAC");
    data(&run(
        &scratch.db(),
        &[
            "billing",
            "add",
            text(&case["id"]),
            "--title",
            "Adelanto",
            "--amount",
            "100",
            "--due",
            "2026-03-10",
        ],
    ));

    let day = |today: &str| {
        let output = binary()
            .arg("--db")
            .arg(scratch.db())
            .args(["--json", "--today", today, "agenda"])
            .output()
            .unwrap();
        data(&output)
    };
    let due = day("2026-03-10");
    let late = day("2026-03-12");

    assert_eq!(due["date"], "2026-03-10");
    let kinds = |items: &Value| {
        let mut found: Vec<String> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|item| text(&item["kind"]).to_owned())
            .collect();
        found.sort();
        found
    };
    assert!(kinds(&due["due_today"]).contains(&"follow_up".to_owned()));
    assert!(kinds(&due["due_today"]).contains(&"milestone".to_owned()));
    assert!(kinds(&late["overdue"]).contains(&"milestone".to_owned()));
}

// Quotes.

#[test]
fn a_quote_goes_from_draft_to_accepted_and_suggests_the_contract_total() {
    let scratch = Scratch::new();
    name_company(&scratch.db());
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    let created = quote(&scratch.db(), &lead);

    assert_eq!(created["number"], "COT-2026-0001");
    assert_eq!(created["status"], "draft");
    assert_eq!(
        created["totals"],
        json!({"subtotal_cents": 20_000, "igv_cents": 3_600, "total_cents": 23_600})
    );

    let sent = data(&run(&scratch.db(), &["quote", "send", "COT-2026-0001"]));
    assert_eq!(sent["quote"]["status"], "sent");
    assert_eq!(sent["quote"]["company"]["name"], "Sintética SAC");
    let accepted = data(&run(&scratch.db(), &["quote", "accept", "COT-2026-0001"]));
    assert_eq!(accepted["quote"]["status"], "accepted");

    for _ in 0..3 {
        data(&run(&scratch.db(), &["lead", "advance", &lead]));
    }
    assert_eq!(
        export(&scratch.db())["cases"][0]["contract_total_cents"],
        23_600
    );
}

#[test]
fn sending_without_a_company_name_needs_the_explicit_flag() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    quote(&scratch.db(), &lead);
    let before = export(&scratch.db());

    let (code, _) = failure(&run(&scratch.db(), &["quote", "send", "COT-2026-0001"]), 1);
    assert_eq!(code, "company_name_missing");
    assert_eq!(export(&scratch.db()), before);

    let sent = data(&run(
        &scratch.db(),
        &["quote", "send", "COT-2026-0001", "--allow-blank-company"],
    ));
    assert_eq!(sent["quote"]["status"], "sent");
}

#[test]
fn a_sent_quote_is_locked_and_a_new_version_repeats_safely() {
    let scratch = Scratch::new();
    name_company(&scratch.db());
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    quote(&scratch.db(), &lead);
    data(&run(&scratch.db(), &["quote", "send", "COT-2026-0001"]));
    let before = export(&scratch.db());

    let (code, _) = failure(
        &run(
            &scratch.db(),
            &["quote", "edit", "COT-2026-0001", "--notes", "Cambio"],
        ),
        1,
    );
    assert_eq!(code, "quote_locked");
    assert_eq!(export(&scratch.db()), before);

    let first = data(&run(&scratch.db(), &["quote", "version", "COT-2026-0001"]));
    let repeat = data(&run(&scratch.db(), &["quote", "version", "COT-2026-0001"]));
    assert_eq!(first["quote"]["number"], "COT-2026-0001-V2");
    assert_eq!(repeat["quote"]["id"], first["quote"]["id"]);
    assert_eq!(export(&scratch.db())["quotes"].as_array().unwrap().len(), 2);

    let old = run(&scratch.db(), &["quote", "accept", "COT-2026-0001"]);
    assert_eq!(failure(&old, 1).0, "quote_superseded");
}

#[test]
fn a_draft_can_be_edited_and_rejected_only_after_sending() {
    let scratch = Scratch::new();
    name_company(&scratch.db());
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    quote(&scratch.db(), &lead);

    let edited = data(&run(
        &scratch.db(),
        &[
            "quote",
            "edit",
            "COT-2026-0001",
            "--notes",
            "Revisada",
            "--line",
            "Nuevo|1|50",
        ],
    ));
    assert_eq!(edited["quote"]["notes"], "Revisada");
    assert_eq!(edited["quote"]["totals"]["subtotal_cents"], 5_000);

    let early = run(&scratch.db(), &["quote", "reject", "COT-2026-0001"]);
    assert_eq!(failure(&early, 1).0, "invalid_quote_transition");

    data(&run(&scratch.db(), &["quote", "send", "COT-2026-0001"]));
    let rejected = data(&run(&scratch.db(), &["quote", "reject", "COT-2026-0001"]));
    assert_eq!(rejected["quote"]["status"], "rejected");
}

#[test]
fn a_malformed_quote_line_is_a_usage_error() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    let before = export(&scratch.db());

    let output = run(
        &scratch.db(),
        &["quote", "create", "--lead", &lead, "--line", "solo texto"],
    );

    assert_eq!(failure(&output, 2).0, "usage");
    assert_eq!(export(&scratch.db()), before);
}

#[test]
fn quotes_can_be_listed_by_lead() {
    let scratch = Scratch::new();
    let one = add_lead(&scratch.db(), "a", "Lead A", "DAC");
    let two = add_lead(&scratch.db(), "b", "Lead B", "DAC");
    quote(&scratch.db(), &one);
    quote(&scratch.db(), &two);

    let all = data(&run(&scratch.db(), &["quote", "list"]));
    let some = data(&run(&scratch.db(), &["quote", "list", "--lead", &two]));

    assert_eq!(all["quotes"].as_array().unwrap().len(), 2);
    assert_eq!(some["quotes"].as_array().unwrap().len(), 1);
    assert_eq!(some["quotes"][0]["number"], "COT-2026-0002");
}

#[test]
fn a_quote_is_saved_as_a_pdf() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    quote(&scratch.db(), &lead);
    let target = scratch.file("cotizacion.pdf");

    let saved = data(&run(
        &scratch.db(),
        &[
            "quote",
            "pdf",
            "COT-2026-0001",
            "--out",
            target.to_str().unwrap(),
        ],
    ));

    assert_eq!(saved["path"], target.to_str().unwrap());
    assert!(fs::read(&target).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn an_unwritable_pdf_path_exits_3() {
    let scratch = Scratch::new();
    let lead = add_lead(&scratch.db(), "l", "Lead", "DAC");
    quote(&scratch.db(), &lead);
    let target = scratch.file("falta/cotizacion.pdf");

    let output = run(
        &scratch.db(),
        &[
            "quote",
            "pdf",
            "COT-2026-0001",
            "--out",
            target.to_str().unwrap(),
        ],
    );

    assert_eq!(failure(&output, 3).0, "pdf_write_failed");
}

// Settings.

#[test]
fn settings_are_set_and_shown() {
    let scratch = Scratch::new();

    data(&run(
        &scratch.db(),
        &[
            "settings",
            "set",
            "--theme",
            "dark",
            "--igv",
            "10.5",
            "--add-member",
            "Ana",
            "--add-member",
            "Luis",
            "--remove-member",
            "Ana",
            "--company-ruc",
            "20100070970",
        ],
    ));

    let shown = data(&run(&scratch.db(), &["settings", "show"]));
    assert_eq!(shown["theme"], "dark");
    assert_eq!(shown["igv_basis_points"], 1050);
    assert_eq!(shown["team"], json!(["Luis"]));
    assert_eq!(shown["company"]["ruc"], "20100070970");
}

#[test]
fn a_refused_setting_stores_nothing() {
    let scratch = Scratch::new();
    data(&run(
        &scratch.db(),
        &["settings", "set", "--follow-up-days", "5"],
    ));
    let before = export(&scratch.db());

    let output = run(
        &scratch.db(),
        &[
            "settings",
            "set",
            "--theme",
            "light",
            "--follow-up-days",
            "500",
        ],
    );

    assert_eq!(failure(&output, 1).0, "invalid_follow_up_days");
    assert_eq!(export(&scratch.db()), before);
}

#[test]
fn adding_a_team_member_twice_is_refused() {
    let scratch = Scratch::new();
    data(&run(
        &scratch.db(),
        &["settings", "set", "--add-member", "Ana"],
    ));

    let output = run(&scratch.db(), &["settings", "set", "--add-member", "Ana"]);

    assert_eq!(failure(&output, 1).0, "duplicate_member");
}

// Backup and restore.

#[test]
fn a_restore_brings_back_the_exact_state() {
    let scratch = Scratch::new();
    let folder = scratch.file("copias");
    signed(&scratch.db(), "s", "DAC");
    let before = export(&scratch.db());

    let backup = data(&run(
        &scratch.db(),
        &[
            "--now",
            "1772470800",
            "backup",
            "create",
            "--folder",
            folder.to_str().unwrap(),
        ],
    ));
    let file = text(&backup["path"]).to_owned();
    assert!(Path::new(&file).exists());
    assert_eq!(
        export(&scratch.db())["settings"]["last_backup_at"],
        1_772_470_800
    );
    add_lead(&scratch.db(), "later", "Lead Posterior", "DAC");

    let restored = data(&run(
        &scratch.db(),
        &["--now", "1772470900", "backup", "restore", "--file", &file],
    ));

    assert_eq!(export(&scratch.db()), before);
    assert!(Path::new(text(&restored["safety_copy"])).exists());
}

#[test]
fn a_backup_without_a_folder_is_refused() {
    let scratch = Scratch::new();
    data(&run(
        &scratch.db(),
        &["settings", "set", "--follow-up-days", "5"],
    ));

    let (code, _) = failure(&run(&scratch.db(), &["backup", "create"]), 1);

    assert_eq!(code, "backup_folder_missing");
}

#[test]
fn restoring_a_file_that_is_not_a_backup_changes_nothing() {
    let scratch = Scratch::new();
    signed(&scratch.db(), "s", "DAC");
    let before = export(&scratch.db());
    let junk = scratch.file("junk.db");
    fs::write(&junk, "no soy una copia").unwrap();

    let output = run(
        &scratch.db(),
        &["backup", "restore", "--file", junk.to_str().unwrap()],
    );

    assert_eq!(failure(&output, 3).0, "storage_error");
    assert_eq!(export(&scratch.db()), before);
}
