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
    fs::create_dir_all(&dir).unwrap();
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

    // fresh store 只在内存里持有空 index,daemon 启动不应制造目录和空文件。
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);

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

    assert!(dir.join("meta.json").is_file());
    assert!(dir.join("index.json").is_file());
    assert!(dir.join("observations.jsonl").is_file());

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
fn dated_store_should_move_to_new_date_without_losing_history() {
    const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

    let root = temp_dir("durable-dated-move");
    let first_day = 1_700_000_000_000;
    let second_day = first_day + DAY_MS;
    let daemon_dir = sanitize_path_component(&identity().daemon_name);
    let first_path = root
        .join(date_component_from_unix_ms(first_day))
        .join(&daemon_dir);
    let second_path = root
        .join(date_component_from_unix_ms(second_day))
        .join(&daemon_dir);
    let mut store = JsonlDurableObservationStore::open_dated(
        root.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        first_day,
    )
    .unwrap();

    assert!(!root.exists());

    let mut first = header("obs-day-1", 1);
    first.created_at_unix_ms = first_day;
    store
        .record_observation(&first, &[], &[selector("obs-day-1", "@e1")])
        .unwrap();
    assert!(first_path.is_dir());

    let mut second = header("obs-day-2", 1);
    second.created_at_unix_ms = second_day;
    store
        .record_observation(&second, &[], &[selector("obs-day-2", "@e2")])
        .unwrap();

    assert!(!first_path.exists());
    assert!(second_path.is_dir());
    assert_eq!(store.index().observations.len(), 2);
    drop(store);

    let reopened = JsonlDurableObservationStore::open_dated(
        root.clone(),
        identity(),
        privacy(),
        16,
        10_000_000,
        true,
        second_day,
    )
    .unwrap();
    assert_eq!(reopened.index().observations.len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cleanup_should_remove_only_expired_inactive_dated_stores() {
    const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

    let root = temp_dir("durable-dated-cleanup");
    let now_ms = 20 * DAY_MS;
    let expired_ms = now_ms - 8 * DAY_MS;
    let boundary_ms = now_ms - 7 * DAY_MS;
    let expired_date = date_component_from_unix_ms(expired_ms);
    let boundary_date = date_component_from_unix_ms(boundary_ms);

    let make_store = |daemon_name: &str, created_at_unix_ms: u64| {
        let identity = DurableObservationIdentity {
            namespace: Some("test".to_owned()),
            daemon_name: daemon_name.to_owned(),
        };
        let mut store = JsonlDurableObservationStore::open_dated(
            root.clone(),
            identity,
            privacy(),
            16,
            10_000_000,
            true,
            created_at_unix_ms,
        )
        .unwrap();
        let mut observation = header(&format!("obs-{daemon_name}"), 0);
        observation.created_at_unix_ms = created_at_unix_ms;
        store.record_observation(&observation, &[], &[]).unwrap();
        store
    };

    drop(make_store("expired-inactive", expired_ms));
    let active_store = make_store("expired-active", expired_ms);
    drop(make_store("retention-boundary", boundary_ms));
    let unknown_dir = root.join(&expired_date).join("not-an-observation-store");
    fs::create_dir_all(unknown_dir.join("tmp")).unwrap();
    fs::write(unknown_dir.join("meta.json"), r#"{"schema":"unknown"}"#).unwrap();
    fs::write(unknown_dir.join("keep.txt"), "unknown data").unwrap();

    let first = cleanup_expired_default_observation_dirs(&root, now_ms, 7).unwrap();

    assert_eq!(first.removed_stores, 1);
    assert_eq!(first.skipped_active_stores, 1);
    assert_eq!(first.skipped_unknown_stores, 1);
    assert!(!root.join(&expired_date).join("expired-inactive").exists());
    assert!(root.join(&expired_date).join("expired-active").exists());
    assert_eq!(
        fs::read_to_string(unknown_dir.join("keep.txt")).unwrap(),
        "unknown data"
    );
    assert!(root
        .join(&boundary_date)
        .join("retention-boundary")
        .exists());

    drop(active_store);
    let second = cleanup_expired_default_observation_dirs(&root, now_ms, 7).unwrap();
    assert_eq!(second.removed_stores, 1);
    assert_eq!(second.skipped_unknown_stores, 1);
    assert!(root.join(expired_date).exists());
    assert!(!owner_lock_path(&root, "expired-inactive").exists());
    assert!(!owner_lock_path(&root, "expired-active").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unix_ms_date_component_should_handle_epoch_and_leap_day() {
    assert_eq!(date_component_from_unix_ms(0), "1970-01-01");
    assert_eq!(date_component_from_unix_ms(1_709_164_800_000), "2024-02-29");
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
        .record_observation(&old, &[ref_entry("@e1")], &[selector("obs-old", "@e1")])
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

#[test]
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
