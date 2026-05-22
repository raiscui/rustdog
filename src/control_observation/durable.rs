use super::{ObservationHeader, ObservationRefEntry, ObservationRoot};
use crate::control_observation::selector::{
    DurableSelectorRecord, PermanentSelector, SelectorKind,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

pub const DURABLE_STATE_SCHEMA: &str = "rdog.observation.state.v1";
pub const DURABLE_OBSERVATION_SCHEMA: &str = "rdog.observation.record.v1";
pub const DURABLE_REF_CACHE_SCHEMA: &str = "rdog.ref-cache.v1";
pub const DURABLE_INDEX_SCHEMA: &str = "rdog.observation.index.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableObservationIdentity {
    pub namespace: Option<String>,
    pub daemon_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableObservationPrivacy {
    pub persist_values: bool,
    pub persist_screenshots: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DurableObservationMeta {
    schema: String,
    daemon_name: String,
    namespace: Option<String>,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    privacy: DurableObservationPrivacy,
    retention: DurableObservationRetention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct DurableObservationRetention {
    observations: usize,
    bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableObservationArtifact {
    pub manifest_path: Option<String>,
    pub screenshot_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableObservationRecord {
    pub schema: String,
    pub observation_id: String,
    pub created_at_unix_ms: u64,
    pub ttl_ms: u64,
    pub scope: String,
    pub source_command: String,
    pub root: ObservationRoot,
    pub ref_count: usize,
    pub selector_count: usize,
    pub artifact: DurableObservationArtifact,
}

impl DurableObservationRecord {
    pub fn from_header(header: &ObservationHeader) -> Self {
        Self {
            schema: DURABLE_OBSERVATION_SCHEMA.to_owned(),
            observation_id: header.observation_id.clone(),
            created_at_unix_ms: header.created_at_unix_ms,
            ttl_ms: header.ttl_ms,
            scope: header.scope.clone(),
            source_command: header.source_command.clone(),
            root: header.root.clone(),
            ref_count: header.ref_count,
            selector_count: header.selector_count,
            artifact: DurableObservationArtifact {
                manifest_path: None,
                screenshot_path: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableRefCacheRecord {
    pub schema: String,
    pub observation_id: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub selector_id: Option<String>,
    pub backend_id_hint: String,
    pub kind: String,
    pub cache_lifetime: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableStateIndex {
    pub schema: String,
    pub updated_at_unix_ms: u64,
    #[serde(default)]
    pub observations: Vec<DurableIndexObservation>,
    #[serde(default)]
    pub selectors: Vec<DurableIndexSelector>,
}

impl DurableStateIndex {
    fn empty(now_ms: u64) -> Self {
        Self {
            schema: DURABLE_INDEX_SCHEMA.to_owned(),
            updated_at_unix_ms: now_ms,
            observations: Vec::new(),
            selectors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableIndexObservation {
    pub observation_id: String,
    pub created_at_unix_ms: u64,
    pub ref_count: usize,
    pub selector_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableIndexSelector {
    pub selector_id: String,
    pub fingerprint: String,
    pub observation_id: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub kind: SelectorKind,
    pub backend_id_hint: String,
    pub last_seen_unix_ms: u64,
    #[serde(default)]
    pub reobserve_commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permanent_selector: Option<PermanentSelector>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableSelectorHint {
    pub selector_hint_available: bool,
    pub selector_id: String,
    pub refind_available: bool,
    pub refind_command: String,
    pub recovery_recipe: Vec<String>,
    pub note: String,
    pub reobserve_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableSelectorLastSeen {
    pub selector_id: String,
    pub observation_id: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub kind: SelectorKind,
    pub backend_id_hint: String,
    pub last_seen_unix_ms: u64,
    pub reobserve_commands: Vec<String>,
}

impl DurableSelectorLastSeen {
    fn from_index_selector(selector: &DurableIndexSelector) -> Self {
        Self {
            selector_id: selector.selector_id.clone(),
            observation_id: selector.observation_id.clone(),
            ref_id: selector.ref_id.clone(),
            kind: selector.kind.clone(),
            backend_id_hint: selector.backend_id_hint.clone(),
            last_seen_unix_ms: selector.last_seen_unix_ms,
            reobserve_commands: selector.reobserve_commands.clone(),
        }
    }
}

#[derive(Debug)]
pub struct JsonlDurableObservationStore {
    state_dir: PathBuf,
    identity: DurableObservationIdentity,
    privacy: DurableObservationPrivacy,
    retention_observations: usize,
    retention_bytes: u64,
    write_ref_cache: bool,
    index: DurableStateIndex,
}

impl JsonlDurableObservationStore {
    pub fn open(
        state_dir: PathBuf,
        identity: DurableObservationIdentity,
        privacy: DurableObservationPrivacy,
        retention_observations: usize,
        retention_bytes: u64,
        write_ref_cache: bool,
        now_ms: u64,
    ) -> io::Result<Self> {
        fs::create_dir_all(state_dir.join("tmp"))?;
        let index = load_or_replay_index(&state_dir, now_ms)?;
        let mut store = Self {
            state_dir,
            identity,
            privacy,
            retention_observations,
            retention_bytes,
            write_ref_cache,
            index,
        };
        store.write_meta(now_ms)?;
        store.write_index(now_ms)?;
        Ok(store)
    }

    pub fn record_observation(
        &mut self,
        header: &ObservationHeader,
        refs: &[ObservationRefEntry],
        selectors: &[DurableSelectorRecord],
    ) -> io::Result<()> {
        let observation_record = DurableObservationRecord::from_header(header);
        append_jsonl(self.observations_path(), &observation_record)?;

        for selector in selectors {
            append_jsonl(self.selectors_path(), selector)?;
        }

        if self.write_ref_cache {
            for entry in refs {
                let selector_id = selectors
                    .iter()
                    .find(|selector| selector.ref_id == entry.ref_id)
                    .map(|selector| selector.selector_id.clone());
                let cache_record = DurableRefCacheRecord {
                    schema: DURABLE_REF_CACHE_SCHEMA.to_owned(),
                    observation_id: header.observation_id.clone(),
                    ref_id: entry.ref_id.clone(),
                    selector_id,
                    backend_id_hint: entry.backend_id.clone(),
                    kind: entry.kind.clone(),
                    cache_lifetime: "hint_only".to_owned(),
                };
                append_jsonl(self.ref_cache_path(), &cache_record)?;
            }
        }

        self.index.observations.push(DurableIndexObservation {
            observation_id: header.observation_id.clone(),
            created_at_unix_ms: header.created_at_unix_ms,
            ref_count: header.ref_count,
            selector_count: header.selector_count,
        });
        self.index
            .selectors
            .extend(selectors.iter().map(|selector| DurableIndexSelector {
                selector_id: selector.stable_selector_id(),
                fingerprint: selector.permanent_selector().fingerprint,
                observation_id: selector.observation_id.clone(),
                ref_id: selector.ref_id.clone(),
                kind: selector.kind.clone(),
                backend_id_hint: selector.backend_id_hint.clone(),
                last_seen_unix_ms: header.created_at_unix_ms,
                reobserve_commands: reobserve_commands_for_selector(selector),
                permanent_selector: Some(selector.permanent_selector()),
            }));
        self.prune_index();
        self.write_index(header.created_at_unix_ms)
    }

    pub fn selector_hint_for_ref(
        &self,
        observation_id: &str,
        ref_id: &str,
    ) -> Option<DurableSelectorHint> {
        if !self
            .index
            .observations
            .iter()
            .any(|observation| observation.observation_id == observation_id)
        {
            return None;
        }

        let selector = self.index.selectors.iter().find(|selector| {
            selector.observation_id == observation_id && selector.ref_id == ref_id
        })?;

        let reobserve_commands = if selector.reobserve_commands.is_empty() {
            vec![
                "@screenshot:{include_ax:true,ax_required:false,ax_mode:\"interactive\"}"
                    .to_owned(),
            ]
        } else {
            selector.reobserve_commands.clone()
        };
        let refind_command = selector_refind_command(&selector.selector_id, observation_id, ref_id);

        Some(DurableSelectorHint {
            selector_hint_available: true,
            selector_id: selector.selector_id.clone(),
            refind_available: true,
            refind_command: refind_command.clone(),
            recovery_recipe: vec![
                format!(
                    "@selector-get:{{selector_id:{},include_history:true}}",
                    json_string(&selector.selector_id)
                ),
                refind_command,
                "执行 verify_hint 后,再显式发送 @ax-action / @ax-set-value / @window-activate 等 side-effect 命令".to_owned(),
            ],
            note: "refind 只能恢复 fresh ref,不表示动作已经执行或验证成功".to_owned(),
            reobserve_commands,
        })
    }

    pub fn selector_by_id(&self, selector_id: &str) -> Option<PermanentSelector> {
        self.index
            .selectors
            .iter()
            .rev()
            .find(|selector| selector.selector_id == selector_id)
            .and_then(|selector| selector.permanent_selector.clone())
    }

    pub fn selector_last_seen(&self, selector_id: &str) -> Option<DurableSelectorLastSeen> {
        let selector = self
            .index
            .selectors
            .iter()
            .rev()
            .find(|selector| selector.selector_id == selector_id)?;
        Some(DurableSelectorLastSeen::from_index_selector(selector))
    }

    pub fn selector_history(
        &self,
        selector_id: &str,
        limit: usize,
    ) -> Vec<DurableSelectorLastSeen> {
        self.index
            .selectors
            .iter()
            .rev()
            .filter(|selector| selector.selector_id == selector_id)
            .take(limit)
            .map(DurableSelectorLastSeen::from_index_selector)
            .collect()
    }

    #[cfg(test)]
    pub fn index(&self) -> &DurableStateIndex {
        &self.index
    }

    fn prune_index(&mut self) {
        while self.index.observations.len() > self.retention_observations {
            self.index.observations.remove(0);
        }
        let retained = self
            .index
            .observations
            .iter()
            .map(|observation| observation.observation_id.as_str())
            .collect::<HashSet<_>>();
        let mut seen_stable = HashSet::<String>::new();
        self.index.selectors.retain(|selector| {
            if retained.contains(selector.observation_id.as_str()) {
                return true;
            }
            // P2 之后 stable selector metadata 不能因为 observation history
            // 淘汰而立刻消失。这里每个 selector_id 保留一条最近记录,
            // 但 `selector_hint_for_ref()` 仍只对 retained observation 生效。
            seen_stable.insert(selector.selector_id.clone())
        });
    }

    fn write_meta(&self, now_ms: u64) -> io::Result<()> {
        let meta = DurableObservationMeta {
            schema: DURABLE_STATE_SCHEMA.to_owned(),
            daemon_name: self.identity.daemon_name.clone(),
            namespace: self.identity.namespace.clone(),
            created_at_unix_ms: now_ms,
            updated_at_unix_ms: now_ms,
            privacy: self.privacy,
            retention: DurableObservationRetention {
                observations: self.retention_observations,
                bytes: self.retention_bytes,
            },
        };
        write_json_atomic(self.state_dir.join("meta.json"), &meta)
    }

    fn write_index(&mut self, now_ms: u64) -> io::Result<()> {
        self.index.updated_at_unix_ms = now_ms;
        write_json_atomic(self.index_path(), &self.index)
    }

    fn observations_path(&self) -> PathBuf {
        self.state_dir.join("observations.jsonl")
    }

    fn selectors_path(&self) -> PathBuf {
        self.state_dir.join("selectors.jsonl")
    }

    fn ref_cache_path(&self) -> PathBuf {
        self.state_dir.join("ref_cache.jsonl")
    }

    fn index_path(&self) -> PathBuf {
        self.state_dir.join("index.json")
    }
}

pub fn resolve_observation_state_dir(configured: Option<&Path>, daemon_name: &str) -> PathBuf {
    if let Some(path) = configured {
        return path.to_path_buf();
    }
    platform_default_observation_state_dir(daemon_name)
}

fn platform_default_observation_state_dir(daemon_name: &str) -> PathBuf {
    let daemon_name = sanitize_path_component(daemon_name);
    if cfg!(windows) {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rdog")
            .join("observations")
            .join(daemon_name)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("rdog")
            .join("observations")
            .join(daemon_name)
    } else {
        env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rdog")
            .join("observations")
            .join(daemon_name)
    }
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn append_jsonl<T: Serialize>(path: PathBuf, value: &T) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, value)
        .map_err(|err| io::Error::other(format!("durable observation JSONL 写入失败: {err}")))?;
    file.write_all(b"\n")?;
    file.flush()
}

fn write_json_atomic<T: Serialize>(path: PathBuf, value: &T) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("durable observation path 缺少 parent"))?;
    fs::create_dir_all(parent.join("tmp"))?;
    let tmp_path = parent.join("tmp").join(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("durable-observation")
    ));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| io::Error::other(format!("durable observation JSON 序列化失败: {err}")))?;
    fs::write(&tmp_path, bytes)?;
    fs::rename(tmp_path, path)
}

fn read_index(state_dir: &Path) -> io::Result<DurableStateIndex> {
    let bytes = fs::read(state_dir.join("index.json"))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
}

fn load_or_replay_index(state_dir: &Path, now_ms: u64) -> io::Result<DurableStateIndex> {
    match read_index(state_dir) {
        Ok(index) => Ok(index),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::InvalidData
            ) =>
        {
            Ok(replay_index(state_dir, now_ms))
        }
        Err(err) => Err(err),
    }
}

fn replay_index(state_dir: &Path, now_ms: u64) -> DurableStateIndex {
    let mut index = DurableStateIndex::empty(now_ms);
    let mut observation_times = HashMap::<String, u64>::new();
    if let Ok(records) =
        read_jsonl::<DurableObservationRecord>(state_dir.join("observations.jsonl"))
    {
        index.observations.extend(records.into_iter().map(|record| {
            observation_times.insert(record.observation_id.clone(), record.created_at_unix_ms);
            DurableIndexObservation {
                observation_id: record.observation_id,
                created_at_unix_ms: record.created_at_unix_ms,
                ref_count: record.ref_count,
                selector_count: record.selector_count,
            }
        }));
    }
    if let Ok(records) = read_jsonl::<DurableSelectorRecord>(state_dir.join("selectors.jsonl")) {
        index.selectors.extend(records.into_iter().map(|record| {
            DurableIndexSelector {
                reobserve_commands: reobserve_commands_for_selector(&record),
                selector_id: record.stable_selector_id(),
                fingerprint: record.permanent_selector().fingerprint,
                observation_id: record.observation_id.clone(),
                ref_id: record.ref_id.clone(),
                kind: record.kind.clone(),
                backend_id_hint: record.backend_id_hint.clone(),
                last_seen_unix_ms: observation_times
                    .get(&record.observation_id)
                    .copied()
                    .unwrap_or_default(),
                permanent_selector: Some(record.permanent_selector()),
            }
        }));
    }
    index
}

fn reobserve_commands_for_selector(selector: &DurableSelectorRecord) -> Vec<String> {
    match selector.kind {
        SelectorKind::Window | SelectorKind::AxWindow => vec![
            window_reobserve_command(&selector.selector),
            "@screenshot:{include_ax:true,ax_required:false,ax_mode:\"interactive\"}".to_owned(),
        ],
        SelectorKind::AxElement => vec![
            ax_reobserve_command(&selector.selector),
            "@screenshot:{include_ax:true,ax_required:false,ax_mode:\"interactive\"}".to_owned(),
        ],
    }
}

fn selector_refind_command(selector_id: &str, observation_id: &str, ref_id: &str) -> String {
    format!(
        "@selector-refind:{{selector_id:{},policy:\"safe\",include_explanations:true,source:{{observation_id:{},ref:{}}}}}",
        json_string(selector_id),
        json_string(observation_id),
        json_string(ref_id)
    )
}

fn window_reobserve_command(
    selector: &crate::control_observation::selector::SelectorEnvelope,
) -> String {
    let mut fields = Vec::new();
    if let Some(app) = selector.app.as_ref() {
        fields.push(format!("app:{}", json_string(&app.name)));
    }
    if let Some(title) = selector
        .window
        .as_ref()
        .and_then(|window| window.title.as_ref())
    {
        fields.push(format!("title_contains:{}", json_string(title)));
    }
    fields.push("limit:10".to_owned());
    fields.push("include_state:true".to_owned());
    format!("@window-find:{{{}}}", fields.join(","))
}

fn ax_reobserve_command(
    selector: &crate::control_observation::selector::SelectorEnvelope,
) -> String {
    let mut fields = Vec::new();
    if let Some(element) = selector.element.as_ref() {
        fields.push(format!("role:{}", json_string(&element.role)));
        if let Some(name) = element.name.as_ref() {
            fields.push(format!("name:{}", json_string(name)));
        }
        if let Some(description) = element.description.as_ref() {
            fields.push(format!("description:{}", json_string(description)));
        }
    }
    if fields.is_empty() {
        if let Some(window) = selector.window.as_ref() {
            fields.push(format!("role:{}", json_string(&window.role)));
        }
    }
    fields.push("limit:20".to_owned());
    format!("@ax-find:{{{}}}", fields.join(","))
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: PathBuf) -> io::Result<Vec<T>> {
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut values = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        values.push(serde_json::from_str(&line).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("durable observation JSONL 解析失败: {err}"),
            )
        })?);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
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
    fn jsonl_store_should_prune_index_without_compacting_jsonl() {
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
}
