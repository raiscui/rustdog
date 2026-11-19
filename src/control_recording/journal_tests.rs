use super::{journal::JournalWriter, session::Profile, CaptureEvent, CapturedInput};

#[test]
fn journal_rejects_writes_past_budget_without_committing() {
    let dir = std::env::temp_dir().join(format!("rdog-journal-budget-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let id = format!("rec-{:?}", std::thread::current().id()).replace(['(', ')'], "");
    let mut writer = JournalWriter::open(&dir, &id, Profile::Physical, 1, 1500).unwrap();
    let mut failed = false;
    for sequence in 0..100 {
        let event = CaptureEvent {
            sequence,
            monotonic_ns: sequence,
            input: CapturedInput::SemanticMissing { for_sequence: 0 },
        };
        if writer.event(&event).is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed);
    drop(writer);
    assert!(!dir.join(format!("{id}.rdogrec.jsonl")).exists());
    assert!(!dir.join(format!(".{id}.staging")).exists());
}
#[test]
fn actual_write_error_does_not_commit_a_terminal_artifact() {
    let dir = std::env::temp_dir().join(format!(
        "rdog-journal-readonly-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut writer =
        JournalWriter::open(&dir, "rec-write-error", Profile::Physical, 1, 4096).unwrap();
    writer.replace_writer_with_read_only_for_test().unwrap();
    assert!(writer.seal().is_err());
    assert!(!dir.join("rec-write-error.rdogrec.jsonl").exists());
    assert!(!dir.join(".rec-write-error.staging").exists());
}

#[test]
fn marks_and_terminal_preserve_append_order_and_capture_time() {
    let dir = std::env::temp_dir().join(format!("rdog-journal-time-{}", uuid::Uuid::new_v4()));
    let mut writer = JournalWriter::open(&dir, "rec-time", Profile::Physical, 1, 4096).unwrap();
    writer
        .event(&CaptureEvent {
            sequence: 0,
            monotonic_ns: 20,
            input: CapturedInput::SemanticMissing { for_sequence: 0 },
        })
        .unwrap();
    writer.mark("checkpoint").unwrap();
    writer
        .event(&CaptureEvent {
            sequence: 1,
            monotonic_ns: 10,
            input: CapturedInput::SemanticMissing { for_sequence: 0 },
        })
        .unwrap();
    let sealed = writer.seal().unwrap();
    let entries: Vec<serde_json::Value> = std::fs::read_to_string(&sealed.path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(entries
        .windows(2)
        .all(|pair| pair[0]["monotonic_ns"].as_u64().unwrap()
            <= pair[1]["monotonic_ns"].as_u64().unwrap()));
    let physical: Vec<_> = entries
        .iter()
        .filter_map(|entry| entry["captured_monotonic_ns"].as_u64())
        .collect();
    assert_eq!(physical, vec![20, 10]);
    assert_eq!(entries[2]["kind"], "mark");
    assert_eq!(entries.last().unwrap()["kind"], "session_terminal");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn captured_facts_retain_their_canonical_event_families() {
    use super::model::{DisplayGeometry, NativeRect, WindowIdentity};
    let dir = std::env::temp_dir().join(format!("rdog-journal-families-{}", uuid::Uuid::new_v4()));
    let mut writer = JournalWriter::open(&dir, "rec-families", Profile::Semantic, 1, 4096).unwrap();
    let window = WindowIdentity {
        pid: 1,
        hwnd: 2,
        process_start: 3,
        generation: 4,
    };
    let inputs = [
        CapturedInput::MouseDown {
            window,
            x: 5,
            y: 6,
            button: 1,
            injected: false,
        },
        CapturedInput::DisplayContext {
            generation: 1,
            displays: vec![DisplayGeometry {
                id: 1,
                rect: NativeRect {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                primary: true,
            }],
        },
        CapturedInput::Redaction {
            window: Some(window),
            segment: 1,
            active: true,
        },
        CapturedInput::SemanticMissing { for_sequence: 0 },
    ];
    for (sequence, input) in inputs.into_iter().enumerate() {
        writer
            .event(&CaptureEvent {
                sequence: sequence as u64,
                monotonic_ns: sequence as u64,
                input,
            })
            .unwrap();
    }
    let artifact = writer.seal().unwrap();
    let entries: Vec<serde_json::Value> = std::fs::read_to_string(artifact.path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "session_start",
            "physical",
            "context",
            "redaction",
            "semantic",
            "session_terminal"
        ]
    );
    assert_eq!(entries[2]["payload"]["displays"][0]["rect"]["x"], -1920);
    std::fs::remove_dir_all(dir).unwrap();
}
