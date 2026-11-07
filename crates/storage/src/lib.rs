//! SQLite adapter for the domain repository port.

use std::{collections::HashMap, fs, path::Path};

use cominorsa_domain::{
    BillingMilestone, BillingMilestoneRecord, BillingRepository, Case, CaseClosure, CaseFile,
    CaseRecord, CaseRepository, CaseStep, CaseStepRecord, CompanyProfile, Lead, LeadRecord,
    LeadRepository, Money, Prospect, ProspectKind, ProspectRepository, Quote, QuoteDraft,
    QuoteLineRecord, QuoteNumber, QuoteRecord, QuoteRepository, QuoteStatus, RepositoryError,
    Settings, SettingsRepository, TaxRecord, ThemePreference,
};
use sqlx::{
    Row, Sqlite, SqliteExecutor, SqlitePool, Transaction,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

const UPSERT: &str = "INSERT INTO leads (id, name, phone, ruc, service, stage, follow_up_on, owner,
        renewal_of_case)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
    ON CONFLICT (id) DO UPDATE SET
        name = excluded.name,
        phone = excluded.phone,
        ruc = excluded.ruc,
        service = excluded.service,
        stage = excluded.stage,
        follow_up_on = excluded.follow_up_on,
        owner = excluded.owner,
        renewal_of_case = excluded.renewal_of_case";

const SELECT_ALL: &str = "SELECT id, name, phone, ruc, service, stage, follow_up_on, owner,
        renewal_of_case
    FROM leads ORDER BY follow_up_on, id";

const INSERT_CASE: &str = "INSERT INTO cases (id, lead_id, client_name, ruc, service, owner,
        opened_on, status, contract_total_cents)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
    ON CONFLICT (lead_id) DO NOTHING";

const INSERT_CASE_STEP: &str = "INSERT INTO case_steps (id, case_id, position, title, due_on,
        done_on)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

const UPDATE_CASE_STEP: &str = "UPDATE case_steps SET due_on = ?2, done_on = ?3 WHERE id = ?1";

const SELECT_OPEN_CASES: &str = "SELECT id, lead_id, client_name, ruc, service, owner, opened_on,
        status, contract_total_cents
    FROM cases WHERE status = 'open' ORDER BY opened_on, id";

const SELECT_OPEN_CASE_STEPS: &str = "SELECT step.id, step.case_id, step.position, step.title,
        step.due_on, step.done_on
    FROM case_steps AS step JOIN cases AS parent ON parent.id = step.case_id
    WHERE parent.status = 'open' ORDER BY step.case_id, step.position";

const SELECT_ACTIVE_CASES: &str = "SELECT id, lead_id, client_name, ruc, service, owner, opened_on,
        status, contract_total_cents
    FROM cases
    WHERE status = 'open' OR EXISTS (SELECT 1 FROM billing_milestones AS item
        WHERE item.case_id = cases.id AND item.paid_on IS NULL)
    ORDER BY opened_on, id";

const SELECT_ACTIVE_CASE_STEPS: &str = "SELECT step.id, step.case_id, step.position, step.title,
        step.due_on, step.done_on
    FROM case_steps AS step JOIN cases AS parent ON parent.id = step.case_id
    WHERE parent.status = 'open' OR EXISTS (SELECT 1 FROM billing_milestones AS item
        WHERE item.case_id = parent.id AND item.paid_on IS NULL)
    ORDER BY step.case_id, step.position";

const SELECT_ALL_CASES: &str = "SELECT id, lead_id, client_name, ruc, service, owner, opened_on,
        status, contract_total_cents
    FROM cases ORDER BY opened_on, id";

const SELECT_ALL_CASE_STEPS: &str = "SELECT id, case_id, position, title, due_on, done_on
    FROM case_steps ORDER BY case_id, position";

const CLOSE_CASE: &str = "UPDATE cases SET status = ?2 WHERE id = ?1 AND status = 'open'";

const UPDATE_CONTRACT_TOTAL: &str = "UPDATE cases SET contract_total_cents = ?2 WHERE id = ?1";

const SELECT_MILESTONES: &str = "SELECT id, case_id, position, title, amount_cents, due_on, paid_on
    FROM billing_milestones ORDER BY case_id, position";

const INSERT_MILESTONE: &str = "INSERT INTO billing_milestones (id, case_id, position, title,
        amount_cents, due_on, paid_on)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

const UPDATE_MILESTONE: &str = "UPDATE billing_milestones
    SET title = ?2, amount_cents = ?3, due_on = ?4, paid_on = ?5 WHERE id = ?1";

const INSERT_PROSPECT: &str = "INSERT INTO prospects (kind, name, profile, concessions, hectares,
        departments, provinces, substances, concession_states, potential_service, needs_review,
        source)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
    ON CONFLICT (kind, name) DO NOTHING";

const APPLY_TAX_RECORD: &str = "UPDATE prospects SET ruc = ?2, tax_status = ?3,
        tax_condition = ?4, fiscal_address = ?5, tax_match_approximate = ?6
    WHERE kind = 'company' AND name = ?1";

