//! Command-line surface. Help text is in Spanish; flags and JSON stay English.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use cominorsa_application::QuoteLineValues;
use time::{Date, format_description::well_known::Iso8601};
use uuid::Uuid;

const ABOUT: &str = "COMINORSA CRM sin ventanas: la misma lógica que la aplicación de escritorio.

Toda orden acepta --json. No envía mensajes ni contacta a nadie.

Códigos de salida: 0 correcto, 1 rechazo del negocio, 2 uso incorrecto, 3 almacenamiento.";

pub fn parse_day(value: &str) -> Result<Date, String> {
    Date::parse(value, &Iso8601::DATE).map_err(|_| format!("«{value}» no es una fecha AAAA-MM-DD"))
}

fn parse_line(value: &str) -> Result<QuoteLineValues, String> {
    let mut fields = value.rsplitn(3, '|');
    let (Some(unit_price), Some(quantity), Some(description)) =
        (fields.next(), fields.next(), fields.next())
    else {
        return Err(format!(
            "«{value}» no tiene la forma descripción|cantidad|precio"
        ));
    };

    Ok(QuoteLineValues {
        description: description.to_owned(),
        quantity: quantity.to_owned(),
        unit_price: unit_price.to_owned(),
    })
}

#[derive(Debug, Parser)]
#[command(name = "cominorsa", version, about = ABOUT)]
pub struct Cli {
    /// Base de datos, o COMINORSA_DB_PATH. Sin ella se niega: nunca usa la base real
    #[arg(long, global = true, value_name = "RUTA")]
    pub db: Option<PathBuf>,

    /// Responde con JSON: {"data": ...} o, en stderr, {"error": {"code", "message"}}
    #[arg(long, global = true)]
    pub json: bool,

    /// Fija el día de hoy, para resultados reproducibles
    #[arg(long, global = true, value_name = "AAAA-MM-DD", value_parser = parse_day)]
    pub today: Option<Date>,

    /// Fija el instante en segundos Unix, para resultados reproducibles
    #[arg(long, global = true, value_name = "SEGUNDOS")]
    pub now: Option<i64>,

    /// Deriva los identificadores nuevos de este texto. Usa uno por orden
    #[arg(long, global = true, value_name = "TEXTO")]
    pub id_seed: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Muestra cuántos registros hay guardados
    Status,
    /// Exporta el estado en JSON, sin cambiar nada
    Export,
    /// Muestra seguimientos, trámites y cobros vencidos o próximos
    Agenda,
    /// Registra, consulta y avanza leads
    #[command(subcommand)]
    Lead(LeadCommand),
    /// Importa, busca y convierte prospectos
    #[command(subcommand)]
    Prospect(ProspectCommand),
    /// Consulta y cierra expedientes
    #[command(subcommand)]
    Case(CaseCommand),
    /// Marca y fecha los pasos de un expediente
    #[command(subcommand)]
    Step(StepCommand),
    /// Gestiona los cobros de un expediente. No emite comprobantes
    #[command(subcommand)]
    Billing(BillingCommand),
    /// Gestiona cotizaciones. No son comprobantes de pago
    #[command(subcommand)]
    Quote(QuoteCommand),
    /// Consulta y cambia la configuración
    #[command(subcommand)]
    Settings(SettingsCommand),
    /// Crea y restaura copias de seguridad
    #[command(subcommand)]
    Backup(BackupCommand),
}

#[derive(Debug, Subcommand)]
pub enum LeadCommand {
    /// Registra un lead nuevo
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        phone: String,
        #[arg(long)]
        service: String,
        #[arg(long)]
        ruc: Option<String>,
        /// Responsable
        #[arg(long)]
        owner: Option<String>,
        /// Próximo seguimiento; por defecto, según la configuración
        #[arg(long, value_name = "AAAA-MM-DD")]
        follow_up: Option<String>,
    },
    /// Lista los leads
    List {
        /// Filtra por nombre, teléfono, RUC o servicio
        #[arg(long)]
        query: Option<String>,
    },
    /// Muestra un lead
    Show { id: Uuid },
    /// Resume el embudo
    Summary,
    /// Avanza una etapa; al firmar el contrato abre el expediente
    Advance { id: Uuid },
    /// Cambia la persona responsable
    Assign {
        id: Uuid,
        #[arg(long)]
        owner: String,
    },
    /// Imprime el enlace de un chat preparado. No envía nada
    Whatsapp { id: Uuid },
}

#[derive(Debug, Subcommand)]
pub enum ProspectCommand {
    /// Carga los archivos CSV con el importador existente
    Import {
        #[arg(long, value_name = "CSV")]
        companies: PathBuf,
        #[arg(long, value_name = "CSV")]
        people: PathBuf,
    },
    /// Agrega los datos del padrón de SUNAT a las empresas
    Enrich {
        #[arg(long, value_name = "CSV")]
        sunat: PathBuf,
    },
    /// Busca prospectos
    Search {
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long, default_value_t = cominorsa_application::PROSPECT_PAGE_SIZE)]
        limit: u32,
    },
    /// Cuenta los prospectos
    Count,
    /// Crea un lead desde el prospecto con ese nombre exacto
    Convert {
        #[arg(long)]
        name: String,
        /// Las fuentes no traen teléfono, así que es obligatorio
        #[arg(long)]
        phone: String,
        /// company o person, si el nombre es de ambos
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        service: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long, value_name = "AAAA-MM-DD")]
        follow_up: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum CaseCommand {
    /// Lista los expedientes abiertos y los cerrados con cobros pendientes
    List,
    /// Muestra un expediente con sus pasos y cobros
    Show { id: Uuid },
    /// Cierra un expediente; un servicio recurrente crea un lead de renovación
    Close { id: Uuid },
}

