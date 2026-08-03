//! humantime three-piece parser for `rdog record start --duration`.
//!
//! 语法: `<integer>[.<fraction>]? <unit>?` where unit ∈ {`s`, `m`, `h`}.
//! - 整数部分必填, 小数部分可选 (允许在 s/m/h 之前).
//! - 数字和单位之间的空白可选 (`30s` / `30 s` / `1.5m` 全部接受).
//! - 单位大小写敏感 (只接受小写).
//! - 上下界校验独立于 parser, 见 `validate_duration_ms`.

use std::fmt;

/// humantime 解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumantimeError {
    /// 字符串为空。
    Empty,
    /// 缺少单位后缀 (无法判断 s/m/h)。
    MissingUnit,
    /// 单位不是 s / m / h 之一。
    UnknownUnit(String),
    /// 数字部分解析失败 (非数字字符)。
    InvalidNumber(String),
    /// 数值溢出 u64。
    Overflow,
}

impl fmt::Display for HumantimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("`--duration` 不能为空"),
            Self::MissingUnit => f.write_str("`--duration` 缺少单位 (s/m/h)"),
            Self::UnknownUnit(unit) => write!(f, "`--duration` 单位不支持: {unit} (只接受 s/m/h)"),
            Self::InvalidNumber(text) => write!(f, "`--duration` 数字部分非法: {text}"),
            Self::Overflow => f.write_str("`--duration` 数值溢出 u64"),
        }
    }
}

impl std::error::Error for HumantimeError {}

/// 把 humantime 字符串解析成毫秒数。
///
/// 例: `30s` → 30_000, `5m` → 300_000, `1h` → 3_600_000, `1.5m` → 90_000.
pub fn parse_humantime(input: &str) -> Result<u64, HumantimeError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(HumantimeError::Empty);
    }
    // 拆数字 + 单位, 允许数字和单位中间有空白。
    let (number_part, unit_part) = split_number_and_unit(trimmed)?;
    let unit_ms = match unit_part {
        "s" => 1_000_u64,
        "m" => 60_000_u64,
        "h" => 3_600_000_u64,
        other => return Err(HumantimeError::UnknownUnit(other.to_owned())),
    };
    let scaled = scale_number(&number_part, unit_ms)?;
    Ok(scaled)
}

fn split_number_and_unit(input: &str) -> Result<(String, &str), HumantimeError> {
    // 找最后一个 ASCII 字母字符, 数字 + 字母分界。
    let bytes = input.as_bytes();
    let mut last_digit_idx = None;
    for (idx, byte) in bytes.iter().enumerate() {
        if byte.is_ascii_digit() || *byte == b'.' {
            last_digit_idx = Some(idx);
        }
    }
    let split_at = last_digit_idx
        .and_then(|idx| idx.checked_add(1))
        .ok_or_else(|| HumantimeError::InvalidNumber(input.to_owned()))?;
    if split_at >= bytes.len() {
        return Err(HumantimeError::MissingUnit);
    }
    let number = input[..split_at].to_owned();
    let unit = &input[split_at..];
    let unit = unit.trim();
    if unit.is_empty() {
        return Err(HumantimeError::MissingUnit);
    }
    if !unit.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(HumantimeError::InvalidNumber(input.to_owned()));
    }
    Ok((number, unit))
}

fn scale_number(number: &str, unit_ms: u64) -> Result<u64, HumantimeError> {
    if let Some((whole, frac)) = number.split_once('.') {
        if frac.is_empty() {
            // "5." 视为整数 5 (clap-friendly).
            return scale_integer(whole, unit_ms);
        }
        if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err(HumantimeError::InvalidNumber(number.to_owned()));
        }
        // 用整数算小数部分, 避免浮点。
        // 精度限制: 小数部分最多 6 位 (微秒级), 超出截断。
        let frac_truncated: &str = if frac.len() > 6 { &frac[..6] } else { frac };
        let frac_len_u64 = frac_truncated.len() as u32;
        let whole_val: u64 = whole.parse().map_err(|_| HumantimeError::Overflow)?;
        let frac_val: u64 = if frac_truncated.is_empty() {
            0
        } else {
            frac_truncated.parse().map_err(|_| HumantimeError::Overflow)?
        };
        let whole_ms = whole_val.checked_mul(unit_ms).ok_or(HumantimeError::Overflow)?;
        // frac_ms = frac_val * unit_ms / 10^frac_len
        let divisor: u64 = 10_u64.checked_pow(frac_len_u64).ok_or(HumantimeError::Overflow)?;
        let frac_ms = frac_val
            .checked_mul(unit_ms)
            .and_then(|v| v.checked_div(divisor))
            .ok_or(HumantimeError::Overflow)?;
        whole_ms.checked_add(frac_ms).ok_or(HumantimeError::Overflow)
    } else {
        scale_integer(number, unit_ms)
    }
}

fn scale_integer(whole: &str, unit_ms: u64) -> Result<u64, HumantimeError> {
    if !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err(HumantimeError::InvalidNumber(whole.to_owned()));
    }
    let n: u64 = whole.parse().map_err(|_| HumantimeError::Overflow)?;
    n.checked_mul(unit_ms).ok_or(HumantimeError::Overflow)
}

/// 上下界校验, 跟 ticket #22 协议一致。
///
/// - `0` 合法
/// - `100 ms` 合法
/// - `1 hour` 合法
/// - 其它返 `DurationTooSmall` / `DurationTooLarge`
pub fn validate_duration_ms(value: u64) -> Result<(), DurationLimitError> {
    // 0 视为不传 duration 的同义词 (protocol layer 才会发 0, controller 直接当 manual).
    if value == 0 {
        return Ok(());
    }
    if value < 100 {
        return Err(DurationLimitError::TooSmall);
    }
    if value > 3_600_000 {
        return Err(DurationLimitError::TooLarge);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationLimitError {
    TooSmall,
    TooLarge,
}

impl fmt::Display for DurationLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall => f.write_str("DURATION_TOO_SMALL"),
            Self::TooLarge => f.write_str("DURATION_TOO_LARGE"),
        }
    }
}

impl std::error::Error for DurationLimitError {}
