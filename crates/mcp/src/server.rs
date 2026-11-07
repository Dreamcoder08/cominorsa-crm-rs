//! MCP tools over the application layer. Each tool opens the database under the
//! CLI's checks, calls a use case, and answers with the CLI's JSON shapes. No
//! business rule lives here.

use std::path::{Path, PathBuf};

use cominorsa_application::{
    AppError, Context, ErrorKind, FollowUp, FormValues, PROSPECT_PAGE_SIZE, add_lead, agenda,
    assign_lead, close_warning, default_follow_up, find_lead, find_quote, find_stored_case,
    lead_quotes, milestone_gap_warning, open_for_reading, open_for_writing, record_consent,
    reschedule_step, revoke_consent, search_leads, set_paid, set_step_done, status,
    suggested_total_note,
};
use cominorsa_domain::{
    BillingSummary, CaseRepository, ProspectRepository, QuoteRepository, SettingsRepository,
};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::views;

const LIST_LIMIT: u32 = 200;
const LIST_LIMIT_MAX: u32 = 1000;
const PROSPECT_LIMIT_MAX: u32 = 500;
const QUERY_MAX: usize = 200;
const ID_MAX: usize = 64;
const NAME_MAX: usize = 200;
const PHONE_MAX: usize = 40;
const RUC_MAX: usize = 20;
const OWNER_MAX: usize = 100;
const CODE_MAX: usize = 32;
const SOURCE_MAX: usize = 300;

const INSTRUCTIONS: &str = "COMINORSA CRM over the same application layer as the `cominorsa` CLI. \
Results are the CLI's JSON shapes: {\"data\": ...} on success, {\"error\": {\"code\", \"message\"}} \
on failure. Dates are ISO 8601, money is integer cents, enums use stored codes. Read tools never \
change the database. Write tools exist only when the server runs with --allow-writes, and none \
deletes, restores, migrates, or sends messages.";

/// Opens the server's database read-only or to write, under the CLI's checks,
/// runs `$body` with it bound to `$store`, then closes it, whatever the outcome.
macro_rules! with_store {
    ($server:ident.reader(), |$store:ident| $body:expr) => {
        with_store!(@open open_for_reading, $server, $store, $body)
    };
    ($server:ident.writer(), |$store:ident| $body:expr) => {
        with_store!(@open open_for_writing, $server, $store, $body)
    };
    (@open $open:ident, $server:ident, $store:ident, $body:expr) => {
        async {
            let $store = $open($server.database()?).await?;
            let outcome: Result<Value, AppError> = async { $body }.await;
            $store.close().await;
            outcome
        }
        .await
    };
}

fn usage(message: impl Into<String>) -> AppError {
    AppError::rejected("usage", message)
}

fn bounded<'a>(field: &str, value: &'a str, max: usize) -> Result<&'a str, AppError> {
    if value.chars().count() > max {
        return Err(usage(format!("{field} admite hasta {max} caracteres")));
    }
    Ok(value)
}

fn optional<'a>(field: &str, value: &'a Option<String>, max: usize) -> Result<&'a str, AppError> {
    bounded(field, value.as_deref().unwrap_or_default(), max)
}

fn id(field: &str, value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(bounded(field, value, ID_MAX)?)
        .map_err(|_| usage(format!("{field} no es un identificador válido")))
}

fn limit(value: Option<u32>, default: u32, max: u32) -> Result<usize, AppError> {
    let value = value.unwrap_or(default);
    if value == 0 || value > max {
        return Err(usage(format!("limit va de 1 a {max}")));
    }
    Ok(value as usize)
}

