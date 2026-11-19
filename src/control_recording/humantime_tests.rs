//! humantime parser unit tests (TDD red-then-green).

use super::humantime::{parse_humantime, HumantimeError};

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
