use cominorsa_desktop::{
    Agenda, AgendaGroups, AgendaItem, FollowUp, FormValues, LeadForm, PipelineSummary, Prospects,
    SaveState, Section, SettingsView, Shell, Workspace, format_date, format_timestamp,
    matches_query, next_follow_up, owner_options, parse_form, profile_label, window_icon,
};
use cominorsa_domain::{
    Lead, NewLead, Prospect, ProspectKind, Settings, TaxRecord, ThemePreference,
};
use dioxus::prelude::*;
use time::{Date, macros::date};

const TODAY: Date = date!(2026 - 11 - 06);

fn sample(name: &str, follow_up_on: Date, advances: usize) -> Lead {
    let mut lead = Lead::create(NewLead {
        name: name.to_owned(),
        phone: "900000000".to_owned(),
        ruc: Some("20123456786".to_owned()),
        service: "DAC".to_owned(),
        owner: None,
        follow_up_on,
    })
    .unwrap();
    lead.record_consent(TODAY, "El cliente inició el contacto")
        .unwrap();
    for _ in 0..advances {
        lead.advance().unwrap();
    }
    lead
}

fn leads() -> Vec<Lead> {
    vec![
        sample("Atrasada", date!(2026 - 11 - 01), 0),
        sample("Futura", date!(2026 - 12 - 01), 1),
        sample("Cerrada", date!(2026 - 11 - 01), 3),
    ]
}