const SEARCH_PROSPECTS: &str = "SELECT kind, name, profile, concessions, hectares, departments,
        provinces, substances, concession_states, potential_service, needs_review, source,
        ruc, tax_status, tax_condition, fiscal_address, tax_match_approximate
    FROM prospects
    WHERE name LIKE ?1 ESCAPE '\\' OR name LIKE ?3 ESCAPE '\\'
        OR departments LIKE ?1 ESCAPE '\\' OR departments LIKE ?3 ESCAPE '\\'
        OR provinces LIKE ?1 ESCAPE '\\' OR provinces LIKE ?3 ESCAPE '\\'
        OR potential_service LIKE ?1 ESCAPE '\\' OR potential_service LIKE ?3 ESCAPE '\\'
        OR ruc LIKE ?1 ESCAPE '\\'
    ORDER BY needs_review, concessions DESC, name
    LIMIT ?2";

const NEXT_QUOTE_SEQUENCE: &str = "INSERT INTO quote_numbers (year, last_sequence) VALUES (?1, 1)
    ON CONFLICT (year) DO UPDATE SET last_sequence = last_sequence + 1
    RETURNING last_sequence";

const FIND_QUOTE_REPEAT: &str = "SELECT id FROM quotes WHERE id = ?1 OR supersedes = ?2";

const INSERT_QUOTE: &str = "INSERT INTO quotes (id, lead_id, year, sequence, version, supersedes,
        client_name, client_ruc, service, issued_on, valid_until, notes, status,
        igv_basis_points, company_name, company_ruc, company_address, company_phone,
        company_email)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)";

const INSERT_QUOTE_LINE: &str =
    "INSERT INTO quote_lines (quote_id, position, description, quantity,
        unit_price_cents)
    VALUES (?1, ?2, ?3, ?4, ?5)";

const UPDATE_QUOTE_DRAFT: &str = "UPDATE quotes
    SET issued_on = ?2, valid_until = ?3, notes = ?4, igv_basis_points = ?5
    WHERE id = ?1 AND status = 'draft'";

const UPDATE_QUOTE_STATUS: &str = "UPDATE quotes
    SET status = ?3, company_name = ?4, company_ruc = ?5, company_address = ?6,
        company_phone = ?7, company_email = ?8
    WHERE id = ?1 AND status = ?2";

const SELECT_QUOTES: &str = "SELECT id, lead_id, year, sequence, version, supersedes, client_name,
        client_ruc, service, issued_on, valid_until, notes, status, igv_basis_points,
        company_name, company_ruc, company_address, company_phone, company_email
    FROM quotes ORDER BY year, sequence, version";

const SELECT_QUOTE_LINES: &str = "SELECT quote_id, description, quantity, unit_price_cents
    FROM quote_lines ORDER BY quote_id, position";

const SELECT_QUOTE_COUNTERS: &str = "SELECT year, last_sequence FROM quote_numbers ORDER BY year";

const SELECT_SCHEMA_VERSION: &str = "SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1";

const CRM_TABLES: i64 = 2;
const CRM_TABLE_COUNT: &str = "SELECT COUNT(*) FROM sqlite_master
    WHERE type = 'table' AND name IN ('leads', '_sqlx_migrations')";

const THEME_KEY: &str = "theme";
const FOLLOW_UP_DAYS_KEY: &str = "follow_up_days";
const WHATSAPP_TEMPLATE_KEY: &str = "whatsapp_template";
const BACKUP_DIRECTORY_KEY: &str = "backup_directory";
const LAST_BACKUP_AT_KEY: &str = "last_backup_at";
const IGV_RATE_KEY: &str = "igv_basis_points";
const COMPANY_NAME_KEY: &str = "company_name";
const COMPANY_RUC_KEY: &str = "company_ruc";
const COMPANY_ADDRESS_KEY: &str = "company_address";
const COMPANY_PHONE_KEY: &str = "company_phone";
const COMPANY_EMAIL_KEY: &str = "company_email";
const UPSERT_SETTING: &str = "INSERT INTO settings (key, value) VALUES (?1, ?2)
    ON CONFLICT (key) DO UPDATE SET value = excluded.value";

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// The last quote number given in one year.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteCounter {
    pub year: i64,
    pub last_sequence: i64,
}

#[derive(Clone)]
pub struct SqliteLeads {
    pool: SqlitePool,
}

impl SqliteLeads {
    /// Opens the database file, creating it and applying pending migrations.
    pub async fn open(path: &Path) -> Result<Self, RepositoryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(failure)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(failure)?;

        MIGRATOR.run(&pool).await.map_err(failure)?;

