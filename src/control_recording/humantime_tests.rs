//! humantime parser unit tests (TDD red-then-green).

use super::humantime::{parse_humantime, validate_duration_ms, DurationLimitError, HumantimeError};

#[test]
fn parses_seconds_integer() {
    assert_eq!(parse_humantime("30s").unwrap(), 30_000);
    assert_eq!(parse_humantime("1s").unwrap(), 1_000);
}

#[test]
fn parses_minutes_integer() {
    assert_eq!(parse_humantime("5m").unwrap(), 300_000);
}

#[test]
fn parses_hours_integer() {
    assert_eq!(parse_humantime("1h").unwrap(), 3_600_000);
}

#[test]
fn parses_fractional_minutes() {
    assert_eq!(parse_humantime("1.5m").unwrap(), 90_000);
    assert_eq!(parse_humantime("0.5h").unwrap(), 1_800_000);
}

#[test]
fn accepts_optional_whitespace_between_number_and_unit() {
    assert_eq!(parse_humantime("30 s").unwrap(), 30_000);
    assert_eq!(parse_humantime(" 5m ").unwrap(), 300_000);
}

#[test]
fn invalid_format_rejected() {
    assert_eq!(parse_humantime(""), Err(HumantimeError::Empty));
    assert_eq!(parse_humantime("30"), Err(HumantimeError::MissingUnit));
    assert_eq!(
        parse_humantime("30x"),
        Err(HumantimeError::UnknownUnit("x".to_owned()))
    );
    assert_eq!(
        parse_humantime("abc"),
        Err(HumantimeError::InvalidNumber("abc".to_owned()))
    );
}

#[test]
fn validate_accepts_zero_as_immediate_stop() {
    // ADR 0007 §6: 0 ms 合法, 等同"不传 duration"的 manual stop 路径。
    assert_eq!(validate_duration_ms(0), Ok(()));
}

#[test]
fn validate_accepts_lower_bound() {
    assert_eq!(validate_duration_ms(100), Ok(()));
}

#[test]
fn validate_accepts_upper_bound() {
    assert_eq!(validate_duration_ms(3_600_000), Ok(()));
}

#[test]
fn validate_rejects_too_small() {
    assert_eq!(validate_duration_ms(50), Err(DurationLimitError::TooSmall));
    assert_eq!(validate_duration_ms(99), Err(DurationLimitError::TooSmall));
}

#[test]
fn validate_rejects_too_large() {
    assert_eq!(
        validate_duration_ms(3_600_001),
        Err(DurationLimitError::TooLarge)
    );
}
