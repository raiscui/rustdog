use std::io::{self, ErrorKind};

pub const KEYEXPR_ROOT: &str = "rdog";
pub const LEGACY_KEYEXPR_ROOT: &str = "rcat";

/// `daemon_name` 是当前 profile 唯一的人类稳定目标名。
///
/// 在当前不支持 HA / 同名多实例并存的前提下:
/// - 它既是人工寻址名
/// - 也是当前 static 模式下的唯一成员标识
/// - `member_id` 直接等于 `daemon_name`
pub fn validate_daemon_name(name: &str) -> io::Result<()> {
    if name.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.daemon_name 不能为空",
        ));
    }

    if name.len() > 128 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.daemon_name 总长度不能超过 128 个字符",
        ));
    }

    if name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.daemon_name 不能以 `.` 开头/结尾,也不能出现连续 `..`",
        ));
    }

    for label in name.split('.') {
        if label.is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "zenoh.daemon_name 不能包含空 label",
            ));
        }

        if label.len() > 63 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "zenoh.daemon_name 的单个 label 不能超过 63 个字符",
            ));
        }

        if label.starts_with('-') || label.ends_with('-') {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "zenoh.daemon_name 的 label 不能以 `-` 开头或结尾",
            ));
        }
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '.'))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.daemon_name 只允许小写字母、数字、`.` 和 `-`",
        ));
    }

    Ok(())
}

pub fn validate_namespace(namespace: &str) -> io::Result<()> {
    if namespace.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.namespace 不能为空",
        ));
    }

    if namespace.contains(char::is_whitespace) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "zenoh.namespace 不能包含空白字符",
        ));
    }

    Ok(())
}

pub fn infer_namespace_from_daemon_name(daemon_name: &str) -> Option<String> {
    let (_, suffix) = daemon_name.rsplit_once('.')?;
    if suffix.is_empty() {
        return None;
    }
    Some(suffix.to_string())
}

pub fn resolve_namespace(
    explicit_namespace: Option<&str>,
    daemon_name: Option<&str>,
) -> io::Result<String> {
    let explicit_namespace = explicit_namespace
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let inferred_namespace = daemon_name.and_then(infer_namespace_from_daemon_name);

    match (explicit_namespace, inferred_namespace) {
        (Some(explicit), Some(inferred)) if explicit != inferred => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "显式 --namespace `{explicit}` 与名字后缀推断出的 namespace `{inferred}` 不一致"
            ),
        )),
        (Some(explicit), _) => {
            validate_namespace(explicit)?;
            Ok(explicit.to_string())
        }
        (None, Some(inferred)) => {
            validate_namespace(&inferred)?;
            Ok(inferred)
        }
        (None, None) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "缺少 namespace,且无法从名字后缀推断,请显式传 --namespace 或使用带后缀的名字",
        )),
    }
}

pub fn member_id_from_daemon_name(daemon_name: &str) -> &str {
    daemon_name
}

pub fn build_alive_key(namespace: &str, daemon_name: &str) -> String {
    build_alive_key_with_root(KEYEXPR_ROOT, namespace, daemon_name)
}

pub fn build_alive_key_with_root(root: &str, namespace: &str, daemon_name: &str) -> String {
    let member_id = member_id_from_daemon_name(daemon_name);
    format!("{root}/{namespace}/daemon/{daemon_name}/member/{member_id}/alive")
}

pub fn build_control_key(namespace: &str, daemon_name: &str) -> String {
    build_control_key_with_root(KEYEXPR_ROOT, namespace, daemon_name)
}

pub fn build_control_key_with_root(root: &str, namespace: &str, daemon_name: &str) -> String {
    let member_id = member_id_from_daemon_name(daemon_name);
    format!("{root}/{namespace}/daemon/{daemon_name}/member/{member_id}/control")
}

pub fn build_key_input_key(namespace: &str, daemon_name: &str) -> String {
    build_key_input_key_with_root(KEYEXPR_ROOT, namespace, daemon_name)
}

pub fn build_key_input_key_with_root(root: &str, namespace: &str, daemon_name: &str) -> String {
    let member_id = member_id_from_daemon_name(daemon_name);
    format!("{root}/{namespace}/daemon/{daemon_name}/member/{member_id}/keyinput")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn build_session_root_key(namespace: &str, session_id: &str) -> String {
    build_session_root_key_with_root(KEYEXPR_ROOT, namespace, session_id)
}

pub fn build_session_root_key_with_root(root: &str, namespace: &str, session_id: &str) -> String {
    format!("{root}/{namespace}/session/{session_id}")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn build_session_to_daemon_key(namespace: &str, session_id: &str) -> String {
    build_session_to_daemon_key_with_root(KEYEXPR_ROOT, namespace, session_id)
}

pub fn build_session_to_daemon_key_with_root(
    root: &str,
    namespace: &str,
    session_id: &str,
) -> String {
    format!(
        "{}/to-daemon",
        build_session_root_key_with_root(root, namespace, session_id)
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn build_session_to_control_key(namespace: &str, session_id: &str) -> String {
    build_session_to_control_key_with_root(KEYEXPR_ROOT, namespace, session_id)
}

pub fn build_session_to_control_key_with_root(
    root: &str,
    namespace: &str,
    session_id: &str,
) -> String {
    format!(
        "{}/to-control",
        build_session_root_key_with_root(root, namespace, session_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_name_should_accept_domain_like_values() {
        validate_daemon_name("mini-a.lab").expect("daemon name should validate");
        validate_daemon_name("cam-01.office").expect("daemon name should validate");
    }

    #[test]
    fn daemon_name_should_reject_invalid_symbols() {
        let err = validate_daemon_name("Mini A").expect_err("daemon name should fail");
        assert!(err.to_string().contains("只允许小写字母"));
    }

    #[test]
    fn namespace_should_infer_from_last_label_of_daemon_name() {
        let namespace = resolve_namespace(None, Some("mini-a.lab"))
            .expect("namespace should infer from daemon name");

        assert_eq!(namespace, "lab");
    }

    #[test]
    fn explicit_namespace_should_have_to_match_name_suffix() {
        let err = resolve_namespace(Some("prod"), Some("mini-a.lab"))
            .expect_err("namespace mismatch should fail");

        assert!(err.to_string().contains("不一致"));
    }

    #[test]
    fn key_input_key_should_follow_same_identity_hierarchy() {
        let keyexpr = build_key_input_key("lab", "mini-a.lab");

        assert_eq!(
            keyexpr,
            "rdog/lab/daemon/mini-a.lab/member/mini-a.lab/keyinput"
        );
    }

    #[test]
    fn session_keys_should_follow_session_hierarchy() {
        let root = build_session_root_key("lab", "sess-42");
        let to_daemon = build_session_to_daemon_key("lab", "sess-42");
        let to_control = build_session_to_control_key("lab", "sess-42");

        assert_eq!(root, "rdog/lab/session/sess-42");
        assert_eq!(to_daemon, "rdog/lab/session/sess-42/to-daemon");
        assert_eq!(to_control, "rdog/lab/session/sess-42/to-control");
    }
}