        Ok(Self { pool })
    }

    /// Opens an existing database without migrating it. Writes through this
    /// store fail, and a missing file is an error, never a new database.
    pub async fn open_read_only(path: &Path) -> Result<Self, RepositoryError> {
        let options = SqliteConnectOptions::new().filename(path).read_only(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(failure)?;

        Ok(Self { pool })
    }

    /// Opens an existing database to write, without creating or migrating it.
    /// The caller has already checked its schema version.
    pub async fn open_existing(path: &Path) -> Result<Self, RepositoryError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(failure)?;

        Ok(Self { pool })
    }

    /// Migrates a working copy of `safety_copy` and only then puts it in place
    /// of `database` with a rename, so a failed migration leaves `database` as
    /// it was. Nothing may be writing to `database` meanwhile.
    pub async fn upgrade(database: &Path, safety_copy: &Path) -> Result<(), RepositoryError> {
        let working = database.with_extension("migrating.db");
        remove_with_sidecars(&working);

        let migrated = async {
            fs::copy(safety_copy, &working).map_err(failure)?;
            Self::open(&working).await?.close().await;
            Ok(())
        }
        .await;
        if let Err(error) = migrated {
            remove_with_sidecars(&working);
            return Err(error);
        }

        // A stale WAL would replay over the new file. If the rename fails, the
        // safety copy still holds what that WAL held.
        remove_sidecars(database);
        fs::rename(&working, database).map_err(failure)
    }

    /// The schema version this build of the adapter works with.
    pub fn required_schema_version() -> i64 {
        MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap_or(0)
    }

    /// The last migration applied to this database.
    pub async fn schema_version(&self) -> Result<i64, RepositoryError> {
        let version: Option<i64> = sqlx::query_scalar(SELECT_SCHEMA_VERSION)
            .fetch_one(&self.pool)
            .await
            .map_err(failure)?;

        Ok(version.unwrap_or(0))
    }

    /// SQLite's own check: `ok`, or its findings, one per line.
    pub async fn integrity_check(&self) -> Result<String, RepositoryError> {
        let findings: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        Ok(findings.join("\n"))
    }

    /// Every case, including closed ones with nothing left to collect.
    pub async fn all_cases(&self) -> Result<Vec<CaseFile>, RepositoryError> {
        self.load_cases(SELECT_ALL_CASES, SELECT_ALL_CASE_STEPS)
            .await
    }

    /// The numbering counter of each year, oldest year first.
    pub async fn quote_counters(&self) -> Result<Vec<QuoteCounter>, RepositoryError> {
        let rows = sqlx::query(SELECT_QUOTE_COUNTERS)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        rows.iter()
            .map(|row| {
                Ok(QuoteCounter {
                    year: row.try_get("year").map_err(failure)?,
                    last_sequence: row.try_get("last_sequence").map_err(failure)?,
                })
            })
            .collect()
    }

    /// Writes a consistent copy of the database to a path that must not exist.
    pub async fn backup_to(&self, path: &Path) -> Result<(), RepositoryError> {
        sqlx::query("VACUUM INTO ?1")
            .bind(path.to_string_lossy().into_owned())
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        Ok(())
    }

    /// Replaces the database with a backup and returns the reopened store.
    ///
    /// The backup is validated on a working copy, so an invalid file changes
    /// nothing. Current data is copied to `safety` before it is replaced.
    pub async fn restore(
        self,
        database: &Path,
        backup: &Path,
        safety: &Path,
    ) -> Result<Self, RepositoryError> {
        let working = database.with_extension("restoring.db");

        if let Err(error) = validate_backup(backup, &working).await {
            remove_with_sidecars(&working);
            return Err(error);
        }

        if let Err(error) = self.backup_to(safety).await {
            remove_with_sidecars(&working);
            return Err(error);
        }

        self.close().await;
        remove_sidecars(database);
        fs::rename(&working, database).map_err(|error| {
            failure(format!(
                "{error}. Los datos anteriores están en {}",
                safety.display()
            ))
        })?;

        Self::open(database).await
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn load_cases(
        &self,
        select_cases: &'static str,
        select_steps: &'static str,
    ) -> Result<Vec<CaseFile>, RepositoryError> {
        let cases = sqlx::query(select_cases)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        let step_rows = sqlx::query(select_steps)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        let milestone_rows = sqlx::query(SELECT_MILESTONES)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        let mut steps: HashMap<Uuid, Vec<CaseStep>> = HashMap::new();
        for row in &step_rows {
            let step = to_case_step(row)?;
            steps.entry(step.case_id()).or_default().push(step);
        }
        let mut milestones: HashMap<Uuid, Vec<BillingMilestone>> = HashMap::new();
        for row in &milestone_rows {
            let milestone = to_milestone(row)?;
            milestones
                .entry(milestone.case_id())
                .or_default()
                .push(milestone);
        }

        cases
            .iter()
            .map(|row| {
                let case = to_case(row)?;
                let steps = steps.remove(&case.id()).unwrap_or_default();
                let milestones = milestones.remove(&case.id()).unwrap_or_default();
                Ok(CaseFile {
                    case,
                    steps,
                    milestones,
                })
            })
            .collect()
    }
}

/// `open` would create the schema in any file, so the CRM tables must already
/// exist before migrations run.
async fn ensure_crm_schema(path: &Path) -> Result<(), RepositoryError> {
    let options = SqliteConnectOptions::new().filename(path);
    let pool = SqlitePool::connect_with(options).await.map_err(failure)?;
    let found = sqlx::query_scalar::<_, i64>(CRM_TABLE_COUNT)
        .fetch_one(&pool)
        .await;
    pool.close().await;

    match found.map_err(failure)? {
        CRM_TABLES => Ok(()),
        _ => Err(failure("El archivo no es una copia de COMINORSA CRM")),
    }
}

