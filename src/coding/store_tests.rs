use super::*;
use crate::coding::model::{CodingRequest, ErrorCode, SessionMetadata};
use std::path::PathBuf;

fn fixture() -> (PathBuf, SessionMetadata) {
    let root = std::env::temp_dir().join(format!("rdog-coding-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let meta = SessionMetadata {
        session_id: "session-1".into(),
        host_instance_id: "host-1".into(),
        cursor_epoch: "epoch-1".into(),
        created_at_ms: 1,
        cwd: root.clone(),
        profile: "default".into(),
        omp_executable: root.join("omp"),
        extensions: vec![],
        max_journal_bytes: 1024 * 1024,
    };
    (root, meta)
}
#[test]
fn offline_queries_expose_host_storage_failure_without_changing_task_outcome() {
    let (root, meta) = fixture();
    let mut slot = super::super::failure::FailureSlot::create(&root, &meta.host_instance_id).unwrap();
    let mut store = Store::create(&root, meta.clone(), 1024 * 1024).unwrap();
    let submit = CodingRequest::TaskSubmit {
        session_id: meta.session_id.clone(),
        request_id: "submit".into(),
        prompt: "hello".into(),
    };
    let accepted = store.request(submit.clone()).unwrap().reply;
    let task_id = accepted["task_id"].as_str().unwrap().to_owned();
    slot.record(
        &meta.host_instance_id,
        &CodingError::new(ErrorCode::Storage, "disk full"),
    )
    .unwrap();
    drop(slot);
    drop(store);

    let session = Store::query_offline(
        &root,
        CodingRequest::SessionStatus { session_id: meta.session_id.clone() },
        meta.max_journal_bytes,
    )
    .unwrap();
    let task = Store::query_offline(
        &root,
        CodingRequest::TaskStatus { session_id: meta.session_id.clone(), task_id },
        meta.max_journal_bytes,
    )
    .unwrap();
    assert_eq!(session["host_error"]["code"], "storage");
    assert_eq!(task["host_error"]["message"], "disk full");
    assert!(task["result_status"].is_null());
    assert_eq!(Store::query_offline(&root, submit, meta.max_journal_bytes).unwrap(), accepted);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn host_failure_evidence_rejects_corruption_and_wrong_identity() {
    use std::io::{Seek, SeekFrom, Write};

    let (root, meta) = fixture();
    let mut slot = super::super::failure::FailureSlot::create(&root, &meta.host_instance_id).unwrap();
    drop(Store::create(&root, meta.clone(), meta.max_journal_bytes).unwrap());
    let status = || CodingRequest::SessionStatus {
        session_id: meta.session_id.clone(),
    };
    let clean = Store::query_offline(&root, status(), meta.max_journal_bytes).unwrap();
    assert!(clean.get("host_error").is_none());
    slot.record(
        &meta.host_instance_id,
        &CodingError::new(ErrorCode::Storage, "disk full"),
    )
    .unwrap();
    drop(slot);
    assert_eq!(
        super::super::failure::FailureSlot::read(&root, "different-host")
            .unwrap_err()
            .code,
        ErrorCode::HostIdentityMismatch
    );
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(root.join("host-failure.bin"))
        .unwrap();
    file.seek(SeekFrom::Start(36)).unwrap();
    file.write_all(b"!").unwrap();
    drop(file);
    assert_eq!(
        Store::query_offline(&root, status(), meta.max_journal_bytes)
            .unwrap_err()
            .code,
        ErrorCode::Storage
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_journal_reports_valid_host_storage_failure_without_replay() {
    use std::io::Write;

    let (root, meta) = fixture();
    let mut slot = super::super::failure::FailureSlot::create(&root, &meta.host_instance_id).unwrap();
    let mut store = Store::create(&root, meta.clone(), meta.max_journal_bytes).unwrap();
    let submit = CodingRequest::TaskSubmit {
        session_id: meta.session_id.clone(),
        request_id: "submit".into(),
        prompt: "hello".into(),
    };
    store.request(submit.clone()).unwrap();
    slot.record(
        &meta.host_instance_id,
        &CodingError::new(ErrorCode::Storage, "disk full"),
    )
    .unwrap();
    drop(slot);
    drop(store);
    let journal = root.join("journal.jsonl");
    let mut file = std::fs::OpenOptions::new().append(true).open(&journal).unwrap();
    file.write_all(b"{").unwrap();
    drop(file);
    let before = std::fs::read(&journal).unwrap();
    for request in [
        CodingRequest::SessionStatus { session_id: meta.session_id.clone() },
        CodingRequest::TaskStatus {
            session_id: meta.session_id.clone(),
            task_id: "unknown".into(),
        },
    ] {
        let failure = Store::query_offline(&root, request, meta.max_journal_bytes).unwrap_err();
        assert_eq!(failure.code, ErrorCode::JournalIncomplete);
        assert!(failure.message.contains("disk full"), "{}", failure.message);
    }
    assert_eq!(
        Store::query_offline(&root, submit, meta.max_journal_bytes)
            .unwrap_err()
            .code,
        ErrorCode::JournalIncomplete
    );
    let mut slot_file = std::fs::OpenOptions::new()
        .write(true)
        .open(root.join("host-failure.bin"))
        .unwrap();
    use std::io::{Seek, SeekFrom};
    slot_file.seek(SeekFrom::Start(36)).unwrap();
    slot_file.write_all(b"!").unwrap();
    drop(slot_file);
    let corrupted = Store::query_offline(
        &root,
        CodingRequest::SessionStatus { session_id: meta.session_id.clone() },
        meta.max_journal_bytes,
    )
    .unwrap_err();
    assert_eq!(corrupted.code, ErrorCode::JournalIncomplete);
    assert!(corrupted.message.contains("host failure evidence unavailable"));
    assert!(!corrupted.message.contains("disk full"));
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retry_precedes_busy_and_survives_reopen() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let request = CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "request-1".into(),
        prompt: "hello".into(),
    };
    let first = store.request(request.clone()).unwrap();
    assert!(first.effect.is_some());
    let retry = store.request(request.clone()).unwrap();
    assert_eq!(first.reply, retry.reply);
    assert!(retry.effect.is_none());
    let busy = CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "request-2".into(),
        prompt: "other".into(),
    };
    assert_eq!(store.request(busy).unwrap_err().code, ErrorCode::Busy);
    drop(store);
    let mut reopened = Store::open(&root, 1024 * 1024).unwrap();
    assert_eq!(reopened.request(request).unwrap().reply, first.reply);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_journal_is_not_silently_replayed() {
    use std::io::Write;
    let (root, meta) = fixture();
    drop(Store::create(&root, meta, 1024 * 1024).unwrap());
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join("journal.jsonl"))
        .unwrap();
    file.write_all(b"{").unwrap();
    assert_eq!(
        Store::read(&root, 1024 * 1024).unwrap_err().code,
        ErrorCode::JournalIncomplete
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn output_pages_and_decisions_survive_close() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let submit = CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "submit".into(),
        prompt: "hello".into(),
    };
    let accepted = store.request(submit.clone()).unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap().to_owned();
    store.output(&task, "text", &"界".repeat(1000)).unwrap();
    store
        .native_result(&task, "native", "completed", true, true, None)
        .unwrap();
    store.engine_state(true, false).unwrap();
    let first = store
        .request(CodingRequest::TaskOutput {
            session_id: "session-1".into(),
            task_id: task.clone(),
            cursor: None,
            limit: Some(1),
        })
        .unwrap()
        .reply;
    let second = store
        .request(CodingRequest::TaskOutput {
            session_id: "session-1".into(),
            task_id: task,
            cursor: Some(first["next_cursor"].as_str().unwrap().into()),
            limit: Some(1),
        })
        .unwrap()
        .reply;
    assert_eq!(first["entries"].as_array().unwrap().len(), 1);
    assert_eq!(second["entries"].as_array().unwrap().len(), 1);
    assert_ne!(first["entries"][0]["seq"], second["entries"][0]["seq"]);
    store.closed("test").unwrap();
    drop(store);
    assert_eq!(
        Store::query_offline(&root, submit, 1024 * 1024).unwrap(),
        accepted.reply
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn interaction_answer_is_decided_once_without_persisting_secret() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let result = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "submit".into(),
            prompt: "hello".into(),
        })
        .unwrap();
    let task = result.reply["task_id"].as_str().unwrap();
    store
        .interaction_open(task, "native-ui", "input", "Question", None, vec![])
        .unwrap();
    let interaction = store.state().interactions_json()["interactions"][0]["interaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = CodingRequest::InteractionRespond {
        session_id: "session-1".into(),
        interaction_id: interaction.clone(),
        request_id: "answer".into(),
        answer: InteractionAnswer::Input {
            value: Some("secret-answer".into()),
        },
    };
    assert!(store.request(answer.clone()).unwrap().effect.is_some());
    assert!(store.request(answer).unwrap().effect.is_none());
    let another = CodingRequest::InteractionRespond {
        session_id: "session-1".into(),
        interaction_id: interaction,
        request_id: "other".into(),
        answer: InteractionAnswer::Input {
            value: Some("other".into()),
        },
    };
    assert_eq!(
        store.request(another).unwrap_err().code,
        ErrorCode::InteractionClosed
    );
    assert!(!std::fs::read_to_string(root.join("journal.jsonl"))
        .unwrap()
        .contains("secret-answer"));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn writer_lease_and_busy_request_do_not_consume_retry_key() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    assert_eq!(
        Store::open(&root, 1024 * 1024).unwrap_err().code,
        ErrorCode::Busy
    );
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "one".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap();
    let second = CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "second".into(),
        prompt: "two".into(),
    };
    assert_eq!(
        store.request(second.clone()).unwrap_err().code,
        ErrorCode::Busy
    );
    store
        .native_result(task, "native", "completed", true, true, None)
        .unwrap();
    store.engine_state(true, false).unwrap();
    assert!(store.request(second).unwrap().effect.is_some());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn settled_without_abort_ack_does_not_confirm_cancellation() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let accepted = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "submit".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap().to_owned();
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    assert_eq!(store.claim_abort().unwrap().unwrap().task_id, task);
    store.engine_state(true, false).unwrap();
    assert_eq!(
        store.state().task_json(&task).unwrap()["status"],
        "cancelling"
    );
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.abort_ack(&task).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.engine_state(true, false).unwrap();
    assert_eq!(
        store.state().task_json(&task).unwrap()["status"],
        "cancelled"
    );
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn result_is_visible_but_old_settled_state_does_not_release_slot() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let accepted = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "submit".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap().to_owned();
    store.engine_state(true, false).unwrap();
    store
        .native_result(&task, "native", "completed", true, false, None)
        .unwrap();
    let result = store.state().task_json(&task).unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(result["settled"], false);
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store
        .unknown(&task, None, "transport closed after result")
        .unwrap();
    assert_eq!(
        store.state().task_json(&task).unwrap()["status"],
        "completed"
    );
    store.engine_state(true, true).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    let recovered = Store::read(&root, 1024 * 1024).unwrap();
    assert_eq!(recovered.task_json(&task).unwrap()["status"], "completed");
    assert_eq!(recovered.active_task_id(), None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_ui_cancel_cannot_be_overwritten_by_late_send_completion() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let accepted = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "submit".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap();
    store
        .interaction_open(task, "native-ui", "confirm", "Approval", None, vec![])
        .unwrap();
    let interaction = store.state().interactions_json()["interactions"][0]["interaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = CodingRequest::InteractionRespond {
        session_id: "session-1".into(),
        interaction_id: interaction.clone(),
        request_id: "answer".into(),
        answer: InteractionAnswer::Confirm { confirmed: false },
    };
    store.request(answer.clone()).unwrap();
    store.interaction_cancel("native-ui").unwrap();
    store.interaction_sent(&interaction).unwrap();
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelled"
    );
    assert!(store.request(answer).unwrap().effect.is_none());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn output_cursor_reconstructs_utf8_and_bounds_escaped_pages() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let accepted = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "submit".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap().to_owned();
    let expected = format!("{}{}", "界".repeat(4000), "\0".repeat(7000));
    store.output(&task, "text", &expected).unwrap();
    let mut cursor = None;
    let mut actual = String::new();
    loop {
        let page = store
            .request(CodingRequest::TaskOutput {
                session_id: "session-1".into(),
                task_id: task.clone(),
                cursor,
                limit: Some(128),
            })
            .unwrap()
            .reply;
        assert!(serde_json::to_vec(&page).unwrap().len() <= 16 * 1024);
        let entries = page["entries"].as_array().unwrap();
        if entries.is_empty() {
            break;
        }
        for entry in entries {
            actual.push_str(entry["text"].as_str().unwrap());
        }
        cursor = Some(page["next_cursor"].as_str().unwrap().to_owned());
    }
    assert_eq!(actual, expected);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_task_result_cannot_release_an_unresolved_abort() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let accepted = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "first".into(),
        })
        .unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap().to_owned();
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    assert_eq!(store.claim_abort().unwrap().unwrap().task_id, task);
    store
        .native_result(&task, "native-first", "completed", true, true, None)
        .unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    let next = CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "next".into(),
        prompt: "next".into(),
    };
    assert_eq!(
        store.request(next.clone()).unwrap_err().code,
        ErrorCode::Busy
    );
    store.abort_ack(&task).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.engine_state(true, false).unwrap();
    let next_task = store.request(next).unwrap();
    assert_eq!(next_task.reply["status"], "accepted");
    assert_ne!(next_task.reply["task_id"], task);
    assert_eq!(
        store.state().task_json(&task).unwrap()["status"],
        "completed"
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancel_after_native_result_preserves_the_result_without_abort() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let accepted = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "first".into(),
        })
        .unwrap();
    let task = accepted.reply["task_id"].as_str().unwrap().to_owned();
    store
        .native_result(&task, "native-first", "completed", true, false, None)
        .unwrap();
    let cancellation = store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    assert_eq!(cancellation.reply["status"], "completed");
    assert!(cancellation.effect.is_none());
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unclaimed_cancel_is_superseded_by_result_and_survives_reopen() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    for request_id in ["cancel-1", "cancel-2"] {
        let cancellation = store
            .request(CodingRequest::TaskCancel {
                session_id: "session-1".into(),
                task_id: task.clone(),
                request_id: request_id.into(),
            })
            .unwrap();
        assert!(cancellation.effect.is_none());
    }
    store
        .native_result(&task, "native", "completed", true, false, None)
        .unwrap();
    assert_eq!(store.claim_abort().unwrap(), None);
    store.engine_state(true, false).unwrap();
    drop(store);
    let mut store = Store::open(&root, 1024 * 1024).unwrap();
    assert_eq!(store.claim_abort().unwrap(), None);
    assert_eq!(
        store.state().task_json(&task).unwrap()["status"],
        "completed"
    );
    let next = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "next".into(),
            prompt: "next".into(),
        })
        .unwrap();
    assert_eq!(next.reply["status"], "accepted");
    assert_ne!(next.reply["task_id"], task);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dispatched_cancel_is_not_reclaimed_or_released_when_unknown() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    for request_id in ["cancel-1", "cancel-2"] {
        store
            .request(CodingRequest::TaskCancel {
                session_id: "session-1".into(),
                task_id: task.clone(),
                request_id: request_id.into(),
            })
            .unwrap();
    }
    assert_eq!(store.claim_abort().unwrap().unwrap().task_id, task);
    assert_eq!(store.claim_abort().unwrap(), None);
    store
        .unknown(&task, None, "abort acknowledgement timed out")
        .unwrap();
    store
        .native_result(&task, "native", "completed", true, true, None)
        .unwrap();
    assert_eq!(
        store.state().task_json(&task).unwrap()["uncertainty"],
        "abort acknowledgement timed out"
    );
    store.engine_state(true, false).unwrap();
    drop(store);
    let mut store = Store::open(&root, 1024 * 1024).unwrap();
    assert_eq!(store.claim_abort().unwrap(), None);
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    let next = store.request(CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "next".into(),
        prompt: "next".into(),
    });
    assert_eq!(next.unwrap_err().code, ErrorCode::Busy);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn prompt_timeout_keeps_native_id_for_late_result_and_reopen() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .unknown(&task, Some("allocated-id"), "acknowledgement timed out")
        .unwrap();
    drop(store);
    let mut store = Store::open(&root, 1024 * 1024).unwrap();
    assert_eq!(
        store.state().task_id_for_native("allocated-id"),
        Some(task.as_str())
    );
    store
        .native_result(&task, "allocated-id", "completed", true, false, None)
        .unwrap();
    assert_eq!(
        store.state().task_json(&task).unwrap()["status"],
        "completed"
    );
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_select_does_not_consume_decision_key() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap();
    store
        .interaction_open(task, "ui", "select", "Choose", None, vec!["yes".into()])
        .unwrap();
    let interaction = store.state().interactions_json()["interactions"][0]["interaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let answer = |value: &str| CodingRequest::InteractionRespond {
        session_id: "session-1".into(),
        interaction_id: interaction.clone(),
        request_id: "answer".into(),
        answer: InteractionAnswer::Select {
            value: Some(value.into()),
        },
    };
    assert_eq!(
        store.request(answer("no")).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "pending"
    );
    assert!(store.request(answer("yes")).unwrap().effect.is_some());
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "decided"
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancellation_claim_captures_only_pending_dialogs() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .interaction_open(&task, "ui", "confirm", "Approval", None, vec![])
        .unwrap();
    let interaction = store.state().interactions_json()["interactions"][0]["interaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    let work = store.claim_abort().unwrap().unwrap();
    assert_eq!(work.task_id, task);
    assert_eq!(work.dialogs, vec!["ui"]);
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelling"
    );
    let answer = CodingRequest::InteractionRespond {
        session_id: "session-1".into(),
        interaction_id: interaction,
        request_id: "answer".into(),
        answer: InteractionAnswer::Confirm { confirmed: false },
    };
    assert_eq!(
        store.request(answer).unwrap_err().code,
        ErrorCode::InteractionClosed
    );
    let next = CodingRequest::TaskSubmit {
        session_id: "session-1".into(),
        request_id: "next".into(),
        prompt: "next".into(),
    };
    store
        .unknown(&task, None, "abort acknowledgement timed out")
        .unwrap();
    assert_eq!(store.claim_abort().unwrap(), None);
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelling"
    );
    store.engine_state(true, false).unwrap();
    assert_eq!(
        store.request(next.clone()).unwrap_err().code,
        ErrorCode::Busy
    );
    store.abort_ack(&task).unwrap();
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelled"
    );
    assert_eq!(
        store.request(next.clone()).unwrap_err().code,
        ErrorCode::Busy
    );
    store.engine_state(true, false).unwrap();
    let accepted = store.request(next).unwrap();
    assert_eq!(accepted.reply["status"], "accepted");
    assert_ne!(accepted.reply["task_id"], task);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sent_select_cancellation_closes_dialog_before_fresh_release() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .interaction_open(&task, "ui", "select", "Choose", None, vec!["yes".into()])
        .unwrap();
    let interaction = store.state().interactions_json()["interactions"][0]["interaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .native_result(&task, "native", "completed", true, false, None)
        .unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    let answer = store
        .request(CodingRequest::InteractionRespond {
            session_id: "session-1".into(),
            interaction_id: interaction.clone(),
            request_id: "answer".into(),
            answer: InteractionAnswer::Select { value: None },
        })
        .unwrap();
    assert!(answer.effect.is_some());
    store.interaction_sent(&interaction).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.engine_state(true, false).unwrap();
    let next = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "next".into(),
            prompt: "next".into(),
        })
        .unwrap();
    assert_eq!(next.reply["status"], "accepted");
    assert_ne!(next.reply["task_id"], task);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn decided_dialog_is_not_claimed_or_closed_by_abort_ack() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .interaction_open(&task, "ui", "confirm", "Approval", None, vec![])
        .unwrap();
    let interaction = store.state().interactions_json()["interactions"][0]["interaction_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(store
        .request(CodingRequest::InteractionRespond {
            session_id: "session-1".into(),
            interaction_id: interaction.clone(),
            request_id: "answer".into(),
            answer: InteractionAnswer::Confirm { confirmed: true },
        })
        .unwrap()
        .effect
        .is_some());
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    let work = store.claim_abort().unwrap().unwrap();
    assert!(work.dialogs.is_empty());
    store.abort_ack(&task).unwrap();
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "decided"
    );
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.interaction_sent(&interaction).unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_dialog_cancellation_and_new_dialog_remain_independent() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .interaction_open(&task, "old-ui", "confirm", "Old", None, vec![])
        .unwrap();
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    assert_eq!(
        store.claim_abort().unwrap().unwrap().dialogs,
        vec!["old-ui"]
    );
    store.interaction_cancel("old-ui").unwrap();
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelled"
    );
    store
        .interaction_open(&task, "new-ui", "confirm", "New", None, vec![])
        .unwrap();
    store.abort_ack(&task).unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.interaction_cancel("new-ui").unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn late_dialog_cancel_waits_for_its_own_fresh_fence() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    assert!(store.claim_abort().unwrap().is_some());
    store
        .interaction_open(&task, "late-ui", "confirm", "Late", None, vec![])
        .unwrap();
    let (interaction, native) = store.claim_late_cancel().unwrap().unwrap();
    assert_eq!(native, "late-ui");
    store.abort_ack(&task).unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelling_late"
    );
    store.late_cancel_sent(&interaction).unwrap();
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store.engine_state(true, false).unwrap();
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelled"
    );
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_prompt_dialog_can_be_cancelled_before_prompt_ack() {
    let (root, meta) = fixture();
    let mut store = Store::create(&root, meta, 1024 * 1024).unwrap();
    let first = store
        .request(CodingRequest::TaskSubmit {
            session_id: "session-1".into(),
            request_id: "first".into(),
            prompt: "work".into(),
        })
        .unwrap();
    let task = first.reply["task_id"].as_str().unwrap().to_owned();
    store
        .request(CodingRequest::TaskCancel {
            session_id: "session-1".into(),
            task_id: task.clone(),
            request_id: "cancel".into(),
        })
        .unwrap();
    store
        .interaction_open(&task, "before-ack", "confirm", "Waiting", None, vec![])
        .unwrap();
    let (interaction, native) = store.claim_late_cancel().unwrap().unwrap();
    assert_eq!(native, "before-ack");
    store.interaction_cancel("before-ack").unwrap();
    store.late_cancel_sent(&interaction).unwrap();
    assert_eq!(
        store.state().interactions_json()["interactions"][0]["state"],
        "cancelled"
    );
    assert_eq!(store.state().active_task_id(), Some(task.as_str()));
    store
        .native_result(&task, "native", "completed", true, true, None)
        .unwrap();
    store.engine_state(true, false).unwrap();
    assert_eq!(store.state().active_task_id(), None);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