fn render(component: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(component);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

fn workspace_with_leads() -> Element {
    rsx! {
        Workspace {
            leads: leads(),
            today: TODAY,
            query: String::new(),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

fn workspace_with_whatsapp() -> Element {
    let mut with_phone = leads();
    with_phone.push(
        Lead::create(NewLead {
            name: "Sin WhatsApp".to_owned(),
            phone: "073123456".to_owned(),
            ruc: None,
            service: "DAC".to_owned(),
            owner: None,
            follow_up_on: TODAY,
        })
        .unwrap(),
    );
    // With consent, only the phone decides whether the chat opens.
    for lead in &mut with_phone {
        if !lead.has_consent() {
            lead.record_consent(TODAY, "El cliente inició el contacto")
                .unwrap();
        }
    }

    rsx! {
        Workspace {
            leads: with_phone,
            today: TODAY,
            query: String::new(),
            whatsapp_template: "Hola {nombre}".to_owned(),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

#[test]
fn links_valid_phones_to_a_prepared_whatsapp_chat() {
    let html = render(workspace_with_whatsapp);

    assert_eq!(html.matches(">WhatsApp<").count(), 3);
    assert!(html.contains("https://wa.me/51900000000?text=Hola%20Atrasada"));
}

fn workspace_without_matches() -> Element {
    rsx! {
        Workspace {
            leads: leads(),
            today: TODAY,
            query: "inexistente".to_owned(),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

fn empty_workspace() -> Element {
    rsx! {
        Workspace {
            leads: Vec::new(),
            today: TODAY,
            query: String::new(),
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

fn form_with_error() -> Element {
    rsx! {
        LeadForm {
            values: FormValues::default(),
            saving: true,
            error: "El RUC no es válido".to_owned(),
            team: vec!["Ana".to_owned()],
            on_change: |_| {},
            on_submit: |_| {},
            on_close: |_| {},
        }
    }
}

#[test]
fn renders_every_pipeline_stage_with_its_leads() {
    let html = render(workspace_with_leads);

    for label in [
        "Contacto inicial",
        "Evaluación de coordenadas",
        "Cotización enviada",
        "Contrato firmado",
    ] {
        assert!(html.contains(label), "missing stage {label}");
    }
    assert!(html.contains("Futura"));
}

#[test]
fn summarizes_the_pipeline() {
    let mut leads = leads();
    leads.push(sample("Hoy", TODAY, 0));

    assert_eq!(
        PipelineSummary::of(&leads, TODAY),
        PipelineSummary {
            active: 3,
            overdue: 1,
            due_today: 1,
            signed: 1,
        }
    );
}

#[test]
fn describes_follow_up_status() {
    let of = |date, advances| FollowUp::of(&sample("Lead", date, advances), TODAY).label();

    assert_eq!(of(date!(2026 - 11 - 01), 0), "Vencido hace 5 días");
    assert_eq!(of(date!(2026 - 11 - 05), 0), "Vencido hace 1 día");
    assert_eq!(of(TODAY, 0), "Vence hoy");
    assert_eq!(of(date!(2026 - 12 - 01), 0), "Seguimiento 01/12/2026");
    assert_eq!(of(date!(2026 - 11 - 01), 3), "Cerrado");
}

#[test]
fn shows_status_on_cards_and_no_advance_for_signed_contracts() {
    let html = render(workspace_with_leads);

    assert_eq!(html.matches("Vencido hace 5 días").count(), 1);
    assert_eq!(html.matches(">Avanzar<").count(), 2);
}

#[test]
fn searches_name_phone_ruc_and_service_ignoring_case() {
    let lead = sample("Minera Ejemplo", TODAY, 0);

    for query in ["minera", "900000", "2012345", "dac", "  EJEMPLO  ", ""] {
        assert!(matches_query(&lead, query), "{query} must match");
    }
    assert!(!matches_query(&lead, "otra"));
}

#[test]
fn explains_an_empty_search() {
    let html = render(workspace_without_matches);

    assert!(html.contains("Sin resultados"));
    assert!(!html.contains("Atrasada"));
}

#[test]
fn invites_the_first_lead_on_an_empty_workspace() {
    assert!(render(empty_workspace).contains("Registra tu primer lead"));
}

#[test]
fn formats_dates_for_peru() {
    assert_eq!(format_date(date!(2026 - 11 - 06)), "06/11/2026");
}

#[test]
fn shows_errors_in_the_form_and_blocks_a_second_submit() {
    let html = render(form_with_error);

    assert!(html.contains("role=\"alert\""));
    assert!(html.contains("El RUC no es válido"));
    assert!(html.contains("disabled"));
    assert!(html.contains("Guardando"));
}

fn form(date: &str, ruc: &str) -> FormValues {
    FormValues {
        name: "Minera".to_owned(),
        phone: "900000000".to_owned(),
        ruc: ruc.to_owned(),
        service: "REINFO".to_owned(),
        owner: "Ana".to_owned(),
        follow_up_on: date.to_owned(),
        origin: String::new(),
    }
}

#[test]
fn parses_form_values_into_a_lead() {
    let lead = parse_form(form("2026-11-10", "20123456786"), TODAY).unwrap();

    assert_eq!(lead.name(), "Minera");
    assert_eq!(lead.owner(), Some("Ana"));
    assert_eq!(lead.follow_up_on(), date!(2026 - 11 - 10));
}

#[test]
fn explains_invalid_form_values() {
    assert_eq!(
        parse_form(form("", ""), TODAY).unwrap_err(),
        "Ingresa la fecha de seguimiento"
    );
    assert_eq!(
        parse_form(form("2026-11-10", "123"), TODAY).unwrap_err(),
        "El RUC no es válido"
    );
}

#[test]
fn starts_the_form_with_a_real_follow_up_date() {
    let values = FormValues::starting_on(TODAY);

    assert_eq!(values.follow_up_on, "2026-11-06");
    assert!(values.name.is_empty());
}

fn prospect(service: &str) -> Prospect {
    Prospect {
        name: "Titular Demo".to_owned(),
        kind: ProspectKind::Person,
        profile: "PERFIL_1_formalizacion".to_owned(),
        concessions: 3,
        hectares: 120.5,
        departments: "PIURA".to_owned(),
        provinces: "SULLANA".to_owned(),
        substances: "M".to_owned(),
        concession_states: "TITULADA".to_owned(),
        potential_service: service.to_owned(),
        needs_review: false,
        source: "Synthetic".to_owned(),
        tax: None,
    }
}

fn with_tax(status: &str, approximate: bool) -> Prospect {
    Prospect {
        kind: ProspectKind::Company,
        tax: Some(TaxRecord {
            ruc: "20123456786".to_owned(),
            status: status.to_owned(),
            condition: "HABIDO".to_owned(),
            address: "CAL DEMO 123".to_owned(),
            approximate,
        }),
        ..prospect("DAC")
    }
}

fn prospects_with_tax() -> Element {
    rsx! {
        Prospects {
            prospects: vec![with_tax("ACTIVO", false), with_tax("BAJA DE OFICIO", true)],
            total: 2,
            query: String::new(),
            on_query: |_| {},
            on_convert: |_| {},
        }
    }
}

#[test]
fn shows_ruc_status_and_flags_approximate_matches() {
    let html = render(prospects_with_tax);

    assert!(html.contains("RUC 20123456786"));
    assert!(html.contains("ACTIVO"));
    assert!(html.contains("BAJA DE OFICIO"));
    assert!(html.contains("Confirmar RUC"));
    assert!(html.contains("CAL DEMO 123"));
}

#[test]
fn prefills_only_confirmed_rucs_when_converting() {
    let exact = FormValues::from_prospect(&with_tax("ACTIVO", false), TODAY);
    let approximate = FormValues::from_prospect(&with_tax("ACTIVO", true), TODAY);

    assert_eq!(exact.ruc, "20123456786");
    assert!(exact.phone.is_empty());
    assert!(approximate.ruc.is_empty());
}

fn prospects_view() -> Element {
    rsx! {
        Prospects {
            prospects: vec![prospect("IGAFOM + REINFO")],
            total: 2021,
            query: String::new(),
            on_query: |_| {},
            on_convert: |_| {},
        }
    }
}

#[test]
fn lists_prospects_with_their_total_and_a_convert_action() {
    let html = render(prospects_view);

    assert!(html.contains("Titular Demo"));
    assert!(html.contains("Mostrando 1 de 2021"));
    assert!(html.contains("Formalización"));
    assert!(html.contains(">Convertir en lead<"));
    assert!(!html.contains("PERFIL_1_formalizacion"));
}

#[test]
fn prefills_the_form_from_a_prospect_without_inventing_contact_data() {
    let values = FormValues::from_prospect(&prospect("IGAFOM + REINFO"), TODAY);

    assert_eq!(values.name, "Titular Demo");
    assert_eq!(values.service, "IGAFOM + REINFO");
    assert_eq!(values.follow_up_on, "2026-11-06");
    assert!(values.phone.is_empty());
    assert!(values.ruc.is_empty());
}

#[test]
fn leaves_the_service_blank_when_it_needs_manual_review() {
    assert!(
        FormValues::from_prospect(&prospect("REVISAR_MANUAL"), TODAY)
            .service
            .is_empty()
    );
}

#[test]
fn labels_prospect_profiles() {
    assert_eq!(profile_label("PERFIL_2_cumplimiento"), "Cumplimiento");
    assert_eq!(profile_label("MIXTO_1_y_2"), "Mixto");
    assert_eq!(profile_label("OTRO_estado"), "Por revisar");
    assert_eq!(profile_label("NUEVO"), "NUEVO");
}

fn shell_view() -> Element {
    rsx! {
        Shell {
            section: Section::Settings,
            on_section: |_| {},
            p { "Contenido" }
        }
    }
}

#[test]
fn shows_every_section_and_marks_the_current_one() {
    let html = render(shell_view);

    for label in [
        "Embudo",
        "Agenda",
        "Expedientes",
        "Prospectos",
        "Configuración",
        "Contenido",
    ] {
        assert!(html.contains(label), "missing {label}");
    }
    assert_eq!(html.matches("aria-current=\"page\"").count(), 1);
}

fn settings_props(pending_restore: Option<String>) -> Element {
    let mut settings = Settings::default();
    settings.theme = ThemePreference::Dark;
    settings.add_member("Ana").unwrap();
    settings.set_backup_directory("/media/usb");
    settings.record_backup(1_793_999_400);

    rsx! {
        SettingsView {
            settings,
            database: "/datos/crm.db".to_owned(),
            save_state: SaveState::Saved,
            backup_notice: "Copia creada".to_owned(),
            pending_restore,
            busy: false,
            on_change: |_| {},
            on_pick_folder: |_| {},
            on_backup: |_| {},
            on_pick_restore: |_| {},
            on_confirm_restore: |_| {},
            on_cancel_restore: |_| {},
        }
    }
}

fn settings_view() -> Element {
    settings_props(None)
}

fn settings_with_pending_restore() -> Element {
    settings_props(Some("/media/usb/crm.db".to_owned()))
}

fn settings_without_backup_folder() -> Element {
    rsx! {
        SettingsView {
            settings: Settings::default(),
            database: "/datos/crm.db".to_owned(),
            save_state: SaveState::Failed("Sin espacio".to_owned()),
            backup_notice: String::new(),
            pending_restore: None,
            busy: false,
            on_change: |_| {},
            on_pick_folder: |_| {},
            on_backup: |_| {},
            on_pick_restore: |_| {},
            on_confirm_restore: |_| {},
            on_cancel_restore: |_| {},
        }
    }
}

#[test]
fn shows_every_setting_with_its_saved_state() {
    let html = render(settings_view);

    for text in [
        "Apariencia",
        "Sistema",
        "Claro",
        "Oscuro",
        "Equipo",
        "Ana",
        "Mensaje de WhatsApp",
        "Vista previa",
        "Guardado",
        "Crear copia ahora",
        "Restaurar una copia",
        "/media/usb",
        "Copia creada",
        "/datos/crm.db",
    ] {
        assert!(html.contains(text), "missing {text}");
    }
    assert_eq!(html.matches("type=\"radio\"").count(), 3);
    assert_eq!(html.matches("checked=true").count(), 1);
    assert!(html.contains("days-unit"));
}

#[test]
fn shows_the_last_backup_time_in_peru() {
    assert_eq!(format_timestamp(1_793_999_400), "06/11/2026 16:10");
    assert!(render(settings_view).contains("Última copia: 06/11/2026 16:10"));
}

#[test]
fn requires_a_folder_before_backing_up_and_warns_about_the_same_disk() {
    let html = render(settings_without_backup_folder);

    assert!(html.contains("Aún no hay copias"));
    assert!(html.contains("disabled"));
    assert!(html.contains("USB"));
    assert!(html.contains("Sin espacio"));
    assert!(html.contains("todavía no agregaste"));
}

#[test]
fn asks_for_confirmation_before_restoring() {
    let html = render(settings_with_pending_restore);

    assert!(html.contains("/media/usb/crm.db"));
    assert!(html.contains("Reemplazar datos actuales"));
    assert!(html.contains("Cancelar"));
}

#[test]
fn keeps_a_removed_owner_selectable_without_duplicates() {
    let team = vec!["Ana".to_owned()];

    assert_eq!(owner_options(&team, Some("Ana")), ["Ana"]);
    assert_eq!(owner_options(&team, Some("Luis")), ["Ana", "Luis"]);
    assert_eq!(owner_options(&team, None), ["Ana"]);
}

#[test]
fn does_not_put_a_tooltip_on_the_whole_page() {
    assert!(!render(shell_view).contains("title=\"06/11/2026\""));
}

fn workspace_with_team() -> Element {
    rsx! {
        Workspace {
            leads: leads(),
            today: TODAY,
            query: String::new(),
            team: vec!["Ana".to_owned()],
            on_query: |_| {},
            on_new: |_| {},
            on_advance: |_| {},
            on_assign: |_| {},
        }
    }
}

#[test]
fn offers_the_responsible_person_on_each_card() {
    let html = render(workspace_with_team);

    assert_eq!(html.matches("Sin responsable").count(), 3);
    assert!(html.contains(">Ana<"));
}

#[test]
fn groups_the_agenda_by_urgency() {
    let leads = vec![
        sample("Atrasada", date!(2026 - 11 - 01), 0),
        sample("Hoy", TODAY, 1),
        sample("Semana", date!(2026 - 11 - 13), 0),
        sample("Lejana", date!(2026 - 11 - 14), 0),
        sample("Cerrada", date!(2026 - 11 - 01), 3),
    ];

    let groups = AgendaGroups::of(&leads, &[], TODAY);
    let names = |group: &[AgendaItem]| {
        group
            .iter()
            .map(|item| item.name().to_owned())
            .collect::<Vec<_>>()
    };

    assert_eq!(names(&groups.overdue), ["Atrasada"]);
    assert_eq!(names(&groups.today), ["Hoy"]);
    assert_eq!(names(&groups.upcoming), ["Semana"]);
}

fn agenda_view() -> Element {
    rsx! {
        Agenda { leads: leads(), today: TODAY, on_open_pipeline: |_| {} }
    }
}

fn empty_agenda() -> Element {
    rsx! {
        Agenda { leads: Vec::new(), today: TODAY, on_open_pipeline: |_| {} }
    }
}

#[test]
fn renders_the_agenda_and_its_empty_state() {
    let html = render(agenda_view);
    assert!(html.contains("Vencidos"));
    assert!(html.contains("Atrasada"));

    assert!(render(empty_agenda).contains("Nada pendiente"));
}

#[test]
fn computes_the_default_follow_up_date() {
    assert_eq!(next_follow_up(TODAY, 3), date!(2026 - 11 - 09));
}

#[test]
fn shows_the_brand_logo_in_the_sidebar() {
    let html = render(shell_view);

    assert!(html.contains("data:image/png;base64,"));
    assert!(html.contains("alt=\"COMINORSA\""));
}

#[test]
fn loads_the_embedded_window_icon() {
    assert!(window_icon().is_ok());
}