async fn validate_backup(backup: &Path, working: &Path) -> Result<(), RepositoryError> {
    fs::copy(backup, working).map_err(failure)?;
    ensure_crm_schema(working).await?;

    let store = SqliteLeads::open(working).await?;
    let checked = async {
        store.list().await?;
        store.count().await?;
        store.active_cases().await?;
        store.quotes().await?;
        store.load().await
    }
    .await;
    store.close().await;

    checked.map(|_| ())
}

fn remove_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = fs::remove_file(name);
    }
}

fn remove_with_sidecars(path: &Path) {
    let _ = fs::remove_file(path);
    remove_sidecars(path);
}

async fn upsert_lead<'e>(
    executor: impl SqliteExecutor<'e>,
    lead: &Lead,
) -> Result<(), RepositoryError> {
    sqlx::query(UPSERT)
        .bind(lead.id().to_string())
        .bind(lead.name())
        .bind(lead.phone())
        .bind(lead.ruc().map(|ruc| ruc.as_str()))
        .bind(lead.service())
        .bind(lead.stage().code())
        .bind(lead.follow_up_on().to_string())
        .bind(lead.owner())
        .bind(lead.renewal_of().map(|case| case.to_string()))
        .execute(executor)
        .await
        .map_err(failure)?;

    Ok(())
}

impl LeadRepository for SqliteLeads {
    async fn save(&self, lead: &Lead) -> Result<(), RepositoryError> {
        upsert_lead(&self.pool, lead).await
    }

    async fn list(&self) -> Result<Vec<Lead>, RepositoryError> {
        let rows = sqlx::query(SELECT_ALL)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        rows.iter().map(to_lead).collect()
    }
}

impl CaseRepository for SqliteLeads {
    async fn sign_and_open(
        &self,
        lead: &Lead,
        opening: &CaseFile,
    ) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        upsert_lead(&mut *transaction, lead).await?;

        let case = &opening.case;
        let inserted = sqlx::query(INSERT_CASE)
            .bind(case.id().to_string())
            .bind(case.lead_id().to_string())
            .bind(case.client_name())
            .bind(case.ruc().map(|ruc| ruc.as_str()))
            .bind(case.service())
            .bind(case.owner())
            .bind(case.opened_on().to_string())
            .bind(case.status().code())
            .bind(case.contract_total().map(Money::cents))
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;
        let opened = inserted.rows_affected() == 1;

        if opened {
            for step in &opening.steps {
                sqlx::query(INSERT_CASE_STEP)
                    .bind(step.id().to_string())
                    .bind(step.case_id().to_string())
                    .bind(i64::from(step.position()))
                    .bind(step.title())
                    .bind(step.due_on().to_string())
                    .bind(step.done_on().map(|date| date.to_string()))
                    .execute(&mut *transaction)
                    .await
                    .map_err(failure)?;
            }
        }

        transaction.commit().await.map_err(failure)?;
        Ok(opened)
    }

    async fn close_case(&self, closure: &CaseClosure) -> Result<bool, RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let closed = sqlx::query(CLOSE_CASE)
            .bind(closure.case.id().to_string())
            .bind(closure.case.status().code())
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;

        if closed.rows_affected() == 0 {
            return Ok(false);
        }
        if let Some(renewal) = &closure.renewal {
            upsert_lead(&mut *transaction, renewal).await?;
        }

        transaction.commit().await.map_err(failure)?;
        Ok(true)
    }

    async fn open_cases(&self) -> Result<Vec<CaseFile>, RepositoryError> {
        self.load_cases(SELECT_OPEN_CASES, SELECT_OPEN_CASE_STEPS)
            .await
    }

    async fn active_cases(&self) -> Result<Vec<CaseFile>, RepositoryError> {
        self.load_cases(SELECT_ACTIVE_CASES, SELECT_ACTIVE_CASE_STEPS)
            .await
    }

    async fn save_step(&self, step: &CaseStep) -> Result<(), RepositoryError> {
        let updated = sqlx::query(UPDATE_CASE_STEP)
            .bind(step.id().to_string())
            .bind(step.due_on().to_string())
            .bind(step.done_on().map(|date| date.to_string()))
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        match updated.rows_affected() {
            0 => Err(failure("El paso del expediente ya no existe")),
            _ => Ok(()),
        }
    }
}

impl BillingRepository for SqliteLeads {
    async fn save_contract_total(
        &self,
        case_id: Uuid,
        total: Money,
    ) -> Result<(), RepositoryError> {
        let updated = sqlx::query(UPDATE_CONTRACT_TOTAL)
            .bind(case_id.to_string())
            .bind(total.cents())
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        match updated.rows_affected() {
            0 => Err(failure("El expediente ya no existe")),
            _ => Ok(()),
        }
    }

    async fn add_milestones(&self, milestones: &[BillingMilestone]) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;

