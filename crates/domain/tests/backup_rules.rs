use cominorsa_domain::{DomainError, Settings};
use time::{Date, macros::date};

const TODAY: Date = date!(2026 - 03 - 02);

#[test]
fn automatic_backups_default_to_daily_keeping_fourteen() {
    let settings = Settings::default();

    assert_eq!(settings.backup_interval_days(), 1);
    assert_eq!(settings.backup_keep(), 14);
}

#[test]
fn limits_the_interval_to_one_through_thirty_days() {
    let mut settings = Settings::default();

    for valid in [1, 30] {
        settings.set_backup_interval_days(valid).unwrap();
        assert_eq!(settings.backup_interval_days(), valid);
    }
    for invalid in [0, 31] {
        assert_eq!(
            settings.set_backup_interval_days(invalid),
            Err(DomainError::InvalidBackupInterval)
        );
    }
    assert_eq!(settings.backup_interval_days(), 30);
}

#[test]
fn limits_the_kept_copies_to_one_through_ninety() {
    let mut settings = Settings::default();

    for valid in [1, 90] {
        settings.set_backup_keep(valid).unwrap();
        assert_eq!(settings.backup_keep(), valid);
    }
    for invalid in [0, 91] {
        assert_eq!(
            settings.set_backup_keep(invalid),
            Err(DomainError::InvalidBackupKeep)
        );
    }
    assert_eq!(settings.backup_keep(), 90);
}

#[test]
fn restores_stored_values_and_falls_back_when_they_are_unreadable() {
    let mut settings = Settings::default();
    settings.restore_backup_interval_days("7");
    settings.restore_backup_keep("30");
    assert_eq!(settings.backup_interval_days(), 7);
    assert_eq!(settings.backup_keep(), 30);

    let mut fallback = Settings::default();
    fallback.restore_backup_interval_days("45");
    fallback.restore_backup_keep("muchas");
    assert_eq!(fallback.backup_interval_days(), 1);
    assert_eq!(fallback.backup_keep(), 14);
}

#[test]
fn no_backup_is_due_without_a_folder() {
    let settings = Settings::default();

    assert!(!settings.backup_due(TODAY, None));
    assert!(!settings.backup_due(TODAY, Some(date!(2026 - 01 - 01))));
    assert_eq!(settings.next_backup_on(Some(TODAY)), None);
}

#[test]
fn a_first_backup_is_due_as_soon_as_a_folder_is_set() {
    let mut settings = Settings::default();
    settings.set_backup_directory("/media/copias");

    assert!(settings.backup_due(TODAY, None));
    assert_eq!(settings.next_backup_on(None), None);
}

#[test]
fn a_daily_backup_is_due_on_the_next_calendar_day_whatever_the_hour() {
    let mut settings = Settings::default();
    settings.set_backup_directory("/media/copias");

    assert!(!settings.backup_due(TODAY, Some(TODAY)));
    assert!(settings.backup_due(date!(2026 - 03 - 03), Some(TODAY)));
    assert_eq!(
        settings.next_backup_on(Some(TODAY)),
        Some(date!(2026 - 03 - 03))
    );
}

#[test]
fn a_backup_is_due_once_the_interval_in_whole_days_has_passed() {
    let mut settings = Settings::default();
    settings.set_backup_directory("/media/copias");
    settings.set_backup_interval_days(2).unwrap();

    assert!(!settings.backup_due(TODAY, Some(TODAY)));
    assert!(!settings.backup_due(date!(2026 - 03 - 03), Some(TODAY)));
    assert!(settings.backup_due(date!(2026 - 03 - 04), Some(TODAY)));
    assert_eq!(
        settings.next_backup_on(Some(TODAY)),
        Some(date!(2026 - 03 - 04))
    );
}

#[test]
fn a_last_backup_dated_after_today_never_blocks_a_copy() {
    let mut settings = Settings::default();
    settings.set_backup_directory("/media/copias");

    assert!(settings.backup_due(TODAY, Some(date!(2099 - 01 - 01))));
}
