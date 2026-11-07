//! Shared fixtures of the MCP black-box tests: a real stdio session against the
//! built binary, and synthetic databases built through the application layer.
//! Old and new schemas are built with the `sqlite3` tool, which the gate
//! already requires.

#![allow(dead_code)] // Each test binary uses part of this module.

use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

use cominorsa_application::{
    BillingEdit, Clock, Context, FormValues, Ids, MilestoneValues, QuoteFields, QuoteLineValues,
    SettingsChange, add_lead, advance_lead, apply_billing, create_database, create_quote,
    find_case, open_for_reading, open_for_writing, snapshot, update_settings,
};
use serde_json::{Value, json};
use time::macros::date;
use uuid::Uuid;

pub const READ_TOOLS: [&str; 11] = [
    "agenda",
    "case_list",
    "case_show",
    "lead_list",
    "lead_show",
    "prospect_count",
    "prospect_search",
    "quote_list",
    "quote_show",
    "settings_show",
    "status",
];

pub const WRITE_TOOLS: [&str; 7] = [
    "lead_add",
    "lead_assign",
    "lead_consent",
    "lead_revoke_consent",
    "milestone_pay",
    "step_date",
    "step_done",
];

const TIMEOUT: Duration = Duration::from_secs(60);

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-mcp-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A real database at schema `version`, holding one synthetic lead. Its
    /// migration records come from a new database, as `migrate` expects.
    pub fn database_at(&self, version: i64, name: &str) -> PathBuf {
        let fresh = self.file("fresh-for-fixture.db");
        block_on(create_database(&fresh)).unwrap();
        let record = sqlite(
            &fresh,
            "SELECT sql FROM sqlite_master WHERE name = '_sqlx_migrations'",
        );
        let mut script = format!("{record};\n");

        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../storage/migrations");
        let mut migrations: Vec<PathBuf> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        migrations.sort();
        for migration in migrations {
            let file_name = migration
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let number: i64 = file_name[..4].parse().unwrap();
            if number <= version {
                script.push_str(&fs::read_to_string(&migration).unwrap());
                script.push('\n');
            }
        }
        script.push_str(&format!(
            "ATTACH '{}' AS fresh;
            INSERT INTO _sqlx_migrations
                SELECT * FROM fresh._sqlx_migrations WHERE version <= {version};
            DETACH fresh;
            INSERT INTO leads (id, name, phone, service, stage, follow_up_on)
                VALUES ('11111111-1111-7111-8111-111111111111', 'Lead Viejo',
                    '987650000', 'DAC', 'initial_contact', '2026-03-01');",
            fresh.display()
        ));

        let database = self.file(name);
        sqlite(&database, &script);
        fs::remove_file(&fresh).unwrap();
        for sidecar in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{sidecar}", fresh.display()));
        }
        database
    }

    /// A database that claims a schema newer than this program knows.
    pub fn newer_database(&self, name: &str) -> PathBuf {
        let database = self.database_at(8, name);
        sqlite(
            &database,
            "INSERT INTO _sqlx_migrations
                SELECT 999, description, installed_on, success, checksum, execution_time
                FROM _sqlx_migrations WHERE version = 8;",
        );
        database
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

/// Runs a script through the `sqlite3` tool and returns its trimmed output.
pub fn sqlite(database: &Path, script: &str) -> String {
    let mut child = Command::new("sqlite3")
        .arg(database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the gate requires sqlite3");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Ids of the synthetic records, for tool arguments.
pub struct Fixture {
    pub db: PathBuf,
    pub backup_folder: PathBuf,
    pub signed_lead: Uuid,
    pub open_lead: Uuid,
    pub case: Uuid,
    pub step: Uuid,
    pub milestone: Uuid,
    pub quote_number: String,
}

fn context(seed: &str) -> Context {
    Context::new(
        Clock::fixed(Some(date!(2026 - 03 - 02)), None),
        Ids::seeded(seed),
    )
}

fn lead_form(name: &str, service: &str, origin: &str, follow_up_on: &str) -> FormValues {
    FormValues {
        name: name.to_owned(),
        phone: "987654321".to_owned(),
        ruc: String::new(),
        service: service.to_owned(),
        owner: String::new(),
        follow_up_on: follow_up_on.to_owned(),
        origin: origin.to_owned(),
    }
}

/// A database at the current schema with a team member, a backup folder, one
/// signed lead with its case, steps, and milestones, and one open lead with a
/// quote. Every value is synthetic.
pub fn synthetic(scratch: &Scratch) -> Fixture {
    let db = scratch.file("crm.db");
    let backup_folder = scratch.file("copias-secretas");
    fs::create_dir_all(&backup_folder).unwrap();

    block_on(async {
        create_database(&db).await.unwrap();
        let store = open_for_writing(&db).await.unwrap();
        let change = SettingsChange {
            add_members: vec!["Ana Torres".to_owned()],
            backup_directory: Some(backup_folder.display().to_string()),
            ..SettingsChange::default()
        };
        update_settings(&store, change).await.unwrap();

        let signed = add_lead(
            &store,
            lead_form("Minera Andina SAC", "DAC", "web_form", "2026-03-01"),
            &mut context("fixture-lead-1"),
        )
        .await
        .unwrap()
        .lead;
        let open = add_lead(
            &store,
            lead_form("Lucía Quispe", "REINFO", "other", "2026-03-10"),
            &mut context("fixture-lead-2"),
        )
        .await
        .unwrap()
        .lead;

        let mut case = None;
        for round in 0..3 {
            let moved = advance_lead(
                &store,
                signed.id(),
                &mut context(&format!("fixture-advance-{round}")),
            )
            .await
            .unwrap();
            case = moved.case.or(case);
        }
        let case = case.expect("the final stage opens a case");
        let draft = MilestoneValues::default();
        apply_billing(
            &store,
            &case,
            BillingEdit::Total("1000.00".to_owned()),
            &draft,
            &mut context("fixture-total"),
        )
        .await
        .unwrap();
        let file = find_case(&store, case.case.id()).await.unwrap();
        apply_billing(
            &store,
            &file,
            BillingEdit::Suggest,
            &draft,
            &mut context("fixture-plan"),
        )
        .await
        .unwrap();
        let file = find_case(&store, case.case.id()).await.unwrap();

        let fields = QuoteFields {
            issued_on: None,
            valid_until: None,
            notes: None,
            lines: vec![QuoteLineValues {
                description: "Evaluación de coordenadas".to_owned(),
                quantity: "1".to_owned(),
                unit_price: "500.00".to_owned(),
            }],
        };
        let quote = create_quote(&store, &open, fields, &mut context("fixture-quote"))
            .await
            .unwrap();
        store.close().await;

        Fixture {
            db: db.clone(),
            backup_folder: backup_folder.clone(),
            signed_lead: signed.id(),
            open_lead: open.id(),
            case: file.case.id(),
            step: file.steps[0].id(),
            milestone: file.milestones[0].id(),
            quote_number: quote.quote.number().to_string(),
        }
    })
}

/// The database bytes and its full exported state, to prove nothing changed.
#[derive(Debug, PartialEq)]
pub struct Evidence {
    bytes: Vec<u8>,
    export: Option<String>,
}

pub fn evidence(db: &Path) -> Evidence {
    let export = block_on(async {
        let store = open_for_reading(db).await.ok()?;
        let state = snapshot(&store).await.ok();
        store.close().await;
        state.map(|state| format!("{state:?}"))
    });

    Evidence {
        bytes: fs::read(db).unwrap(),
        export,
    }
}

/// One MCP client session over the server's stdin and stdout.
pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    stderr: Option<JoinHandle<String>>,
    pub stdout: Vec<String>,
    next_id: u64,
}

pub fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cominorsa-mcp"));
    command.env_remove("COMINORSA_DB_PATH");
    command
}

impl Session {
    /// Starts the server with these arguments and completes the handshake.
    pub fn start(args: &[&str]) -> Self {
        Self::start_with(binary().args(args))
    }

    pub fn with_db(db: &Path, args: &[&str]) -> Self {
        Self::start_with(binary().arg("--db").arg(db).args(args))
    }

    pub fn start_with(command: &mut Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });

        let mut session = Self {
            stdin: child.stdin.take(),
            child,
            lines,
            stderr: Some(stderr),
            stdout: Vec::new(),
            next_id: 0,
        };
        let initialized = session.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "cominorsa-test", "version": "0.0.1"},
            }),
        );
        assert!(initialized.get("result").is_some(), "{initialized}");
        session.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        session
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and returns its response, skipping notifications.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));

        loop {
            let line = self.lines.recv_timeout(TIMEOUT).unwrap_or_else(|_| {
                panic!("no response to {method}; stdout so far: {:?}", self.stdout)
            });
            self.stdout.push(line.clone());
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|_| panic!("stdout carried a non-JSON line: {line}"));
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// The tools the server lists, by name, sorted.
    pub fn tools(&mut self) -> Vec<Value> {
        let listed = self.request("tools/list", json!({}));
        let mut tools = listed["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("{listed}"))
            .clone();
        tools.sort_by_key(|tool| tool["name"].as_str().unwrap().to_owned());
        tools
    }

    pub fn tool_names(&mut self) -> Vec<String> {
        self.tools()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The full JSON-RPC response of a tool call.
    pub fn call_raw(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    /// The `data` of a successful call.
    pub fn data(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.call_raw(name, arguments);
        let result = &response["result"];
        assert_eq!(result["isError"], json!(false), "{name}: {response}");
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed, result["structuredContent"], "{name}: {response}");
        parsed["data"].clone()
    }

    /// The error code and message of a failed call.
    pub fn failure(&mut self, name: &str, arguments: Value) -> (String, String) {
        let response = self.call_raw(name, arguments);
        let result = &response["result"];
        assert_eq!(result["isError"], json!(true), "{name}: {response}");
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        (
            parsed["error"]["code"].as_str().unwrap().to_owned(),
            parsed["error"]["message"].as_str().unwrap().to_owned(),
        )
    }

    /// Closes stdin, waits for exit, and returns every stdout line and stderr.
    pub fn finish(mut self) -> (Vec<String>, String) {
        drop(self.stdin.take());
        let _ = self.child.wait();
        while let Ok(line) = self.lines.recv_timeout(Duration::from_millis(200)) {
            self.stdout.push(line);
        }
        let stderr = self.stderr.take().unwrap().join().unwrap();
        (std::mem::take(&mut self.stdout), stderr)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Minimal arguments that pass validation for each tool, given a fixture.
pub fn arguments(tool: &str, fixture: &Fixture) -> Value {
    match tool {
        "lead_show" => json!({"id": fixture.open_lead.to_string()}),
        "case_show" => json!({"id": fixture.case.to_string()}),
        "quote_show" => json!({"quote": fixture.quote_number}),
        "lead_add" => json!({"name": "Nuevo", "phone": "987000111", "service": "DAC"}),
        "lead_assign" => json!({"id": fixture.open_lead.to_string(), "owner": "Ana Torres"}),
        "lead_consent" | "lead_revoke_consent" => {
            json!({"id": fixture.open_lead.to_string(), "source": "Aceptó por llamada"})
        }
        "step_done" => json!({"id": fixture.step.to_string()}),
        "step_date" => json!({"id": fixture.step.to_string(), "date": "2026-04-01"}),
        "milestone_pay" => json!({"id": fixture.milestone.to_string()}),
        _ => json!({}),
    }
}