#[derive(Debug, Subcommand)]
pub enum StepCommand {
    /// Marca un paso como hecho hoy
    Done { id: Uuid },
    /// Marca un paso como pendiente
    Undo { id: Uuid },
    /// Cambia la fecha de un paso
    Date {
        id: Uuid,
        #[arg(long, value_name = "AAAA-MM-DD")]
        date: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum BillingCommand {
    /// Define el total del contrato
    Total {
        case: Uuid,
        #[arg(long, value_name = "SOLES")]
        amount: String,
    },
    /// Aplica el plan sugerido 30 / 40 / 30
    Plan { case: Uuid },
    /// Agrega un hito de cobro
    Add {
        case: Uuid,
        #[arg(long)]
        title: String,
        #[arg(long, value_name = "SOLES")]
        amount: String,
        #[arg(long, value_name = "AAAA-MM-DD")]
        due: String,
    },
    /// Cambia un hito: todos los datos o ninguno
    Edit {
        id: Uuid,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, value_name = "SOLES")]
        amount: Option<String>,
        #[arg(long, value_name = "AAAA-MM-DD")]
        due: Option<String>,
    },
    /// Marca un hito como pagado hoy
    Pay { id: Uuid },
    /// Marca un hito como pendiente
    Unpay { id: Uuid },
    /// Elimina un hito
    Delete { id: Uuid },
}

#[derive(Debug, Subcommand)]
pub enum QuoteCommand {
    /// Crea un borrador numerado
    Create {
        #[arg(long)]
        lead: Uuid,
        #[command(flatten)]
        content: QuoteContentArgs,
    },
    /// Cambia un borrador. Una línea nueva reemplaza todas las anteriores
    Edit {
        /// Número, como COT-2026-0001, o identificador
        quote: String,
        #[command(flatten)]
        content: QuoteContentArgs,
    },
    /// Lista las cotizaciones
    List {
        #[arg(long)]
        lead: Option<Uuid>,
    },
    /// Muestra una cotización
    Show { quote: String },
    /// Marca una cotización como enviada y congela los datos de la empresa
    Send {
        quote: String,
        /// Permite enviarla sin nombre de empresa
        #[arg(long)]
        allow_blank_company: bool,
    },
    /// Marca una cotización como aceptada
    Accept { quote: String },
    /// Marca una cotización como rechazada
    Reject { quote: String },
    /// Crea una nueva versión de una cotización enviada
    Version { quote: String },
    /// Guarda la cotización en PDF
    Pdf {
        quote: String,
        #[arg(long, value_name = "RUTA")]
        out: PathBuf,
    },
}

#[derive(Args, Debug)]
pub struct QuoteContentArgs {
    /// Línea descripción|cantidad|precio. Se puede repetir
    #[arg(long, value_name = "LINEA", value_parser = parse_line)]
    pub line: Vec<QuoteLineValues>,
    #[arg(long, value_name = "AAAA-MM-DD")]
    pub issued_on: Option<String>,
    #[arg(long, value_name = "AAAA-MM-DD")]
    pub valid_until: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Muestra la configuración
    Show,
    /// Cambia varios valores: todos o ninguno
    Set(Box<SettingsArgs>),
}

#[derive(Args, Debug)]
pub struct SettingsArgs {
    /// system, light o dark
    #[arg(long)]
    pub theme: Option<String>,
    #[arg(long, value_name = "DIAS")]
    pub follow_up_days: Option<u32>,
    /// Admite {nombre} y {servicio}
    #[arg(long, value_name = "TEXTO")]
    pub whatsapp_template: Option<String>,
    /// Tasa en porcentaje, como 18
    #[arg(long, value_name = "TASA")]
    pub igv: Option<String>,
    #[arg(long, value_name = "CARPETA")]
    pub backup_directory: Option<String>,
    #[arg(long)]
    pub company_name: Option<String>,
    #[arg(long)]
    pub company_ruc: Option<String>,
    #[arg(long)]
    pub company_address: Option<String>,
    #[arg(long)]
    pub company_phone: Option<String>,
    #[arg(long)]
    pub company_email: Option<String>,
    /// Se puede repetir
    #[arg(long, value_name = "NOMBRE")]
    pub add_member: Vec<String>,
    /// Se puede repetir
    #[arg(long, value_name = "NOMBRE")]
    pub remove_member: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    /// Copia la base en la carpeta indicada o en la guardada
    Create {
        #[arg(long, value_name = "CARPETA")]
        folder: Option<String>,
    },
    /// Reemplaza la base con una copia validada, tras guardar una copia de seguridad
    Restore {
        #[arg(long, value_name = "ARCHIVO")]
        file: PathBuf,
    },
}
