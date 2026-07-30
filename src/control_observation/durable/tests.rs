use super::*;
use crate::control_observation::selector::{
    AppSelector, SelectorEnvelope, SelectorRedaction, WindowSelector,
};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    env::temp_dir().join(format!("rdog-{name}-{nonce}"))
}

fn identity() -> DurableObservationIdentity {
    DurableObservationIdentity {
        namespace: Some("lab".to_owned()),
        daemon_name: "mini-a.lab".to_owned(),
    }
}

fn privacy() -> DurableObservationPrivacy {
    DurableObservationPrivacy {
        persist_values: false,
        persist_screenshots: false,
    }
}

fn header(id: &str, selector_count: usize) -> ObservationHeader {
    ObservationHeader {
        observation_id: id.to_owned(),
        session_id: None,
        created_at_unix_ms: 100,
        ttl_ms: 300_000,
        scope: "ax".to_owned(),
        source_command: "@ax-tree".to_owned(),
        root: ObservationRoot {
            schema: "rdog.ax.v1".to_owned(),
            platform: "macos".to_owned(),
            coordinate_space: "os-logical".to_owned(),
        },
        ref_count: 1,
        selector_count,
    }
}

fn ref_entry(ref_id: &str) -> ObservationRefEntry {
    ObservationRefEntry {
        ref_id: ref_id.to_owned(),
        backend_id: "pid:1/window:0".to_owned(),
        kind: "window".to_owned(),
    }
}

fn selector(observation_id: &str, ref_id: &str) -> DurableSelectorRecord {
    DurableSelectorRecord::new(
        observation_id,
        ref_id,
        SelectorKind::AxWindow,
        "pid:1/window:0",
        SelectorEnvelope {
            platform: "macos".to_owned(),
            app: Some(AppSelector {
                name: "System Settings".to_owned(),
                bundle_id: Some("com.apple.systempreferences".to_owned()),
                pid_hint: Some(1),
            }),
            window: Some(WindowSelector {
                title: Some("Storage".to_owned()),
                role: "AXWindow".to_owned(),
                rect: None,
            }),
            element: None,
            anchors: Vec::new(),
        },
        SelectorRedaction::metadata_only(),
    )
}

