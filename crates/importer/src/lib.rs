//! Loads prospect CSV files into the local database.
//!
//! Errors name lines and columns but never echo values, because sources hold
//! personal data.

use std::{
    cmp::Ordering,
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use cominorsa_domain::{
    Prospect, ProspectKind, ProspectRepository, RepositoryError, Ruc, TaxRecord,
};
use cominorsa_storage::{AsideFiles, SqliteLeads};
use csv::{ReaderBuilder, StringRecord};
use thiserror::Error;

const COLUMNS: [&str; 11] = [
    "titular",
    "perfil_icp",
    "n_concesiones",
    "hectareas_totales",
    "departamentos",
    "provincias",
    "sustancias",
    "estados_concesion",
    "servicio_potencial",
    "revisar_manual",
    "fuente_dato",
];

const TAX_COLUMNS: [&str; 6] = [
    "titular",
    "match",
    "ruc",
    "estado",
    "condicion",
    "domicilio",
];
const APPROXIMATE_MATCH: &str = "aproximado";
const CONFIRMED_MATCHES: [&str; 2] = ["unico", "unico_activo"];

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("No se pudo leer el archivo: {0}")]
    Unreadable(String),
    #[error("El archivo no contiene filas")]
    NoRows,
    #[error("Falta la columna {0}")]
    MissingColumn(&'static str),
    #[error("Valor inválido en la línea {line}, columna {column}")]
    InvalidValue { line: u64, column: &'static str },
    #[error("Titular duplicado en la línea {line}")]
    Duplicate { line: u64 },
    #[error("No existe la base de datos {}. Créala con: cominorsa init", .0.display())]
    DatabaseMissing(PathBuf),
    #[error(
        "La base está en la versión {found} y este programa necesita la {required}. Actualízala con: cominorsa migrate"
    )]
    SchemaOutdated { found: i64, required: i64 },
    #[error(
        "La base está en la versión {found}, más nueva que la {required} que admite este programa. Actualiza el programa"
    )]
    SchemaNewer { found: i64, required: i64 },
    #[error("{0}")]
    DatabaseAside(AsideFiles),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

impl ImportError {
    /// Whether the database was refused, which the CLI contract reports as a
    /// storage failure and not as a rejected input.
    pub fn is_database_refusal(&self) -> bool {
        matches!(
            self,
            Self::DatabaseMissing(_)
                | Self::DatabaseAside(_)
                | Self::SchemaOutdated { .. }
                | Self::SchemaNewer { .. }
        )
    }
}

#[derive(Debug)]
pub struct ImportReport {
    pub added: usize,
    pub total: usize,
    pub backup: PathBuf,
}

#[derive(Debug)]
pub struct EnrichReport {
    pub updated: usize,
    pub backup: PathBuf,
}

/// Reads rows with a RUC. Rows without one stay out, never guessed.
pub fn parse_tax_records(source: impl Read) -> Result<Vec<(String, TaxRecord)>, ImportError> {
    let bytes = strip_bom(source)?;
    let mut reader = ReaderBuilder::new().from_reader(bytes.as_slice());
    let headers = reader.headers().map_err(unreadable)?.clone();
    let positions = positions_of(&TAX_COLUMNS, &headers)?;
    let mut records = Vec::new();
    let mut names = HashSet::new();

    for record in reader.records() {
        let record = record.map_err(unreadable)?;
        let line = record.position().map_or(0, |position| position.line());
        let text = |index: usize| record.get(positions[index]).unwrap_or("").trim().to_owned();
        let ruc = text(2);
        if ruc.is_empty() {
            continue;
        }
        Ruc::parse(&ruc).map_err(|_| ImportError::InvalidValue {
            line,
            column: "ruc",
        })?;
        let approximate = match text(1).as_str() {
            APPROXIMATE_MATCH => true,
            label if CONFIRMED_MATCHES.contains(&label) => false,
            _ => {
                return Err(ImportError::InvalidValue {
                    line,
                    column: "match",
                });
            }
        };
        if !names.insert(text(0)) {
            return Err(ImportError::Duplicate { line });
        }

        records.push((
            text(0),
            TaxRecord {
                ruc,
                status: text(3),
                condition: text(4),
                address: text(5),
                approximate,
            },
        ));
    }

    if records.is_empty() {
        return Err(ImportError::NoRows);
    }
    Ok(records)
}

/// Opens a database under the checks of every `cominorsa` write: it is never
/// created or migrated, and a refused file stays as it was. The importer does
/// not depend on the application crate, so the wording is repeated here, and
/// `scripts/e2e-cli.sh` proves both messages match.
async fn open_for_import(database: &Path) -> Result<SqliteLeads, ImportError> {
    if let Some(found) = AsideFiles::find(database) {
        return Err(ImportError::DatabaseAside(found));
    }
    if !database.exists() {
        return Err(ImportError::DatabaseMissing(database.to_path_buf()));
    }

    let reader = SqliteLeads::open_read_only(database).await?;
    let found = reader.schema_version().await?;
    reader.close().await;
    let required = SqliteLeads::required_schema_version();

    match found.cmp(&required) {
        Ordering::Less => Err(ImportError::SchemaOutdated { found, required }),
        Ordering::Greater => Err(ImportError::SchemaNewer { found, required }),
        Ordering::Equal => Ok(SqliteLeads::open_existing(database).await?),
    }
}

