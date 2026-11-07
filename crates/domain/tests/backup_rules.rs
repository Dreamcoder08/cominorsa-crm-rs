use cominorsa_domain::{DomainError, Settings};

const DAY: i64 = 86_400;
const LAST: i64 = 1_772_470_800;

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

    assert!(!settings.backup_due(LAST));
    assert_eq!(settings.next_backup_due_at(), None);
}

#[test]
fn a_first_backup_is_due_as_soon_as_a_folder_is_set() {
    let mut settings = Settings::default();
    settings.set_backup_directory("copias");

    assert!(settings.backup_due(LAST));
    assert_eq!(settings.next_backup_due_at(), None);
}

#[test]
fn a_backup_is_due_only_when_the_last_one_is_older_than_the_interval() {
    let mut settings = Settings::default();
    settings.set_backup_directory("copias");
    settings.set_backup_interval_days(2).unwrap();
    settings.record_backup(LAST);

    assert!(!settings.backup_due(LAST + DAY));
    assert!(!settings.backup_due(LAST + 2 * DAY));
    assert!(settings.backup_due(LAST + 2 * DAY + 1));
    assert_eq!(settings.next_backup_due_at(), Some(LAST + 2 * DAY));
}