/// Storage failures get a fixed message per code, because their own messages
/// name files and carry driver details.
fn storage_message(code: &str) -> &'static str {
    match code {
        "database_missing" => {
            "No hay una base de datos autorizada: indica --db o COMINORSA_DB_PATH con una base creada con cominorsa init"
        }
        "schema_outdated" => {
            "La base está en una versión anterior. Actualízala con: cominorsa migrate"
        }
        "schema_newer" => "La base es más nueva que este programa. Actualiza el programa",
        "corrupt_record" => "Un registro guardado está dañado",
        "database_aside" => {
            "Una actualización interrumpida dejó la base apartada; recupérala con cominorsa status"
        }
        "database_busy" => "Otro programa tiene abierta la base",
        _ => "No se pudo leer o guardar la base de datos",
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListInput {
    /// Filters by name, phone, RUC, or service
    #[schemars(length(max = 200))]
    pub query: Option<String>,
    /// At most this many rows, 1 to 1000; 200 by default
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LimitInput {
    /// At most this many rows, 1 to 1000; 200 by default
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProspectSearchInput {
    /// Text to search; empty lists the first prospects
    #[schemars(length(max = 200))]
    pub query: Option<String>,
    /// At most this many rows, 1 to 500; 100 by default
    #[schemars(range(min = 1, max = 500))]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdInput {
    /// Record id (UUID)
    #[schemars(length(max = 64))]
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuoteListInput {
    /// Only the quotes of this lead id
    #[schemars(length(max = 64))]
    pub lead: Option<String>,
    /// At most this many rows, 1 to 1000; 200 by default
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuoteInput {
    /// Quote number, such as COT-2026-0001-V2, or id
    #[schemars(length(max = 64))]
    pub quote: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeadAddInput {
    #[schemars(length(max = 200))]
    pub name: String,
    #[schemars(length(max = 40))]
    pub phone: String,
    /// Requested service, such as DAC or REINFO
    #[schemars(length(max = 200))]
    pub service: String,
    #[schemars(length(max = 20))]
    pub ruc: Option<String>,
    /// A team member's name
    #[schemars(length(max = 100))]
    pub owner: Option<String>,
    /// Next follow-up, YYYY-MM-DD; by default from settings
    #[schemars(length(max = 32))]
    pub follow_up: Option<String>,
    /// web_form, inbound_whatsapp, inbound_call, referral, event, existing_client, prospect, or
    /// other (default). The first three record the client's consent
    #[schemars(length(max = 32))]
    pub origin: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignInput {
    /// Lead id (UUID)
    #[schemars(length(max = 64))]
    pub id: String,
    /// A team member's name; blank clears the owner
    #[schemars(length(max = 100))]
    pub owner: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsentInput {
    /// Lead id (UUID)
    #[schemars(length(max = 64))]
    pub id: String,
    /// How the client decided, such as "Aceptó por llamada"
    #[schemars(length(max = 300))]
    pub source: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepDateInput {
    /// Step id (UUID)
    #[schemars(length(max = 64))]
    pub id: String,
    /// New date, YYYY-MM-DD
    #[schemars(length(max = 32))]
    pub date: String,
}

/// The MCP server. `db` comes only from `--db` or `COMINORSA_DB_PATH`.
#[derive(Clone)]
pub struct CrmServer {
    db: Option<PathBuf>,
    tools: ToolRouter<Self>,
}

impl CrmServer {
    /// Write tools are registered only when `allow_writes` is true.
    pub fn new(db: Option<PathBuf>, allow_writes: bool) -> Self {
        let mut tools = Self::read_tools();
        if allow_writes {
            tools += Self::write_tools();
        }
        Self { db, tools }
    }

    fn database(&self) -> Result<&Path, AppError> {
        self.db.as_deref().ok_or_else(|| {
            AppError::storage("database_missing", storage_message("database_missing"))
        })
    }

    /// A rejection keeps its message unless it names the database location.
    fn message(&self, error: &AppError) -> String {
        if error.kind() == ErrorKind::Storage {
            return storage_message(error.code()).to_owned();
        }
        let message = error.to_string();
        let names_location = self.db.as_deref().is_some_and(|db| {
            [Some(db), db.parent()]
                .into_iter()
                .flatten()
                .map(|path| path.display().to_string())
                .any(|path| !path.is_empty() && message.contains(&path))
        });
        if names_location {
            "Solicitud rechazada".to_owned()
        } else {
            message
        }
    }

    fn reply(&self, outcome: Result<Value, AppError>) -> Result<CallToolResult, ErrorData> {
        Ok(match outcome {
            Ok(data) => CallToolResult::structured(json!({"data": data})),
            Err(error) => CallToolResult::structured_error(json!({
                "error": {"code": error.code(), "message": self.message(&error)},
            })),
        })
    }
}

#[tool_router(router = read_tools)]
impl CrmServer {
    #[tool(
        description = "Schema version, integrity check, and record counts, like `cominorsa status`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn status(&self) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| status(&store)
            .await
            .map(|found| views::status(&found)));
        self.reply(outcome)
    }

    #[tool(
        description = "Overdue, due today, and upcoming follow-ups, case steps, and collections, like `cominorsa agenda`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn agenda(&self) -> Result<CallToolResult, ErrorData> {
        let today = Context::system().clock.today();
        let outcome = with_store!(self.reader(), |store| agenda(&store, today)
            .await
            .map(|groups| views::agenda(&groups, today)));
        self.reply(outcome)
    }

    #[tool(
        description = "Leads with their follow-up status for today, like `cominorsa lead list`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn lead_list(
        &self,
        Parameters(input): Parameters<ListInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = Context::system().clock.today();
        let outcome = with_store!(self.reader(), |store| {
            let query = optional("query", &input.query, QUERY_MAX)?;
            let limit = limit(input.limit, LIST_LIMIT, LIST_LIMIT_MAX)?;
            let found = search_leads(&store, query).await?;
            let leads: Vec<Value> = found
                .iter()
                .take(limit)
                .map(|lead| views::listed_lead(lead, today))
                .collect();
            Ok(json!({"leads": leads}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "One lead and its follow-up status, like `cominorsa lead show`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn lead_show(
        &self,
        Parameters(input): Parameters<IdInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = Context::system().clock.today();
        let outcome = with_store!(self.reader(), |store| {
            let lead = find_lead(&store, id("id", &input.id)?).await?;
            let follow_up = FollowUp::of(&lead, today);
            Ok(json!({
                "lead": views::lead(&lead),
                "follow_up": views::follow_up(&follow_up),
            }))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Searches prospects, like `cominorsa prospect search`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn prospect_search(
        &self,
        Parameters(input): Parameters<ProspectSearchInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let query = optional("query", &input.query, QUERY_MAX)?;
            let limit = limit(input.limit, PROSPECT_PAGE_SIZE, PROSPECT_LIMIT_MAX)?;
            let found = store.search(query, limit as u32).await?;
            let prospects: Vec<Value> = found.iter().map(views::prospect).collect();
            Ok(json!({"prospects": prospects}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Counts prospects, like `cominorsa prospect count`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn prospect_count(&self) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let total = store.count().await?;
            Ok(json!({"total": total}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Open cases and closed cases with pending collections, like `cominorsa case list`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn case_list(
        &self,
        Parameters(input): Parameters<LimitInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let limit = limit(input.limit, LIST_LIMIT, LIST_LIMIT_MAX)?;
            let cases = store.active_cases().await?;
            let cases: Vec<Value> = cases.iter().take(limit).map(views::case).collect();
            Ok(json!({"cases": cases}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "One case with its steps, milestones, and collection summary, like `cominorsa case show`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn case_show(
        &self,
        Parameters(input): Parameters<IdInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let file = find_stored_case(&store, id("id", &input.id)?).await?;
            let quotes = store.quotes().await?;
            let summary = BillingSummary::of(&file);
            Ok(json!({
                "case": views::case(&file),
                "billing": {
                    "total_cents": summary.total.map(|total| total.cents()),
                    "collected_cents": summary.collected.cents(),
                    "pending_cents": summary.pending.cents(),
                    "gap_warning": milestone_gap_warning(&file),
                    "total_note": suggested_total_note(&file, &quotes),
                    "close_warning": close_warning(&file),
                },
            }))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Quotes, optionally of one lead, like `cominorsa quote list`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn quote_list(
        &self,
        Parameters(input): Parameters<QuoteListInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let lead = input
                .lead
                .as_deref()
                .map(|lead| id("lead", lead))
                .transpose()?;
            let limit = limit(input.limit, LIST_LIMIT, LIST_LIMIT_MAX)?;
            let quotes = store.quotes().await?;
            let found = match lead {
                Some(lead) => lead_quotes(&quotes, lead),
                None => quotes,
            };
            let quotes: Vec<Value> = found.iter().take(limit).map(views::quote).collect();
            Ok(json!({"quotes": quotes}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "One quote by number or id, like `cominorsa quote show`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn quote_show(
        &self,
        Parameters(input): Parameters<QuoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let quote = find_quote(&store, bounded("quote", &input.quote, ID_MAX)?).await?;
            Ok(json!({"quote": views::quote(&quote)}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Settings, like `cominorsa settings show`, except that the backup folder is reported only as `backup_directory_set`",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn settings_show(&self) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.reader(), |store| {
            let saved = store.load().await?;
            Ok(views::settings(&saved))
        });
        self.reply(outcome)
    }
}

#[tool_router(router = write_tools)]
impl CrmServer {
    #[tool(
        description = "Registers a lead, like `cominorsa lead add`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn lead_add(
        &self,
        Parameters(input): Parameters<LeadAddInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut context = Context::system();
        let outcome = with_store!(self.writer(), |store| {
            let follow_up = optional("follow_up", &input.follow_up, CODE_MAX)?;
            let values = FormValues {
                name: bounded("name", &input.name, NAME_MAX)?.to_owned(),
                phone: bounded("phone", &input.phone, PHONE_MAX)?.to_owned(),
                ruc: optional("ruc", &input.ruc, RUC_MAX)?.to_owned(),
                service: bounded("service", &input.service, NAME_MAX)?.to_owned(),
                owner: optional("owner", &input.owner, OWNER_MAX)?.to_owned(),
                origin: optional("origin", &input.origin, CODE_MAX)?.to_owned(),
                follow_up_on: match input.follow_up {
                    Some(_) => follow_up.to_owned(),
                    None => default_follow_up(&store, context.clock.today()).await?,
                },
            };
            let added = add_lead(&store, values, &mut context).await?;
            Ok(json!({"created": added.created, "lead": views::lead(&added.lead)}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Changes the responsible team member of a lead, like `cominorsa lead assign`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn lead_assign(
        &self,
        Parameters(input): Parameters<AssignInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.writer(), |store| {
            let lead = id("id", &input.id)?;
            let owner = bounded("owner", &input.owner, OWNER_MAX)?;
            let lead = assign_lead(&store, lead, owner).await?;
            Ok(json!({"lead": views::lead(&lead)}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Records today that the client agreed to be contacted, like `cominorsa lead consent`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn lead_consent(
        &self,
        Parameters(input): Parameters<ConsentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = Context::system().clock.today();
        let outcome = with_store!(self.writer(), |store| {
            let lead = id("id", &input.id)?;
            let source = bounded("source", &input.source, SOURCE_MAX)?;
            let lead = record_consent(&store, lead, source, today).await?;
            Ok(json!({"lead": views::lead(&lead)}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Records today that the client withdrew consent, like `cominorsa lead revoke-consent`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn lead_revoke_consent(
        &self,
        Parameters(input): Parameters<ConsentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = Context::system().clock.today();
        let outcome = with_store!(self.writer(), |store| {
            let lead = id("id", &input.id)?;
            let source = bounded("source", &input.source, SOURCE_MAX)?;
            let lead = revoke_consent(&store, lead, source, today).await?;
            Ok(json!({"lead": views::lead(&lead)}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Marks a case step done today, like `cominorsa step done`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn step_done(
        &self,
        Parameters(input): Parameters<IdInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = Context::system().clock.today();
        let outcome = with_store!(self.writer(), |store| {
            let changed = set_step_done(&store, id("id", &input.id)?, true, today).await?;
            Ok(json!({"changed": changed}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Changes the date of a case step, like `cominorsa step date`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn step_date(
        &self,
        Parameters(input): Parameters<StepDateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = with_store!(self.writer(), |store| {
            let step = id("id", &input.id)?;
            let date = bounded("date", &input.date, CODE_MAX)?;
            let step = reschedule_step(&store, step, date).await?;
            Ok(json!({"step": views::step(&step)}))
        });
        self.reply(outcome)
    }

    #[tool(
        description = "Marks a billing milestone paid today, like `cominorsa billing pay`",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn milestone_pay(
        &self,
        Parameters(input): Parameters<IdInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut context = Context::system();
        let outcome = with_store!(self.writer(), |store| {
            let changed = set_paid(&store, id("id", &input.id)?, true, &mut context).await?;
            Ok(json!({"changed": changed}))
        });
        self.reply(outcome)
    }
}

#[tool_handler(router = self.tools)]
impl ServerHandler for CrmServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "cominorsa-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}
