use super::delivery::save_recording;
use crate::control_frames::{ControlFrame, SaveFileFrame};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use std::fs;

fn exchange(id: &str, bytes: &[u8]) -> Vec<ControlFrame> {
    let filename = format!("{id}.rdogrec.jsonl");
    let metadata = serde_json::json!({"schema":"rdog.record-control.v2","kind":"record-stop","phase":"recorded","recording_id":id,"artifact_filename":filename,"artifact_size_bytes":bytes.len(),"artifact_sha256":format!("{:x}", Sha256::digest(bytes))});
    vec![
        ControlFrame::SaveFile(SaveFileFrame {
            request_id: None,
            filename,
            mime: "application/x-ndjson".into(),
            encoding: "base64".into(),
            data: STANDARD.encode(bytes),
            quality: None,
            width: None,
            height: None,
        }),
        ControlFrame::ResponseLine(format!("@response {metadata}")),
    ]
}
fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rdog-delivery-test-{}", uuid::Uuid::new_v4()))
}

#[test]
fn verified_file_is_saved_without_overwriting_existing_file() {
    let dir = directory();
    let frames = exchange("rec-1", b"test\n");
    let path = save_recording(&frames, "rec-1", &dir).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"test\n");
    assert!(save_recording(&exchange("rec-1", b"other"), "rec-1", &dir).is_err());
    assert_eq!(fs::read(path).unwrap(), b"test\n");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn missing_frame_bad_hash_error_and_path_escape_never_commit() {
    let dir = directory();
    let frames = exchange("rec-1", b"test");
    assert!(save_recording(&frames[1..], "rec-1", &dir).is_err());
    let mut bad = frames.clone();
    if let ControlFrame::SaveFile(file) = &mut bad[0] {
        file.data = STANDARD.encode(b"evil");
    }
    assert!(save_recording(&bad, "rec-1", &dir).is_err());
    let mut error = frames.clone();
    error[1] = ControlFrame::ResponseLine("@response {\"code\":1,\"error\":\"failed\"}".into());
    assert!(save_recording(&error, "rec-1", &dir).is_err());
    let mut disguised_error = frames.clone();
    let ControlFrame::ResponseLine(success) = &frames[1] else {
        panic!("缺少成功响应 fixture");
    };
    let metadata: serde_json::Value =
        serde_json::from_str(success.trim_start_matches("@response ")).unwrap();
    disguised_error[1] = ControlFrame::ResponseLine(format!(
        "@response {}",
        serde_json::json!({"code":1,"value":metadata})
    ));
    assert!(save_recording(&disguised_error, "rec-1", &dir).is_err());
    assert!(save_recording(&exchange("../escape", b"x"), "../escape", &dir).is_err());
    assert!(!dir.exists());
}

#[test]
fn inaccessible_output_directory_reports_failure() {
    let path = directory();
    fs::write(&path, b"not a directory").unwrap();
    assert!(save_recording(&exchange("rec-1", b"test"), "rec-1", &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"not a directory");
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn recording_files_are_private_to_the_local_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = directory();
    let path = save_recording(&exchange("rec-private", b"recording"), "rec-private", &dir).unwrap();
    assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o077, 0);
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stop_errors_keep_recovery_details_without_exposing_raw_payload() {
    let dir = directory();
    let frames = vec![ControlFrame::ResponseLine(
        r#"@response {"code":4106,"error_code":"RECORD_STAGING_CLEANUP_FAILED","cleanup_residual_path":"private-staging","error":"private-canary"}"#.into()
    )];
    let error = save_recording(&frames, "rec-failed", &dir)
        .unwrap_err()
        .to_string();
    assert!(error.contains("RECORD_STAGING_CLEANUP_FAILED"));
    assert!(error.contains("private-staging"));
    assert!(!error.contains("private-canary"));
    assert!(!dir.exists());
}
