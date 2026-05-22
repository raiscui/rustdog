use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionBridgeRequest {
    pub(super) session_id: Option<String>,
    pub(super) line: String,
}

pub(super) fn render_session_open_payload(session_id: &str) -> String {
    format!("__rdog_session_open__:{session_id}")
}

pub(super) fn render_session_close_payload(session_id: &str) -> String {
    format!("__rdog_session_close__:{session_id}")
}

pub(super) fn parse_session_open_payload(payload: &str) -> io::Result<Option<String>> {
    const PREFIX: &str = "__rdog_session_open__:";
    const LEGACY_PREFIX: &str = "__rcat_session_open__:";
    let trimmed = payload.trim();

    let Some(rest) = trimmed
        .strip_prefix(PREFIX)
        .or_else(|| trimmed.strip_prefix(LEGACY_PREFIX))
    else {
        return Ok(None);
    };

    let session_id = rest.trim();
    if session_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Zenoh session open payload 的 session_id 不能为空",
        ));
    }

    Ok(Some(session_id.to_owned()))
}

pub(super) fn parse_session_close_payload(payload: &str) -> io::Result<Option<String>> {
    const PREFIX: &str = "__rdog_session_close__:";
    const LEGACY_PREFIX: &str = "__rcat_session_close__:";
    let trimmed = payload.trim();

    let Some(rest) = trimmed
        .strip_prefix(PREFIX)
        .or_else(|| trimmed.strip_prefix(LEGACY_PREFIX))
    else {
        return Ok(None);
    };

    let session_id = rest.trim();
    if session_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Zenoh session close payload 的 session_id 不能为空",
        ));
    }

    Ok(Some(session_id.to_owned()))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn render_session_bridge_payload(session_id: &str, line: &str) -> String {
    format!("__rdog_session__:{session_id}\n{line}")
}

pub(super) fn parse_session_bridge_request(payload: &str) -> io::Result<SessionBridgeRequest> {
    const PREFIX: &str = "__rdog_session__:";
    const LEGACY_PREFIX: &str = "__rcat_session__:";

    if let Some(rest) = payload
        .strip_prefix(PREFIX)
        .or_else(|| payload.strip_prefix(LEGACY_PREFIX))
    {
        let Some((session_id, line)) = rest.split_once('\n') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Zenoh session bridge payload 缺少换行分隔的控制指令",
            ));
        };

        let session_id = session_id.trim();
        let line = line.trim();
        if session_id.is_empty() || line.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Zenoh session bridge payload 的 session_id 或 line 不能为空",
            ));
        }

        return Ok(SessionBridgeRequest {
            session_id: Some(session_id.to_owned()),
            line: line.to_owned(),
        });
    }

    Ok(SessionBridgeRequest {
        session_id: None,
        line: payload.trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_bridge_payload_should_roundtrip() {
        let payload = render_session_bridge_payload("sess-42", "@ping");
        let parsed = parse_session_bridge_request(&payload).expect("payload should parse");

        assert_eq!(
            parsed,
            SessionBridgeRequest {
                session_id: Some("sess-42".to_owned()),
                line: "@ping".to_owned(),
            }
        );
    }

    #[test]
    fn session_open_payload_should_roundtrip() {
        let payload = render_session_open_payload("sess-42");
        let parsed = parse_session_open_payload(&payload).expect("payload should parse");

        assert_eq!(parsed, Some("sess-42".to_owned()));
    }

    #[test]
    fn legacy_session_payloads_should_still_parse() {
        let open =
            parse_session_open_payload("__rcat_session_open__:sess-42").expect("payload parses");
        let close =
            parse_session_close_payload("__rcat_session_close__:sess-42").expect("payload parses");
        let bridge = parse_session_bridge_request("__rcat_session__:sess-42\n@ping")
            .expect("payload parses");

        assert_eq!(open, Some("sess-42".to_owned()));
        assert_eq!(close, Some("sess-42".to_owned()));
        assert_eq!(
            bridge,
            SessionBridgeRequest {
                session_id: Some("sess-42".to_owned()),
                line: "@ping".to_owned(),
            }
        );
    }

    #[test]
    fn session_close_payload_should_roundtrip() {
        let payload = render_session_close_payload("sess-42");
        let parsed = parse_session_close_payload(&payload).expect("payload should parse");

        assert_eq!(parsed, Some("sess-42".to_owned()));
    }
}
