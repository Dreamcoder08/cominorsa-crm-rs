use cominorsa_domain::{CompanyProfile, DomainError, IgvRate, Settings, ThemePreference};

#[test]
fn starts_with_system_theme_and_three_follow_up_days() {
    let settings = Settings::default();

    assert_eq!(settings.theme, ThemePreference::System);
    assert_eq!(settings.follow_up_days(), 3);
    assert!(settings.team().is_empty());
}

#[test]
fn theme_codes_round_trip() {
    for theme in ThemePreference::ALL {
        assert_eq!(ThemePreference::from_code(theme.code()), Some(theme));
    }
    assert_eq!(ThemePreference::from_code("sepia"), None);
}

#[test]
fn adds_trimmed_members_and_rejects_blank_or_repeated_names() {
    let mut settings = Settings::default();

    settings.add_member("  Ana  ").unwrap();

    assert_eq!(settings.team(), ["Ana"]);
    assert_eq!(
        settings.add_member("   "),
        Err(DomainError::MissingField("nombre"))
    );
    assert_eq!(
        settings.add_member("ana"),
        Err(DomainError::DuplicateMember)
    );
    assert_eq!(settings.team(), ["Ana"]);
}

#[test]
fn removes_members() {
    let mut settings = Settings::default();
    settings.add_member("Ana").unwrap();
    settings.add_member("Luis").unwrap();

    settings.remove_member("Ana");

    assert_eq!(settings.team(), ["Luis"]);
}

#[test]
fn limits_follow_up_days_to_one_through_ninety() {
    let mut settings = Settings::default();

    settings.set_follow_up_days(1).unwrap();
    settings.set_follow_up_days(90).unwrap();
    assert_eq!(settings.follow_up_days(), 90);

    for invalid in [0, 91] {
        assert_eq!(
            settings.set_follow_up_days(invalid),
            Err(DomainError::InvalidFollowUpDays)
        );
    }
    assert_eq!(settings.follow_up_days(), 90);
}

#[test]
fn records_the_backup_folder_and_last_backup() {
    let mut settings = Settings::default();
    assert_eq!(settings.backup_directory(), None);
    assert_eq!(settings.last_backup_at(), None);

    settings.set_backup_directory("  /media/usb/copias  ");
    settings.record_backup(1_800_000_000);

    assert_eq!(settings.backup_directory(), Some("/media/usb/copias"));
    assert_eq!(settings.last_backup_at(), Some(1_800_000_000));
}

fn company() -> CompanyProfile {
    CompanyProfile {
        name: "  Empresa Ejemplo SAC ".to_owned(),
        ruc: " 20123456786 ".to_owned(),
        address: " Av. Ejemplo 123, Piura ".to_owned(),
        phone: " 900000000 ".to_owned(),
        email: " ventas@example.com ".to_owned(),
    }
}

#[test]
fn the_company_profile_starts_blank_and_invents_nothing() {
    let settings = Settings::default();

    assert_eq!(settings.company(), &CompanyProfile::default());
    assert!(!settings.company().has_name());
}

#[test]
fn saves_a_trimmed_company_profile() {
    let mut settings = Settings::default();

    settings.set_company(company()).unwrap();

    assert_eq!(settings.company().name, "Empresa Ejemplo SAC");
    assert_eq!(settings.company().ruc, "20123456786");
    assert_eq!(settings.company().address, "Av. Ejemplo 123, Piura");
    assert_eq!(settings.company().phone, "900000000");
    assert_eq!(settings.company().email, "ventas@example.com");
    assert!(settings.company().has_name());
}

#[test]
fn a_name_of_spaces_counts_as_no_name() {
    let mut settings = Settings::default();

    settings
        .set_company(CompanyProfile {
            name: "   ".to_owned(),
            ..CompanyProfile::default()
        })
        .unwrap();

    assert!(!settings.company().has_name());
}

#[test]
fn a_blank_company_ruc_is_allowed() {
    let mut settings = Settings::default();

    assert_eq!(settings.set_company(CompanyProfile::default()), Ok(()));
}

#[test]
fn refuses_an_invalid_company_ruc_and_keeps_the_previous_profile() {
    let mut settings = Settings::default();
    settings.set_company(company()).unwrap();
    let before = settings.company().clone();

    let refused = settings.set_company(CompanyProfile {
        ruc: "20123456780".to_owned(),
        ..company()
    });

    assert_eq!(refused, Err(DomainError::InvalidRuc));
    assert_eq!(settings.company(), &before);
}

#[test]
fn restores_a_stored_company_without_blocking_on_an_old_value() {
    let mut settings = Settings::default();
    let stored = CompanyProfile {
        ruc: "123".to_owned(),
        ..CompanyProfile::default()
    };

    settings.restore_company(stored);

    assert_eq!(settings.company().ruc, "123");
}

#[test]
fn the_igv_rate_defaults_to_eighteen_percent_and_can_change() {
    let mut settings = Settings::default();
    assert_eq!(settings.igv_rate().basis_points(), 1800);

    settings.set_igv_rate(IgvRate::parse("10.5").unwrap());

    assert_eq!(settings.igv_rate().basis_points(), 1050);
}

#[test]
fn restores_a_stored_igv_rate_and_falls_back_when_it_is_unreadable() {
    let mut settings = Settings::default();

    settings.restore_igv_rate("1050");
    assert_eq!(settings.igv_rate().basis_points(), 1050);

    for unreadable in ["abc", "20000", ""] {
        let mut settings = Settings::default();
        settings.restore_igv_rate(unreadable);
        assert_eq!(settings.igv_rate().basis_points(), 1800, "{unreadable:?}");
    }
}