        for milestone in milestones {
            sqlx::query(INSERT_MILESTONE)
                .bind(milestone.id().to_string())
                .bind(milestone.case_id().to_string())
                .bind(i64::from(milestone.position()))
                .bind(milestone.title())
                .bind(milestone.amount().cents())
                .bind(milestone.due_on().to_string())
                .bind(milestone.paid_on().map(|date| date.to_string()))
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
        }

        transaction.commit().await.map_err(failure)
    }

    async fn save_milestone(&self, milestone: &BillingMilestone) -> Result<(), RepositoryError> {
        let updated = sqlx::query(UPDATE_MILESTONE)
            .bind(milestone.id().to_string())
            .bind(milestone.title())
            .bind(milestone.amount().cents())
            .bind(milestone.due_on().to_string())
            .bind(milestone.paid_on().map(|date| date.to_string()))
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        match updated.rows_affected() {
            0 => Err(failure("El hito de cobro ya no existe")),
            _ => Ok(()),
        }
    }

    async fn delete_milestone(&self, id: Uuid) -> Result<(), RepositoryError> {
        let deleted = sqlx::query("DELETE FROM billing_milestones WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        match deleted.rows_affected() {
            0 => Err(failure("El hito de cobro ya no existe")),
            _ => Ok(()),
        }
    }
}

async fn insert_quote_lines(
    transaction: &mut Transaction<'_, Sqlite>,
    quote: &Quote,
) -> Result<(), RepositoryError> {
    for (position, line) in (1_i64..).zip(quote.lines()) {
        sqlx::query(INSERT_QUOTE_LINE)
            .bind(quote.id().to_string())
            .bind(position)
            .bind(line.description())
            .bind(i64::from(line.quantity()))
            .bind(line.unit_price().cents())
            .execute(&mut **transaction)
            .await
            .map_err(failure)?;
    }

    Ok(())
}

impl QuoteRepository for SqliteLeads {
    async fn create_quote(&self, draft: &QuoteDraft) -> Result<Quote, RepositoryError> {
        // The write lock is taken first, so no other writer can read the same
        // counter, even from a second process.
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(failure)?;
        let repeated: Option<String> = sqlx::query_scalar(FIND_QUOTE_REPEAT)
            .bind(draft.id().to_string())
            .bind(draft.revised_quote().map(|id| id.to_string()))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(failure)?;

        if let Some(id) = repeated {
            transaction.rollback().await.map_err(failure)?;
            let stored = self.quotes().await?;
            return stored
                .into_iter()
                .find(|quote| quote.id().to_string() == id)
                .ok_or_else(|| failure("La cotización ya no existe"));
        }

        let number = match draft.next_version() {
            Some(number) => number,
            None => {
                let sequence: i64 = sqlx::query_scalar(NEXT_QUOTE_SEQUENCE)
                    .bind(draft.year())
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(failure)?;
                let sequence = u32::try_from(sequence).map_err(failure)?;
                QuoteNumber::new(draft.year(), sequence, 1).map_err(failure)?
            }
        };
        let quote = draft.clone().numbered(number);
        let company = quote.company();

        sqlx::query(INSERT_QUOTE)
            .bind(quote.id().to_string())
            .bind(quote.lead_id().to_string())
            .bind(number.year())
            .bind(i64::from(number.sequence()))
            .bind(i64::from(number.version()))
            .bind(quote.supersedes().map(|id| id.to_string()))
            .bind(quote.client_name())
            .bind(quote.client_ruc().map(|ruc| ruc.as_str()))
            .bind(quote.service())
            .bind(quote.issued_on().to_string())
            .bind(quote.valid_until().to_string())
            .bind(quote.notes())
            .bind(quote.status().code())
            .bind(i64::from(quote.igv_rate().basis_points()))
            .bind(&company.name)
            .bind(&company.ruc)
            .bind(&company.address)
            .bind(&company.phone)
            .bind(&company.email)
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;
        insert_quote_lines(&mut transaction, &quote).await?;

        transaction.commit().await.map_err(failure)?;
        Ok(quote)
    }

    async fn save_quote_draft(&self, quote: &Quote) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let updated = sqlx::query(UPDATE_QUOTE_DRAFT)
            .bind(quote.id().to_string())
            .bind(quote.issued_on().to_string())
            .bind(quote.valid_until().to_string())
            .bind(quote.notes())
            .bind(i64::from(quote.igv_rate().basis_points()))
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;

        if updated.rows_affected() == 0 {
            return Err(failure("La cotización ya no es un borrador"));
        }

        sqlx::query("DELETE FROM quote_lines WHERE quote_id = ?1")
            .bind(quote.id().to_string())
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;
        insert_quote_lines(&mut transaction, quote).await?;

