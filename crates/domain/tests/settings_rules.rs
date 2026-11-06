use cominorsa_domain::{DomainError, Settings, ThemePreference};

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
