//! Loads prospect CSV files into the local database.
//!
//! Errors name lines and columns but never echo values, because sources hold
//! personal data.

use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use cominorsa_domain::{
    Prospect, ProspectKind, ProspectRepository, RepositoryError, Ruc, TaxRecord,
};
use cominorsa_storage::SqliteLeads;
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
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

#[derive(Debug)]
pub struct ImportReport {
    pub added: usize,
    pub total: usize,
    pub backup: Option<PathBuf>,
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

        records.push((
            text(0),
            TaxRecord {
                ruc,
                status: text(3),
                condition: text(4),
                address: text(5),
                approximate: text(1) == APPROXIMATE_MATCH,
            },
        ));
    }

    if records.is_empty() {
        return Err(ImportError::NoRows);
    }
    Ok(records)
}

/// Validates the file first, then backs up the database before writing.
pub async fn enrich(database: &Path, tax_file: &Path) -> Result<EnrichReport, ImportError> {
    let records = parse_tax_records(File::open(tax_file).map_err(unreadable)?)?;
    if !database.exists() {
        return Err(ImportError::Unreadable(
            "La base de datos no existe".to_owned(),
        ));
    }

    let store = SqliteLeads::open(database).await?;
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

/// Validates both files before touching the database, and backs up an
/// existing database before writing.
pub async fn run(
    database: &Path,
    companies: &Path,
    people: &Path,
) -> Result<ImportReport, ImportError> {
    let mut prospects = read_file(companies, ProspectKind::Company)?;
    prospects.extend(read_file(people, ProspectKind::Person)?);

    let existed = database.exists();
    let store = SqliteLeads::open(database).await?;
    let backup = if existed {
        let path = backup_path(database);
        store.backup_to(&path).await?;
        Some(path)
    } else {
        None
    };

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
