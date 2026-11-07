use std::{env, fs, path::PathBuf};

use cominorsa_domain::{ProspectKind, ProspectRepository};
use cominorsa_importer::{ImportError, enrich, parse_prospects, parse_tax_records, run};
use cominorsa_storage::SqliteLeads;
use uuid::Uuid;

const HEADER: &str = "titular,perfil_icp,n_concesiones,hectareas_totales,departamentos,provincias,sustancias,estados_concesion,servicio_potencial,revisar_manual,fuente_dato";
const VALID: &str = "Empresa Demo,PERFIL_1_formalizacion,3,120.5,PIURA,SULLANA,M,TITULADA,IGAFOM + REINFO,FALSE,Synthetic";

fn file(rows: &[&str]) -> String {
    format!("{HEADER}\n{}\n", rows.join("\n"))
}

#[test]
fn parses_a_valid_row() {
    let prospects = parse_prospects(file(&[VALID]).as_bytes(), ProspectKind::Company).unwrap();

    assert_eq!(prospects.len(), 1);
    assert_eq!(prospects[0].name, "Empresa Demo");
    assert_eq!(prospects[0].kind, ProspectKind::Company);
    assert_eq!(prospects[0].concessions, 3);
    assert_eq!(prospects[0].hectares, 120.5);
    assert!(!prospects[0].needs_review);
}

#[test]
fn rejects_files_without_rows_or_expected_columns() {
    let parse = |text: &str| parse_prospects(text.as_bytes(), ProspectKind::Person);

    assert!(matches!(parse(HEADER), Err(ImportError::NoRows)));
    assert!(matches!(
        parse("nombre,telefono\nA,1\n"),
        Err(ImportError::MissingColumn("titular"))
    ));
}

#[test]
fn reports_the_line_of_invalid_values_without_echoing_them() {
    let bad = VALID.replace(",3,", ",tres,");
    let error =
        parse_prospects(file(&[VALID, &bad]).as_bytes(), ProspectKind::Company).unwrap_err();

    assert!(matches!(
        error,
        ImportError::InvalidValue {
            line: 3,
            column: "n_concesiones"
        }
    ));
    assert!(!error.to_string().contains("Empresa Demo"));
}

#[test]
fn rejects_duplicate_titleholders() {
    let error =
        parse_prospects(file(&[VALID, VALID]).as_bytes(), ProspectKind::Company).unwrap_err();

    assert!(matches!(error, ImportError::Duplicate { line: 3 }));
}

const TAX_HEADER: &str = "titular,match,ruc,razon_social,estado,condicion,ubigeo,domicilio";

#[test]
fn parses_only_rows_with_a_ruc_and_marks_approximate_matches() {
    let text = format!(
        "{TAX_HEADER}\nExacta,unico,20123456786,EXACTA SAC,ACTIVO,HABIDO,200101,CAL UNO\nCerca,aproximado,20100070970,CERCA SAC,ACTIVO,HABIDO,200101,CAL DOS\nNada,sin_coincidencia,,,,,,\n"
    );

    let records = parse_tax_records(text.as_bytes()).unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].0, "Exacta");
    assert!(!records[0].1.approximate);
    assert!(records[1].1.approximate);
}

#[test]
fn rejects_an_invalid_ruc_without_echoing_it() {
    let text = format!("{TAX_HEADER}\nMala,unico,20999999999,MALA,ACTIVO,HABIDO,200101,CAL\n");

    let error = parse_tax_records(text.as_bytes()).unwrap_err();

    assert!(matches!(
        error,
        ImportError::InvalidValue {
            line: 2,
            column: "ruc"
        }
    ));
    assert!(!error.to_string().contains("20999999999"));
}

#[test]
fn rejects_repeated_companies_in_the_tax_file() {
    let row = "Doble,unico,20123456786,DOBLE,ACTIVO,HABIDO,200101,CAL";
    let other = "Doble,unico,20100070970,DOBLE,ACTIVO,HABIDO,200101,CAL";
    let text = format!("{TAX_HEADER}\n{row}\n{other}\n");

    let error = parse_tax_records(text.as_bytes()).unwrap_err();

    assert!(matches!(error, ImportError::Duplicate { line: 3 }));
}

#[test]
fn rejects_unknown_match_labels_instead_of_confirming_them() {
    for label in ["ambiguo", "Aproximado", "otro"] {
        let text =
            format!("{TAX_HEADER}\nEmpresa,{label},20123456786,E,ACTIVO,HABIDO,200101,CAL\n");

        let error = parse_tax_records(text.as_bytes()).unwrap_err();

        assert!(
            matches!(
                error,
                ImportError::InvalidValue {
                    line: 2,
                    column: "match"
                }
            ),
            "{label} must be rejected"
        );
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("cominorsa-import-{}", Uuid::now_v7()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn imports_both_files_once_and_writes_nothing_when_one_is_invalid() {
    let scratch = Scratch::new();
    let companies = scratch.write("companies.csv", &file(&[VALID]));
    let people = scratch.write(
        "people.csv",
        &file(&[&VALID.replace("Empresa Demo", "Persona Demo")]),
    );
    let broken = scratch.write("broken.csv", "nombre\nA\n");
    let database = scratch.0.join("crm.db");

    let store = SqliteLeads::open(&database).await.unwrap();
    store.close().await;

    assert!(run(&database, &companies, &broken).await.is_err());
    let store = SqliteLeads::open(&database).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 0);
    store.close().await;

    assert_eq!(run(&database, &companies, &people).await.unwrap().added, 2);
    let repeated = run(&database, &companies, &people).await.unwrap();
    assert_eq!(repeated.added, 0);
    assert_eq!(repeated.total, 2);
    assert!(repeated.backup.exists());
}

#[tokio::test]
async fn enriches_an_existing_database_after_backing_it_up() {
    let scratch = Scratch::new();
    let companies = scratch.write("companies.csv", &file(&[VALID]));
    let people = scratch.write(
        "people.csv",
        &file(&[&VALID.replace("Empresa Demo", "Persona Demo")]),
    );
    let tax = scratch.write(
        "tax.csv",
        &format!(
            "{TAX_HEADER}\nEmpresa Demo,unico,20123456786,EMPRESA DEMO,ACTIVO,HABIDO,200101,CAL\n"
        ),
    );
    let database = scratch.0.join("crm.db");
    SqliteLeads::open(&database).await.unwrap().close().await;
    run(&database, &companies, &people).await.unwrap();

    let report = enrich(&database, &tax).await.unwrap();

    assert_eq!(report.updated, 1);
    assert!(report.backup.exists());
    let store = SqliteLeads::open(&database).await.unwrap();
    assert!(
        store.search("20123456786", 5).await.unwrap()[0]
            .tax
            .is_some()
    );
}
