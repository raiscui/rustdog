use std::{fs, process::Command};

use serde_json::json;

use super::bundle::{canonical_json, write_bundle, BundleError};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("rdog-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn canonical_json_sorts_nested_keys_and_has_one_lf() {
    let bytes = canonical_json(&json!({"z":{"b":2,"a":1},"a":0})).unwrap();
    assert_eq!(
        bytes,
        br#"{"a":0,"z":{"a":1,"b":2}}
"#
    );
}

#[test]
fn bundle_is_deterministic_atomic_and_standard_tar_readable() {
    let dir = temp_dir("bundle");
    let journal = dir.join("source.jsonl");
    fs::write(&journal, b"{\"schema\":\"rdog.recording.v1\"}\n").unwrap();
    let flow = json!({"steps":[],"schema":"rdog.flow.v1","policy":{},"compiler":{"version":"1","name":"rdog-replay-compiler"}});

    let first = write_bundle(&dir, "rec-1", 123, &journal, &flow, 0).unwrap();
    let first_bytes = fs::read(&first.path).unwrap();
    let second = write_bundle(&dir, "rec-1", 123, &journal, &flow, 0).unwrap();
    assert_eq!(first_bytes, fs::read(&second.path).unwrap());
    assert_eq!(first.size_bytes, first_bytes.len() as u64);
    assert_eq!(first.sha256, second.sha256);
    assert!(!dir.join(".rec-1.rdogrec.tar.staging").exists());

    let output = Command::new("tar")
        .args(["-tf", first.path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "manifest.json\njournal.jsonl\nflow.json\n"
    );
}

#[test]
fn unsafe_recording_id_is_rejected_without_partial_bundle() {
    let dir = temp_dir("unsafe-bundle");
    let journal = dir.join("source.jsonl");
    fs::write(&journal, b"{}\n").unwrap();
    let err = write_bundle(&dir, "../escape", 0, &journal, &json!({}), 0).unwrap_err();
    assert!(matches!(err, BundleError::UnsafePath(_)));
    assert!(!dir.join("escape.rdogrec.tar").exists());
}