        transaction.commit().await.map_err(failure)
    }

    async fn save_quote_status(
        &self,
        quote: &Quote,
        from: QuoteStatus,
    ) -> Result<(), RepositoryError> {
        let company = quote.company();
        let updated = sqlx::query(UPDATE_QUOTE_STATUS)
            .bind(quote.id().to_string())
            .bind(from.code())
            .bind(quote.status().code())
            .bind(&company.name)
            .bind(&company.ruc)
            .bind(&company.address)
            .bind(&company.phone)
            .bind(&company.email)
            .execute(&self.pool)
            .await
            .map_err(failure)?;

        match updated.rows_affected() {
            0 => Err(failure("La cotización cambió de estado; revisa la lista")),
            _ => Ok(()),
        }
    }

    async fn quotes(&self) -> Result<Vec<Quote>, RepositoryError> {
        let rows = sqlx::query(SELECT_QUOTES)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        let line_rows = sqlx::query(SELECT_QUOTE_LINES)
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        let mut lines: HashMap<String, Vec<QuoteLineRecord>> = HashMap::new();
        for row in &line_rows {
            let quote_id: String = row.try_get("quote_id").map_err(failure)?;
            lines.entry(quote_id).or_default().push(QuoteLineRecord {
                description: row.try_get("description").map_err(failure)?,
                quantity: row.try_get("quantity").map_err(failure)?,
                unit_price_cents: row.try_get("unit_price_cents").map_err(failure)?,
            });
        }

        rows.iter().map(|row| to_quote(row, &mut lines)).collect()
    }
}

impl ProspectRepository for SqliteLeads {
    async fn import(&self, prospects: &[Prospect]) -> Result<usize, RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let mut added = 0;

        for prospect in prospects {
            let result = sqlx::query(INSERT_PROSPECT)
                .bind(prospect.kind.code())
                .bind(&prospect.name)
                .bind(&prospect.profile)
                .bind(i64::from(prospect.concessions))
                .bind(prospect.hectares)
                .bind(&prospect.departments)
                .bind(&prospect.provinces)
                .bind(&prospect.substances)
                .bind(&prospect.concession_states)
                .bind(&prospect.potential_service)
                .bind(prospect.needs_review)
                .bind(&prospect.source)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
            added += result.rows_affected() as usize;
        }

        transaction.commit().await.map_err(failure)?;
        Ok(added)
    }

    async fn apply_tax_records(
        &self,
        records: &[(String, TaxRecord)],
    ) -> Result<usize, RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;
        let mut updated = 0;

        for (name, record) in records {
            let result = sqlx::query(APPLY_TAX_RECORD)
                .bind(name)
                .bind(&record.ruc)
                .bind(&record.status)
                .bind(&record.condition)
                .bind(&record.address)
                .bind(record.approximate)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
            updated += result.rows_affected() as usize;
        }

        transaction.commit().await.map_err(failure)?;
        Ok(updated)
    }

    async fn count(&self) -> Result<usize, RepositoryError> {
        let row = sqlx::query("SELECT COUNT(*) AS total FROM prospects")
            .fetch_one(&self.pool)
            .await
            .map_err(failure)?;
        let total: i64 = row.try_get("total").map_err(failure)?;

        usize::try_from(total).map_err(failure)
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Prospect>, RepositoryError> {
        let rows = sqlx::query(SEARCH_PROSPECTS)
            .bind(like_pattern(query))
            .bind(i64::from(limit))
            // SQLite folds case for ASCII only, and source data is uppercase.
            .bind(like_pattern(&query.to_uppercase()))
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;

        rows.iter().map(to_prospect).collect()
    }
}

impl SettingsRepository for SqliteLeads {
    async fn load(&self) -> Result<Settings, RepositoryError> {
        let rows = sqlx::query("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        let mut settings = Settings::default();
        let mut company = CompanyProfile::default();

        for row in &rows {
            let key: String = row.try_get("key").map_err(failure)?;
            let value: String = row.try_get("value").map_err(failure)?;

            match key.as_str() {
                THEME_KEY => {
                    settings.theme = ThemePreference::from_code(&value)
                        .ok_or_else(|| failure("Tema guardado desconocido"))?;
                }
                FOLLOW_UP_DAYS_KEY => {
                    let days = value.parse().map_err(failure)?;
                    settings.set_follow_up_days(days).map_err(failure)?;
                }
                WHATSAPP_TEMPLATE_KEY => {
                    settings.restore_whatsapp_template(value);
                }
                BACKUP_DIRECTORY_KEY => settings.set_backup_directory(&value),
                LAST_BACKUP_AT_KEY => settings.record_backup(value.parse().map_err(failure)?),
                IGV_RATE_KEY => settings.restore_igv_rate(&value),
                COMPANY_NAME_KEY => company.name = value,
                COMPANY_RUC_KEY => company.ruc = value,
                COMPANY_ADDRESS_KEY => company.address = value,
                COMPANY_PHONE_KEY => company.phone = value,
                COMPANY_EMAIL_KEY => company.email = value,
                _ => {}
            }
        }
        settings.restore_company(company);

        let members = sqlx::query("SELECT name FROM team_members ORDER BY position")
            .fetch_all(&self.pool)
            .await
            .map_err(failure)?;
        for member in &members {
            let name: String = member.try_get("name").map_err(failure)?;
            settings.add_member(&name).map_err(failure)?;
        }

        Ok(settings)
    }

    async fn persist(&self, settings: &Settings) -> Result<(), RepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(failure)?;

        for (key, value) in [
            (THEME_KEY, settings.theme.code().to_owned()),
            (FOLLOW_UP_DAYS_KEY, settings.follow_up_days().to_string()),
            (
                WHATSAPP_TEMPLATE_KEY,
                settings.whatsapp_template().to_owned(),
            ),
            (IGV_RATE_KEY, settings.igv_rate().basis_points().to_string()),
        ] {
            sqlx::query(UPSERT_SETTING)
                .bind(key)
                .bind(value)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
        }

        let company = settings.company();
        let filled = |value: &str| (!value.is_empty()).then(|| value.to_owned());
        let optional = [
            (
                BACKUP_DIRECTORY_KEY,
                settings.backup_directory().map(str::to_owned),
            ),
            (
                LAST_BACKUP_AT_KEY,
                settings.last_backup_at().map(|at| at.to_string()),
            ),
            (COMPANY_NAME_KEY, filled(&company.name)),
            (COMPANY_RUC_KEY, filled(&company.ruc)),
            (COMPANY_ADDRESS_KEY, filled(&company.address)),
            (COMPANY_PHONE_KEY, filled(&company.phone)),
            (COMPANY_EMAIL_KEY, filled(&company.email)),
        ];
        for (key, value) in optional {
            let Some(value) = value else {
                sqlx::query("DELETE FROM settings WHERE key = ?1")
                    .bind(key)
                    .execute(&mut *transaction)
                    .await
                    .map_err(failure)?;
                continue;
            };
            sqlx::query(UPSERT_SETTING)
                .bind(key)
                .bind(value)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
        }

        sqlx::query("DELETE FROM team_members")
            .execute(&mut *transaction)
            .await
            .map_err(failure)?;
        for (position, name) in settings.team().iter().enumerate() {
            sqlx::query("INSERT INTO team_members (position, name) VALUES (?1, ?2)")
                .bind(i64::try_from(position).map_err(failure)?)
                .bind(name)
                .execute(&mut *transaction)
                .await
                .map_err(failure)?;
        }

        transaction.commit().await.map_err(failure)
    }
}

