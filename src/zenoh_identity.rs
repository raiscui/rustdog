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

/// daemon 自启路径的 namespace 来源合并 (2026-10-06 拍板)。
///
/// 口径: **配置为主, 命令行只用于 control 场景"连别人"** ——
/// - `[zenoh].namespace` 与 `--namespace` 同时给出 = 显式冲突, 报错而非静默裁决
///   (谁赢都不对, 错配只会把人引去错误的寻址空间);
/// - 只有配置 → 用配置;
/// - 只有命令行 → 用命令行;
/// - 都没有 → 返回 None, 交由 `resolve_namespace` 落名字后缀推断。
///
/// 返回值语义上就是 resolve_namespace 的 explicit_namespace 入参 ——
/// 本函数只管来源合并, 校验与推断仍归 resolve_namespace 单一真相源。
pub fn merge_config_namespace(
    config_namespace: &str,
    cli_override: Option<&str>,
) -> io::Result<Option<String>> {
    let config_ns = config_namespace.trim();
    let cli_ns = cli_override.map(str::trim).filter(|value| !value.is_empty());

    if !config_ns.is_empty() {
        if let Some(cli) = cli_ns {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "[zenoh].namespace (`{config_ns}`) 与 --namespace (`{cli}`) 同时给出: \
                     daemon 自启以配置为主, --namespace 只用于 control 连别的 daemon, 请二选一"
                ),
            ));
        }
        return validate_namespace(config_ns).map(|_| Some(config_ns.to_string()));
    }
    Ok(cli_ns.map(str::to_string))
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

// ---------------------------------------------------------------------------
// agent messaging keyexpr (specs/rdog-agent-messaging-plan.md, Phase 3)
//
// agent 身份复用 daemon_name 的校验规则 (DNS 风格 label), 但命名空间层级
// 独立于 daemon: 一个 daemon 可以托管多个 agent。
// ---------------------------------------------------------------------------

/// `rdog/<ns>/agent/<name>/inbox` — 消息投递 (pub) 与补拉 (queryable)。
pub fn build_agent_inbox_key(namespace: &str, agent_name: &str) -> String {
    build_agent_inbox_key_with_root(KEYEXPR_ROOT, namespace, agent_name)
}

pub fn build_agent_inbox_key_with_root(root: &str, namespace: &str, agent_name: &str) -> String {
    format!("{root}/{namespace}/agent/{agent_name}/inbox")
}

/// `rdog/<ns>/agent/<name>/card` — 能力卡片托管 (pub + queryable)。
pub fn build_agent_card_key(namespace: &str, agent_name: &str) -> String {
    build_agent_card_key_with_root(KEYEXPR_ROOT, namespace, agent_name)
}

pub fn build_agent_card_key_with_root(root: &str, namespace: &str, agent_name: &str) -> String {
    format!("{root}/{namespace}/agent/{agent_name}/card")
}

/// `rdog/<ns>/agent/<name>/alive` — agent 在线状态 (liveliness, 对齐 daemon alive)。
pub fn build_agent_alive_key(namespace: &str, agent_name: &str) -> String {
    build_agent_alive_key_with_root(KEYEXPR_ROOT, namespace, agent_name)
}

pub fn build_agent_alive_key_with_root(root: &str, namespace: &str, agent_name: &str) -> String {
    format!("{root}/{namespace}/agent/{agent_name}/alive")
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

    // ── merge_config_namespace: 配置为主 + 冲突报错 (拍板 2026-10-06) ──

    #[test]
    fn merge_config_namespace_should_reject_config_and_cli_together() {
        let err = merge_config_namespace("lab", Some("prod"))
            .expect_err("config+CLI 同时给出必须报错, 不静默裁决");

        assert!(
            err.to_string().contains("二选一"),
            "错误信息要指出两个来源并给出可照做的指引: {err}"
        );
    }

    #[test]
    fn merge_config_namespace_should_prefer_config_when_cli_absent() {
        let merged = merge_config_namespace("lab", None).expect("config-only should merge");

        assert_eq!(merged.as_deref(), Some("lab"));
    }

    #[test]
    fn merge_config_namespace_should_pass_cli_through_when_config_empty() {
        let merged = merge_config_namespace("", Some("prod")).expect("cli-only should merge");

        assert_eq!(merged.as_deref(), Some("prod"));
    }

    #[test]
    fn merge_config_namespace_should_return_none_when_both_absent() {
        let merged = merge_config_namespace("", None).expect("both absent should merge");

        assert_eq!(merged, None, "两者皆空交给 resolve_namespace 落名字后缀推断");
    }

    #[test]
    fn merge_config_namespace_should_validate_config_value() {
        let err = merge_config_namespace("has space", None)
            .expect_err("配置里的非法 namespace 也要被校验拦下");

        assert!(err.to_string().contains("空白字符"));
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
    fn agent_keys_should_follow_agent_hierarchy() {
        assert_eq!(
            build_agent_inbox_key("lab", "helper-a.lab"),
            "rdog/lab/agent/helper-a.lab/inbox"
        );
        assert_eq!(
            build_agent_card_key("lab", "helper-a.lab"),
            "rdog/lab/agent/helper-a.lab/card"
        );
        assert_eq!(
            build_agent_alive_key("lab", "helper-a.lab"),
            "rdog/lab/agent/helper-a.lab/alive"
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