/// Validates the file first, then backs up the database before writing.
pub async fn enrich(database: &Path, tax_file: &Path) -> Result<EnrichReport, ImportError> {
    let records = parse_tax_records(File::open(tax_file).map_err(unreadable)?)?;
    let store = open_for_import(database).await?;
    let backup = backup_path(database);
    store.backup_to(&backup).await?;
    let updated = store.apply_tax_records(&records).await?;
    store.close().await;

    Ok(EnrichReport { updated, backup })
}

pub fn parse_prospects(
    source: impl Read,
    kind: ProspectKind,
) -> Result<Vec<Prospect>, ImportError> {
    let bytes = strip_bom(source)?;
    let mut reader = ReaderBuilder::new().from_reader(bytes.as_slice());
    let headers = reader.headers().map_err(unreadable)?.clone();
    let positions = column_positions(&headers)?;
    let mut prospects = Vec::new();
    let mut names = HashSet::new();

    for record in reader.records() {
        let record = record.map_err(unreadable)?;
        let line = record.position().map_or(0, |position| position.line());
        let row = Row {
            record: &record,
            positions: &positions,
            line,
        };
        let prospect = row.to_prospect(kind)?;

        if !names.insert(prospect.name.clone()) {
            return Err(ImportError::Duplicate { line });
        }
        prospects.push(prospect);
    }

    if prospects.is_empty() {
        return Err(ImportError::NoRows);
    }
    Ok(prospects)
}

/// Validates both files before touching the database, and backs it up before
/// writing.
pub async fn run(
    database: &Path,
    companies: &Path,
    people: &Path,
) -> Result<ImportReport, ImportError> {
    let mut prospects = read_file(companies, ProspectKind::Company)?;
    prospects.extend(read_file(people, ProspectKind::Person)?);

    let store = open_for_import(database).await?;
    let backup = backup_path(database);
    store.backup_to(&backup).await?;

    let added = store.import(&prospects).await?;
    let total = store.count().await?;
    store.close().await;

    Ok(ImportReport {
        added,
        total,
        backup,
    })
}

struct Row<'a> {
    record: &'a StringRecord,
    positions: &'a [usize],
    line: u64,
}

impl Row<'_> {
    fn text(&self, index: usize) -> String {
        self.record
            .get(self.positions[index])
            .unwrap_or("")
            .trim()
            .to_owned()
    }

    fn invalid(&self, index: usize) -> ImportError {
        ImportError::InvalidValue {
            line: self.line,
            column: COLUMNS[index],
        }
    }

    fn to_prospect(&self, kind: ProspectKind) -> Result<Prospect, ImportError> {
        let name = self.text(0);
        if name.is_empty() {
            return Err(self.invalid(0));
        }

        Ok(Prospect {
            name,
            kind,
            profile: self.text(1),
            concessions: self.text(2).parse().map_err(|_| self.invalid(2))?,
            hectares: self
                .text(3)
                .parse()
                .ok()
                .filter(|hectares: &f64| hectares.is_finite() && *hectares >= 0.0)
                .ok_or_else(|| self.invalid(3))?,
            departments: self.text(4),
            provinces: self.text(5),
            substances: self.text(6),
            concession_states: self.text(7),
            potential_service: self.text(8),
            needs_review: match self.text(9).as_str() {
                "TRUE" => true,
                "FALSE" => false,
                _ => return Err(self.invalid(9)),
            },
            source: self.text(10),
            tax: None,
        })
    }
}

fn column_positions(headers: &StringRecord) -> Result<Vec<usize>, ImportError> {
    positions_of(&COLUMNS, headers)
}

fn positions_of(
    columns: &[&'static str],
    headers: &StringRecord,
) -> Result<Vec<usize>, ImportError> {
    columns
        .iter()
        .map(|column| {
            headers
                .iter()
                .position(|header| header.trim() == *column)
                .ok_or(ImportError::MissingColumn(column))
        })
        .collect()
}

fn strip_bom(mut source: impl Read) -> Result<Vec<u8>, ImportError> {
    let mut bytes = Vec::new();
    source.read_to_end(&mut bytes).map_err(unreadable)?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    Ok(bytes)
}

fn read_file(path: &Path, kind: ProspectKind) -> Result<Vec<Prospect>, ImportError> {
    parse_prospects(File::open(path).map_err(unreadable)?, kind)
}

fn backup_path(database: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    database.with_extension(format!("before-import-{stamp}.db"))
}

fn unreadable(error: impl ToString) -> ImportError {
    ImportError::Unreadable(error.to_string())
}