fn like_pattern(query: &str) -> String {
    let escaped = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");

    format!("%{escaped}%")
}

fn to_prospect(row: &SqliteRow) -> Result<Prospect, RepositoryError> {
    let kind: String = row.try_get("kind").map_err(failure)?;
    let concessions: i64 = row.try_get("concessions").map_err(failure)?;

    Ok(Prospect {
        name: row.try_get("name").map_err(failure)?,
        kind: ProspectKind::from_code(&kind)
            .ok_or_else(|| failure("Tipo de prospecto desconocido"))?,
        profile: row.try_get("profile").map_err(failure)?,
        concessions: u32::try_from(concessions).map_err(failure)?,
        hectares: row.try_get("hectares").map_err(failure)?,
        departments: row.try_get("departments").map_err(failure)?,
        provinces: row.try_get("provinces").map_err(failure)?,
        substances: row.try_get("substances").map_err(failure)?,
        concession_states: row.try_get("concession_states").map_err(failure)?,
        potential_service: row.try_get("potential_service").map_err(failure)?,
        needs_review: row.try_get("needs_review").map_err(failure)?,
        source: row.try_get("source").map_err(failure)?,
        tax: to_tax_record(row)?,
    })
}

fn to_tax_record(row: &SqliteRow) -> Result<Option<TaxRecord>, RepositoryError> {
    let ruc: Option<String> = row.try_get("ruc").map_err(failure)?;
    let Some(ruc) = ruc else {
        return Ok(None);
    };
    let text = |column: &str| -> Result<String, RepositoryError> {
        let value: Option<String> = row.try_get(column).map_err(failure)?;
        Ok(value.unwrap_or_default())
    };
    let approximate: Option<bool> = row.try_get("tax_match_approximate").map_err(failure)?;

    Ok(Some(TaxRecord {
        ruc,
        status: text("tax_status")?,
        condition: text("tax_condition")?,
        address: text("fiscal_address")?,
        approximate: approximate.unwrap_or(false),
    }))
}

fn to_lead(row: &SqliteRow) -> Result<Lead, RepositoryError> {
    let id: String = row.try_get("id").map_err(failure)?;
    let follow_up_on: String = row.try_get("follow_up_on").map_err(failure)?;
    let renewal_of: Option<String> = row.try_get("renewal_of_case").map_err(failure)?;

    Lead::restore(LeadRecord {
        id: Uuid::parse_str(&id).map_err(failure)?,
        name: row.try_get("name").map_err(failure)?,
        phone: row.try_get("phone").map_err(failure)?,
        ruc: row.try_get("ruc").map_err(failure)?,
        service: row.try_get("service").map_err(failure)?,
        owner: row.try_get("owner").map_err(failure)?,
        stage: row.try_get("stage").map_err(failure)?,
        follow_up_on: Date::parse(&follow_up_on, &Iso8601::DATE).map_err(failure)?,
        renewal_of: renewal_of
            .map(|case| Uuid::parse_str(&case))
            .transpose()
            .map_err(failure)?,
    })
    .map_err(failure)
}

