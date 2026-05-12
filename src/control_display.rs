use std::io::{self, IsTerminal, Write};

/// control client 的本地显示策略。
///
/// 这里刻意只管“本机怎么显示”,不参与 wire protocol:
/// - `Protocol` 适合 pipe / redirect / 程序 stdio,保持原始 `@response ...`
/// - `HumanReadable` 适合人类直接在 TTY 输入,把简单成功值显示成正文
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ControlResponseDisplay {
    Protocol,
    HumanReadable,
}

impl ControlResponseDisplay {
    /// 根据当前进程 stdio 自动选择显示策略。
    ///
    /// 只有 stdin 和 stdout 同时是终端时才启用人类可读显示。
    /// 这样 `printf 'ls\n' | rdog control ...` 这类自动化仍能拿到稳定协议行。
    pub fn from_stdio() -> Self {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            Self::HumanReadable
        } else {
            Self::Protocol
        }
    }
}

/// 写出一条 `@response ...`。
///
/// 人类可读模式只解码最简单、最安全的成功值:
/// - `@response "a\nb\n"` -> 真实多行正文
/// - `@response 0` -> `0`
///
/// 带 request id 的对象、错误对象、复杂 shell 结果对象继续原样显示。
/// 这些对象里有诊断信息,不应该在显示层悄悄藏掉。
pub fn write_response_for_display<W: Write>(
    output: &mut W,
    response_line: &str,
    display: ControlResponseDisplay,
) -> io::Result<()> {
    match display {
        ControlResponseDisplay::Protocol => {
            writeln!(output, "{response_line}")?;
        }
        ControlResponseDisplay::HumanReadable => {
            write_human_response(output, response_line)?;
        }
    }

    output.flush()
}

fn write_human_response<W: Write>(output: &mut W, response_line: &str) -> io::Result<()> {
    let Some(payload) = response_line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("@response ")
    else {
        writeln!(output, "{response_line}")?;
        return Ok(());
    };

    let payload = payload.trim();
    if let Some(value) = decode_json_string_literal(payload) {
        output.write_all(value.as_bytes())?;
        if !value.ends_with('\n') {
            writeln!(output)?;
        }
        return Ok(());
    }

    if payload.parse::<i64>().is_ok() {
        writeln!(output, "{payload}")?;
        return Ok(());
    }

    writeln!(output, "{response_line}")?;
    Ok(())
}

fn decode_json_string_literal(input: &str) -> Option<String> {
    let mut chars = input.chars();
    if chars.next()? != '"' {
        return None;
    }

    let mut decoded = String::new();

    loop {
        let ch = chars.next()?;
        match ch {
            '"' => {
                if chars.as_str().trim().is_empty() {
                    return Some(decoded);
                }
                return None;
            }
            '\\' => decoded.push(decode_json_escape(&mut chars)?),
            ch if ch.is_control() => return None,
            ch => decoded.push(ch),
        }
    }
}

fn decode_json_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    match chars.next()? {
        '"' => Some('"'),
        '\\' => Some('\\'),
        '/' => Some('/'),
        'b' => Some('\u{08}'),
        'f' => Some('\u{0c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'u' => decode_json_unicode_escape(chars),
        _ => None,
    }
}

fn decode_json_unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    let mut value = 0_u32;

    for _ in 0..4 {
        let digit = chars.next()?.to_digit(16)?;
        value = value * 16 + digit;
    }

    char::from_u32(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(response_line: &str, display: ControlResponseDisplay) -> String {
        let mut output = Vec::new();
        write_response_for_display(&mut output, response_line, display)
            .expect("display should render");
        String::from_utf8(output).expect("display output should be utf-8")
    }

    #[test]
    fn protocol_display_should_keep_raw_response_line() {
        let output = render(
            r#"@response "Cargo.toml\nsrc\n""#,
            ControlResponseDisplay::Protocol,
        );

        assert_eq!(output, "@response \"Cargo.toml\\nsrc\\n\"\n");
    }

    #[test]
    fn human_display_should_decode_simple_string_response() {
        let output = render(
            r#"@response "Cargo.toml\nsrc\n""#,
            ControlResponseDisplay::HumanReadable,
        );

        assert_eq!(output, "Cargo.toml\nsrc\n");
    }

    #[test]
    fn human_display_should_add_newline_when_string_has_no_trailing_newline() {
        let output = render(r#"@response "pong""#, ControlResponseDisplay::HumanReadable);

        assert_eq!(output, "pong\n");
    }

    #[test]
    fn human_display_should_strip_prefix_from_numeric_success() {
        let output = render("@response 0", ControlResponseDisplay::HumanReadable);

        assert_eq!(output, "0\n");
    }

    #[test]
    fn human_display_should_keep_structured_error_raw() {
        let response = r#"@response {"code":64,"error":"bad"}"#;
        let output = render(response, ControlResponseDisplay::HumanReadable);

        assert_eq!(output, format!("{response}\n"));
    }

    #[test]
    fn human_display_should_keep_request_id_object_raw() {
        let response = r#"@response {"id":42,"value":"READY\n"}"#;
        let output = render(response, ControlResponseDisplay::HumanReadable);

        assert_eq!(output, format!("{response}\n"));
    }
}
