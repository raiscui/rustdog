//! `rdog record` CLI dispatcher 的单元测试。
//!
//! 不连 daemon, 只覆盖 `render_line` 的 line-control 文本生成。

use super::cli::{RecordCommand, render_line};

#[test]
fn start_default_profile_emits_semantic() {
    let line = render_line(&RecordCommand::Start { profile: "semantic".to_owned() }).unwrap();
    assert_eq!(line, "@record-start:{\"profile\":\"semantic\"}");
}

#[test]
fn start_rejects_unknown_profile() {
    let err = render_line(&RecordCommand::Start { profile: "x".to_owned() }).unwrap_err();
    assert!(err.contains("profile"));
}

#[test]
fn status_emits_bare_kind() {
    let line = render_line(&RecordCommand::Status).unwrap();
    assert_eq!(line, "@record-status");
}

#[test]
fn mark_with_label_and_redaction_active() {
    let line = render_line(&RecordCommand::Mark { label: Some("step-1".to_owned()), redaction_active: true }).unwrap();
    assert_eq!(line, "@record-mark:{\"redaction_active\":true,\"label\":\"step-1\"}");
}

#[test]
fn mark_without_label_omits_field() {
    let line = render_line(&RecordCommand::Mark { label: None, redaction_active: false }).unwrap();
    assert_eq!(line, "@record-mark:{\"redaction_active\":false}");
}

#[test]
fn mark_escapes_quote_in_label() {
    let line = render_line(&RecordCommand::Mark { label: Some("a\"b".to_owned()), redaction_active: false }).unwrap();
    assert!(line.contains("a\\\"b"));
}

#[test]
fn stop_and_cancel_emit_empty_object() {
    assert_eq!(render_line(&RecordCommand::Stop).unwrap(), "@record-stop:{}");
    assert_eq!(render_line(&RecordCommand::Cancel).unwrap(), "@record-cancel:{}");
}