fn to_case(row: &SqliteRow) -> Result<Case, RepositoryError> {
    let id: String = row.try_get("id").map_err(failure)?;
    let lead_id: String = row.try_get("lead_id").map_err(failure)?;
    let opened_on: String = row.try_get("opened_on").map_err(failure)?;

    Case::restore(CaseRecord {
        id: Uuid::parse_str(&id).map_err(failure)?,
        lead_id: Uuid::parse_str(&lead_id).map_err(failure)?,
        client_name: row.try_get("client_name").map_err(failure)?,
        ruc: row.try_get("ruc").map_err(failure)?,
        service: row.try_get("service").map_err(failure)?,
        owner: row.try_get("owner").map_err(failure)?,
        opened_on: Date::parse(&opened_on, &Iso8601::DATE).map_err(failure)?,
        status: row.try_get("status").map_err(failure)?,
        contract_total_cents: row.try_get("contract_total_cents").map_err(failure)?,
    })
    .map_err(failure)
}

fn to_case_step(row: &SqliteRow) -> Result<CaseStep, RepositoryError> {
    let id: String = row.try_get("id").map_err(failure)?;
    let case_id: String = row.try_get("case_id").map_err(failure)?;
    let position: i64 = row.try_get("position").map_err(failure)?;
    let due_on: String = row.try_get("due_on").map_err(failure)?;
    let done_on: Option<String> = row.try_get("done_on").map_err(failure)?;

    CaseStep::restore(CaseStepRecord {
        id: Uuid::parse_str(&id).map_err(failure)?,
        case_id: Uuid::parse_str(&case_id).map_err(failure)?,
        position: u32::try_from(position).map_err(failure)?,
        title: row.try_get("title").map_err(failure)?,
        due_on: Date::parse(&due_on, &Iso8601::DATE).map_err(failure)?,
        done_on: done_on
            .map(|date| Date::parse(&date, &Iso8601::DATE))
            .transpose()
            .map_err(failure)?,
    })
    .map_err(failure)
}

fn to_milestone(row: &SqliteRow) -> Result<BillingMilestone, RepositoryError> {
    let id: String = row.try_get("id").map_err(failure)?;
    let case_id: String = row.try_get("case_id").map_err(failure)?;
    let position: i64 = row.try_get("position").map_err(failure)?;
    let due_on: String = row.try_get("due_on").map_err(failure)?;
    let paid_on: Option<String> = row.try_get("paid_on").map_err(failure)?;

    BillingMilestone::restore(BillingMilestoneRecord {
        id: Uuid::parse_str(&id).map_err(failure)?,
        case_id: Uuid::parse_str(&case_id).map_err(failure)?,
        position: u32::try_from(position).map_err(failure)?,
        title: row.try_get("title").map_err(failure)?,
        amount_cents: row.try_get("amount_cents").map_err(failure)?,
        due_on: Date::parse(&due_on, &Iso8601::DATE).map_err(failure)?,
        paid_on: paid_on
            .map(|date| Date::parse(&date, &Iso8601::DATE))
            .transpose()
            .map_err(failure)?,
    })
    .map_err(failure)
}

fn to_quote(
    row: &SqliteRow,
    lines: &mut HashMap<String, Vec<QuoteLineRecord>>,
) -> Result<Quote, RepositoryError> {
    let id: String = row.try_get("id").map_err(failure)?;
    let lead_id: String = row.try_get("lead_id").map_err(failure)?;
    let supersedes: Option<String> = row.try_get("supersedes").map_err(failure)?;
    let issued_on: String = row.try_get("issued_on").map_err(failure)?;
    let valid_until: String = row.try_get("valid_until").map_err(failure)?;

    Quote::restore(QuoteRecord {
        id: Uuid::parse_str(&id).map_err(failure)?,
        lead_id: Uuid::parse_str(&lead_id).map_err(failure)?,
        year: row.try_get("year").map_err(failure)?,
        sequence: row.try_get("sequence").map_err(failure)?,
        version: row.try_get("version").map_err(failure)?,
        supersedes: supersedes
            .map(|id| Uuid::parse_str(&id))
            .transpose()
            .map_err(failure)?,
        client_name: row.try_get("client_name").map_err(failure)?,
        client_ruc: row.try_get("client_ruc").map_err(failure)?,
        service: row.try_get("service").map_err(failure)?,
        issued_on: Date::parse(&issued_on, &Iso8601::DATE).map_err(failure)?,
        valid_until: Date::parse(&valid_until, &Iso8601::DATE).map_err(failure)?,
        notes: row.try_get("notes").map_err(failure)?,
        status: row.try_get("status").map_err(failure)?,
        igv_basis_points: row.try_get("igv_basis_points").map_err(failure)?,
        company: CompanyProfile {
            name: row.try_get("company_name").map_err(failure)?,
            ruc: row.try_get("company_ruc").map_err(failure)?,
            address: row.try_get("company_address").map_err(failure)?,
            phone: row.try_get("company_phone").map_err(failure)?,
            email: row.try_get("company_email").map_err(failure)?,
        },
        lines: lines.remove(&id).unwrap_or_default(),
    })
    .map_err(failure)
}

fn failure(error: impl ToString) -> RepositoryError {
    RepositoryError(error.to_string())
}