#[test]
fn jsonl_store_should_write_and_reload_index() {
    let dir = temp_dir("durable-reload");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        100,
    )
    .unwrap();
    let header = header("obs-1", 1);
    store
        .record_observation(
            &header,
            &[ObservationRefEntry {
                ref_id: "@e1".to_owned(),
                backend_id: "pid:1/window:0".to_owned(),
                kind: "window".to_owned(),
            }],
            &[selector("obs-1", "@e1")],
        )
        .unwrap();

    let reopened = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        101,
    )
    .unwrap();

    assert_eq!(reopened.index().observations.len(), 1);
    assert_eq!(reopened.index().selectors.len(), 1);
    assert!(reopened.selector_hint_for_ref("obs-1", "@e1").is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn jsonl_store_should_replay_when_index_is_corrupt() {
    let dir = temp_dir("durable-replay");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        100,
    )
    .unwrap();
    store
        .record_observation(&header("obs-1", 1), &[], &[selector("obs-1", "@e1")])
        .unwrap();
    fs::write(dir.join("index.json"), b"not-json").unwrap();

    let reopened = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        101,
    )
    .unwrap();

    assert_eq!(reopened.index().observations.len(), 1);
    assert_eq!(reopened.index().selectors.len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn jsonl_store_should_prune_index_by_observation_count() {
    let dir = temp_dir("durable-retention");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        1,
        10_000_000,
        true,
        100,
    )
    .unwrap();
    store
        .record_observation(&header("obs-1", 1), &[], &[selector("obs-1", "@e1")])
        .unwrap();
    store
        .record_observation(&header("obs-2", 1), &[], &[selector("obs-2", "@e1")])
        .unwrap();

    assert_eq!(store.index().observations.len(), 1);
    assert_eq!(store.index().observations[0].observation_id, "obs-2");
    assert!(store.selector_hint_for_ref("obs-1", "@e1").is_none());
    assert!(store.selector_hint_for_ref("obs-2", "@e1").is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn jsonl_store_should_compact_existing_state_to_byte_limit_and_replay() {
    let dir = temp_dir("durable-byte-retention");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        32,
        10_000_000,
        true,
        100,
    )
    .unwrap();
    let mut latest_selector_id = String::new();
    for index in 0..12 {
        let observation_id = format!("obs-{index}");
        let ref_id = format!("@e{index}");
        let mut item = selector(&observation_id, &ref_id);
        item.backend_id_hint = format!("pid:1/window:{index}");
        latest_selector_id = item.stable_selector_id();
        let mut observation = header(&observation_id, 1);
        observation.created_at_unix_ms = 100 + index;
        store
            .record_observation(&observation, &[ref_entry(&ref_id)], &[item])
            .unwrap();
    }
    let before = state_file_bytes(&dir).unwrap();
    let byte_limit = before / 2;
    drop(store);

    let compacted = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        32,
        byte_limit,
        true,
        200,
    )
    .unwrap();

    assert!(state_file_bytes(&dir).unwrap() <= byte_limit);
    assert!(compacted.index().observations.len() < 12);
    assert_eq!(
        compacted
            .index()
            .observations
            .last()
            .unwrap()
            .observation_id,
        "obs-11"
    );
    assert!(compacted.selector_by_id(&latest_selector_id).is_some());
    drop(compacted);

    fs::write(dir.join("index.json"), b"not-json").unwrap();
    let replayed = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        32,
        byte_limit,
        true,
        201,
    )
    .unwrap();
    assert!(state_file_bytes(&dir).unwrap() <= byte_limit);
    assert!(replayed.selector_by_id(&latest_selector_id).is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ref_cache_should_keep_first_selector_for_duplicate_ref() {
    let dir = temp_dir("durable-first-selector");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        100,
    )
    .unwrap();
    let mut first = selector("obs-1", "@e1");
    first.selector_id = "first-selector".to_owned();
    let mut second = selector("obs-1", "@e1");
    second.selector_id = "second-selector".to_owned();

    store
        .record_observation(&header("obs-1", 2), &[ref_entry("@e1")], &[first, second])
        .unwrap();

    let cache = read_jsonl::<DurableRefCacheRecord>(dir.join("ref_cache.jsonl")).unwrap();
    assert_eq!(cache.len(), 1);
    assert_eq!(cache[0].selector_id.as_deref(), Some("first-selector"));

    // 用可忽略空行制造超限,强制进入 compaction。重复 selector key 的记录基数
    // 必须保持为 2,不能被 HashSet 意外折叠成 1。
    let compacted_records = CompactionRecords::load(&dir, store.index(), true).unwrap();
    store.retention_bytes = compacted_state_bytes(store.index(), &compacted_records).unwrap();
    let mut selector_log = fs::OpenOptions::new()
        .append(true)
        .open(dir.join("selectors.jsonl"))
        .unwrap();
    selector_log.write_all(&vec![b'\n'; 4096]).unwrap();
    selector_log.flush().unwrap();
    drop(selector_log);
    store.enforce_byte_retention(101).unwrap();

    assert_eq!(store.index().selectors.len(), 2);
    assert_eq!(
        read_jsonl::<DurableSelectorRecord>(dir.join("selectors.jsonl"))
            .unwrap()
            .len(),
        2
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn record_observation_should_handle_two_thousand_ref_selector_pairs() {
    let dir = temp_dir("durable-scale");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        50_000_000,
        true,
        100,
    )
    .unwrap();
    let refs = (0..2_000)
        .map(|index| ref_entry(&format!("@e{index}")))
        .collect::<Vec<_>>();
    let selectors = (0..2_000)
        .map(|index| selector("obs-scale", &format!("@e{index}")))
        .collect::<Vec<_>>();

    store
        .record_observation(&header("obs-scale", selectors.len()), &refs, &selectors)
        .unwrap();

    assert_eq!(
        read_jsonl::<DurableSelectorRecord>(dir.join("selectors.jsonl"))
            .unwrap()
            .len(),
        2_000
    );
    assert_eq!(
        read_jsonl::<DurableRefCacheRecord>(dir.join("ref_cache.jsonl"))
            .unwrap()
            .len(),
        2_000
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
#[test]
fn prototype_observation_durable_selector_history_should_be_filtered_by_visibility() {
    // ponytail: 1 runnable check that documents the expected eviction contract from W-OS-02.
    // Currently `selector_history` returns rows by index order; this test is RED until
    // W-OS-04 implements `selector_visibility_ms` filtering in `enforce_byte_retention`.
    let dir = temp_dir("durable-visibility");
    let mut store = JsonlDurableObservationStore::open_with_visibility(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        30_000,
        100,
    )
    .unwrap();
    let mut old = header("obs-old", 1);
    old.created_at_unix_ms = 100;
    store
        .record_observation(
            &old,
            &[ref_entry("@e1")],
            &[selector("obs-old", "@e1")],
        )
        .unwrap();
    let mut recent = header("obs-recent", 1);
    recent.created_at_unix_ms = 195_000;
    store
        .record_observation(
            &recent,
            &[ref_entry("@e2")],
            &[selector("obs-recent", "@e2")],
        )
        .unwrap();
    // W-OS-02 contract: 触发一次 enforce_byte_retention 之后,`obs-old` (last_seen=100)
    // 应被 visibility (30_000 ms) 淘汰,只剩 `obs-recent` (last_seen=195_000) 保留。
    let _ = store.enforce_byte_retention_for_test(200_000);
    let history = store.selector_history(&selector("obs-old", "@e1").stable_selector_id(), 10);
    assert_eq!(
        history.len(),
        1,
        "W-OS-02: only the in-window selector row should remain: {history:?}"
    );
    assert_eq!(history[0].observation_id, "obs-recent");
    let _ = fs::remove_dir_all(dir);
}

fn prune_should_keep_latest_evicted_selector_record() {
    let dir = temp_dir("durable-latest-stable");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        1,
        10_000_000,
        false,
        100,
    )
    .unwrap();
    let selector_id = selector("obs-1", "@e1").stable_selector_id();
    for index in 1..=3 {
        let observation_id = format!("obs-{index}");
        let ref_id = format!("@e{index}");
        let mut observation = header(&observation_id, 1);
        observation.created_at_unix_ms = 100 + index;
        store
            .record_observation(&observation, &[], &[selector(&observation_id, &ref_id)])
            .unwrap();
    }

    let history = store.selector_history(&selector_id, 10);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].observation_id, "obs-3");
    assert_eq!(history[1].observation_id, "obs-2");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn jsonl_store_should_return_selector_history_by_stable_id() {
    let dir = temp_dir("durable-history");
    let mut store = JsonlDurableObservationStore::open(
        dir.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        100,
    )
    .unwrap();
    let first_selector = selector("obs-1", "@e1");
    let selector_id = first_selector.selector_id.clone();
    store
        .record_observation(&header("obs-1", 1), &[], &[first_selector])
        .unwrap();
    store
        .record_observation(&header("obs-2", 1), &[], &[selector("obs-2", "@e2")])
        .unwrap();

    let history = store.selector_history(&selector_id, 10);

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].observation_id, "obs-2");
    assert_eq!(history[0].ref_id, "@e2");
    assert_eq!(history[1].observation_id, "obs-1");
    assert_eq!(history[1].ref_id, "@e1");
    let _ = fs::remove_dir_all(dir);
}
