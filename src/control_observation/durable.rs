use super::{ObservationHeader, ObservationRefEntry, ObservationRoot};
use crate::control_observation::selector::{
    DurableSelectorRecord, PermanentSelector, SelectorKind,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, BufRead, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const DURABLE_STATE_SCHEMA: &str = "rdog.observation.state.v1";
pub const DURABLE_OBSERVATION_SCHEMA: &str = "rdog.observation.record.v1";
pub const DURABLE_REF_CACHE_SCHEMA: &str = "rdog.ref-cache.v1";
pub const DURABLE_INDEX_SCHEMA: &str = "rdog.observation.index.v1";

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
const MAINTENANCE_LOCK_NAME: &str = ".maintenance.lock";
const OWNER_DIR_NAME: &str = ".owners";

/// Wayfinder Destination B: 默认 5 分钟,与 `ttl_ms` 数量级一致。
pub const DEFAULT_SELECTOR_VISIBILITY_MS: u64 = 5 * 60_000;

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
    layout: DurableStoreLayout,
    identity: DurableObservationIdentity,
    privacy: DurableObservationPrivacy,
    retention_observations: usize,
    retention_bytes: u64,
    selector_visibility_ms: u64,
    write_ref_cache: bool,
    index: DurableStateIndex,
}

#[derive(Debug)]
enum DurableStoreLayout {
    Exact,
    Dated {
        root_dir: PathBuf,
        daemon_dir_name: String,
        owner_lock: Option<File>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObservationCleanupReport {
    pub scanned_date_dirs: usize,
    pub removed_date_dirs: usize,
    pub removed_stores: usize,
    pub skipped_active_stores: usize,
    pub skipped_unknown_stores: usize,
}

/// 默认 observation root 的定时清理任务。
///
/// `Condvar` 让 Drop 可以立即唤醒线程,daemon 退出时不会等待完整检查间隔。
#[derive(Debug)]
pub struct ObservationMaintenance {
    stop: Arc<(Mutex<bool>, Condvar)>,
    join: Option<JoinHandle<()>>,
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
        Self::open_with_visibility(
            state_dir,
            identity,
            privacy,
            retention_observations,
            retention_bytes,
            write_ref_cache,
            DEFAULT_SELECTOR_VISIBILITY_MS,
            now_ms,
        )
    }

    pub fn open_with_visibility(
        state_dir: PathBuf,
        identity: DurableObservationIdentity,
        privacy: DurableObservationPrivacy,
        retention_observations: usize,
        retention_bytes: u64,
        write_ref_cache: bool,
        selector_visibility_ms: u64,
        now_ms: u64,
    ) -> io::Result<Self> {
        Self::open_with_layout(
            state_dir,
            DurableStoreLayout::Exact,
            identity,
            privacy,
            retention_observations,
            retention_bytes,
            write_ref_cache,
            selector_visibility_ms,
            now_ms,
        )
    }

    pub fn open_dated(
        root_dir: PathBuf,
        identity: DurableObservationIdentity,
        privacy: DurableObservationPrivacy,
        retention_observations: usize,
        retention_bytes: u64,
        write_ref_cache: bool,
        now_ms: u64,
    ) -> io::Result<Self> {
        let daemon_dir_name = sanitize_path_component(&identity.daemon_name);
        let _root_lock = root_dir
            .is_dir()
            .then(|| acquire_root_maintenance_lock(&root_dir))
            .transpose()?;
        let existing = discover_existing_dated_store(&root_dir, &daemon_dir_name)?;
        let mut owner_lock = None;
        let state_dir = if let Some(path) = existing {
            owner_lock = Some(acquire_owner_lock(&root_dir, &daemon_dir_name)?);
            path
        } else {
            root_dir
                .join(date_component_from_unix_ms(now_ms))
                .join(&daemon_dir_name)
        };

        Self::open_with_layout(
            state_dir,
            DurableStoreLayout::Dated {
                root_dir,
                daemon_dir_name,
                owner_lock,
            },
            identity,
            privacy,
            retention_observations,
            retention_bytes,
            write_ref_cache,
            DEFAULT_SELECTOR_VISIBILITY_MS,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn open_with_layout(
        state_dir: PathBuf,
        layout: DurableStoreLayout,
        identity: DurableObservationIdentity,
        privacy: DurableObservationPrivacy,
        retention_observations: usize,
        retention_bytes: u64,
        write_ref_cache: bool,
        selector_visibility_ms: u64,
        now_ms: u64,
    ) -> io::Result<Self> {
        let materialized = looks_like_observation_store(&state_dir);
        let index = load_or_replay_index(&state_dir, now_ms)?;
        let mut store = Self {
            state_dir,
            layout,
            identity,
            privacy,
            retention_observations,
            retention_bytes,
            selector_visibility_ms,
            write_ref_cache,
            index,
        };
        store.prune_index();
        if materialized {
            store.write_meta(now_ms)?;
            store.write_index(now_ms)?;
            store.enforce_byte_retention(now_ms)?;
        }
        Ok(store)
    }

    pub fn record_observation(
        &mut self,
        header: &ObservationHeader,
        refs: &[ObservationRefEntry],
        selectors: &[DurableSelectorRecord],
    ) -> io::Result<()> {
        self.prepare_for_record(header.created_at_unix_ms)?;
        let observation_record = DurableObservationRecord::from_header(header);
        append_jsonl(self.observations_path(), &observation_record)?;
        append_jsonl_batch(self.selectors_path(), selectors.iter())?;

        if self.write_ref_cache {
            // `.entry().or_insert()` 保留原线性 `.find()` 的首个匹配语义,
            // 同时把 ref 到 selector 的关联从 O(R*S) 收敛为 O(R+S)。
            let mut selector_ids = HashMap::<&str, &str>::with_capacity(selectors.len());
            for selector in selectors {
                selector_ids
                    .entry(selector.ref_id.as_str())
                    .or_insert(selector.selector_id.as_str());
            }
            let cache_records = refs
                .iter()
                .map(|entry| DurableRefCacheRecord {
                    schema: DURABLE_REF_CACHE_SCHEMA.to_owned(),
                    observation_id: header.observation_id.clone(),
                    ref_id: entry.ref_id.clone(),
                    selector_id: selector_ids
                        .get(entry.ref_id.as_str())
                        .map(|id| (*id).to_owned()),
                    backend_id_hint: entry.backend_id.clone(),
                    kind: entry.kind.clone(),
                    cache_lifetime: "hint_only".to_owned(),
                })
                .collect::<Vec<_>>();
            append_jsonl_batch(self.ref_cache_path(), cache_records.iter())?;
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
        self.write_index(header.created_at_unix_ms)?;
        self.enforce_byte_retention(header.created_at_unix_ms)
    }

    fn prepare_for_record(&mut self, now_ms: u64) -> io::Result<()> {
        let _root_lock = match &self.layout {
            DurableStoreLayout::Exact => None,
            DurableStoreLayout::Dated { root_dir, .. } => {
                Some(acquire_root_maintenance_lock(root_dir)?)
            }
        };
        let target = match &mut self.layout {
            DurableStoreLayout::Exact => self.state_dir.clone(),
            DurableStoreLayout::Dated {
                root_dir,
                daemon_dir_name,
                owner_lock,
            } => {
                if owner_lock.is_none() {
                    *owner_lock = Some(acquire_owner_lock(root_dir, daemon_dir_name)?);
                }
                root_dir
                    .join(date_component_from_unix_ms(now_ms))
                    .join(daemon_dir_name)
            }
        };

        if self.state_dir != target && self.state_dir.exists() {
            let target_parent = target
                .parent()
                .ok_or_else(|| io::Error::other("dated observation path 缺少 parent"))?;
            fs::create_dir_all(target_parent)?;
            if target.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("dated observation 目标目录已存在: {}", target.display()),
                ));
            }
            let previous_parent = self.state_dir.parent().map(Path::to_path_buf);
            if let Err(err) = fs::rename(&self.state_dir, &target) {
                let _ = fs::remove_dir(target_parent);
                return Err(err);
            }
            if let Some(previous_parent) = previous_parent {
                if matches!(&self.layout, DurableStoreLayout::Dated { root_dir, .. } if previous_parent != *root_dir)
                {
                    let _ = fs::remove_dir(previous_parent);
                }
            }
        }

        self.state_dir = target;
        if !looks_like_observation_store(&self.state_dir) {
            fs::create_dir_all(self.state_dir.join("tmp"))?;
            self.write_meta(now_ms)?;
            self.write_index(now_ms)?;
        }
        Ok(())
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
            .filter(|selector| {
                self.selector_within_visibility(selector, self.index.updated_at_unix_ms)
            })
            .take(limit)
            .map(DurableSelectorLastSeen::from_index_selector)
            .collect()
    }

    #[cfg(test)]
    pub fn enforce_byte_retention_for_test(&mut self, now_ms: u64) -> io::Result<()> {
        self.enforce_byte_retention(now_ms)
    }

    /// Wayfinder Destination B (W-OS-02): selector is live iff the caller-supplied    /// Wayfinder Destination B (W-OS-02): selector is live iff the caller-supplied
    /// `now_ms` is within `selector_visibility_ms` of its last_seen timestamp.
    fn selector_within_visibility(&self, selector: &DurableIndexSelector, now_ms: u64) -> bool {
        now_ms.saturating_sub(selector.last_seen_unix_ms) <= self.selector_visibility_ms
    }

    #[cfg(test)]
    pub fn index(&self) -> &DurableStateIndex {
        &self.index
    }

    fn prune_index(&mut self) {
        let excess = self
            .index
            .observations
            .len()
            .saturating_sub(self.retention_observations);
        self.index.observations.drain(..excess);
        let retained = self
            .index
            .observations
            .iter()
            .map(|observation| observation.observation_id.as_str())
            .collect::<HashSet<_>>();
        let mut seen_stable = HashSet::<String>::new();
        let mut keep = vec![false; self.index.selectors.len()];
        for (index, selector) in self.index.selectors.iter().enumerate().rev() {
            keep[index] = retained.contains(selector.observation_id.as_str())
                || seen_stable.insert(selector.selector_id.clone());
        }
        let mut index = 0usize;
        self.index.selectors.retain(|_| {
            let retain = keep[index];
            index += 1;
            retain
        });
    }

    fn enforce_byte_retention(&mut self, now_ms: u64) -> io::Result<()> {
        if state_file_bytes(&self.state_dir)? <= self.retention_bytes {
            return Ok(());
        }

        let mut records =
            CompactionRecords::load(&self.state_dir, &self.index, self.write_ref_cache)?;
        self.index.updated_at_unix_ms = now_ms;
        // Wayfinder Destination B (W-OS-02): 先把超窗的 orphan selector rows 立即淘汰,
        // 这样 `selector_history` 不会因为 disk 容差窗口之外看到 stale row。
        let retained_observation_ids: HashSet<String> = self
            .index
            .observations
            .iter()
            .map(|obs| obs.observation_id.clone())
            .collect();
        let visibility_ms = self.selector_visibility_ms;
        self.index.selectors.retain(|row| {
            retained_observation_ids.contains(&row.observation_id)
                || now_ms.saturating_sub(row.last_seen_unix_ms) <= visibility_ms
        });
        loop {
            records.retain_for_index(&self.index);
            let total = compacted_state_bytes(&self.index, &records)?;
            if total <= self.retention_bytes {
                break;
            }

            let retained = self
                .index
                .observations
                .iter()
                .map(|item| item.observation_id.as_str())
                .collect::<HashSet<_>>();
            let mut orphan_times = HashMap::<SelectorKey, u64>::new();
            for item in self
                .index
                .selectors
                .iter()
                .filter(|item| !retained.contains(item.observation_id.as_str()))
            {
                orphan_times
                    .entry(selector_key(item))
                    .and_modify(|last_seen| *last_seen = (*last_seen).max(item.last_seen_unix_ms))
                    .or_insert(item.last_seen_unix_ms);
            }
            let mut orphans = orphan_times
                .into_iter()
                .map(|(key, last_seen)| (last_seen, key))
                .collect::<Vec<_>>();
            orphans.sort();

            if !orphans.is_empty() {
                let selector_sizes = records.selector_sizes()?;
                let mut index_sizes = HashMap::<SelectorKey, u64>::new();
                for item in &self.index.selectors {
                    *index_sizes.entry(selector_key(item)).or_default() +=
                        serialized_line_bytes(item)?;
                }
                let mut estimated = total;
                let mut remove = HashSet::new();
                for (_, key) in orphans {
                    estimated = estimated.saturating_sub(
                        index_sizes.get(&key).copied().unwrap_or_default()
                            + selector_sizes.get(&key).copied().unwrap_or_default(),
                    );
                    remove.insert(key);
                    if estimated <= self.retention_bytes {
                        break;
                    }
                }
                self.index
                    .selectors
                    .retain(|item| !remove.contains(&selector_key(item)));
            } else if !self.index.observations.is_empty() {
                self.index.observations.remove(0);
                self.prune_index();
            } else {
                return Err(io::Error::other(
                    "durable observation 无法压缩到 retention_bytes 上限",
                ));
            }
        }

        // index 先替换,此时旧 JSONL 仍是新 index 的超集。即使中途退出,
        // 正常读取也不会引用不存在的记录;随后再逐个原子替换 compact JSONL。
        self.write_index(now_ms)?;
        write_jsonl_atomic(self.observations_path(), &records.observations)?;
        write_jsonl_atomic(self.selectors_path(), &records.selectors)?;
        write_jsonl_atomic(self.ref_cache_path(), &records.ref_cache)?;
        if state_file_bytes(&self.state_dir)? > self.retention_bytes {
            return Err(io::Error::other(
                "durable observation compaction 后仍超过 retention_bytes",
            ));
        }
        Ok(())
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

impl ObservationMaintenance {
    pub fn start(root_dir: PathBuf, interval: Duration, retention_days: u64) -> Self {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = Arc::clone(&stop);
        let join = thread::spawn(move || loop {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            match cleanup_expired_default_observation_dirs(&root_dir, now_ms, retention_days) {
                Ok(report)
                    if report.removed_stores > 0
                        || report.skipped_active_stores > 0
                        || report.skipped_unknown_stores > 0 =>
                {
                    log::info!(
                        "observation cleanup complete: root={}, removed_stores={}, removed_date_dirs={}, skipped_active_stores={}, skipped_unknown_stores={}",
                        root_dir.display(),
                        report.removed_stores,
                        report.removed_date_dirs,
                        report.skipped_active_stores,
                        report.skipped_unknown_stores
                    );
                }
                Ok(_) => {}
                Err(err) => log::warn!(
                    "observation cleanup failed: root={}, error={err}",
                    root_dir.display()
                ),
            }

            let (lock, wake) = &*worker_stop;
            let Ok(stopped) = lock.lock() else {
                return;
            };
            if *stopped {
                return;
            }
            let Ok((stopped, _)) = wake.wait_timeout(stopped, interval) else {
                return;
            };
            if *stopped {
                return;
            }
        });

        Self {
            stop,
            join: Some(join),
        }
    }
}

impl Drop for ObservationMaintenance {
    fn drop(&mut self) {
        let (lock, wake) = &*self.stop;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
            wake.notify_one();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn cleanup_expired_default_observation_dirs(
    root_dir: &Path,
    now_ms: u64,
    retention_days: u64,
) -> io::Result<ObservationCleanupReport> {
    if retention_days == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "observation retention_days 必须大于 0",
        ));
    }
    if !root_dir.is_dir() {
        return Ok(ObservationCleanupReport::default());
    }

    // 所有 dated store 物化、跨日移动和 owner lock 获取都先经过这把根锁。
    // 因此清理后删除 owner lock path 不会和新 daemon 打开同一路径产生 inode 分裂。
    let _root_lock = acquire_root_maintenance_lock(root_dir)?;
    let cutoff_day = (now_ms / DAY_MS).saturating_sub(retention_days);
    let mut report = ObservationCleanupReport::default();
    for date_entry in fs::read_dir(root_dir)? {
        let date_entry = date_entry?;
        if !date_entry.file_type()?.is_dir() {
            continue;
        }
        let date_name = date_entry.file_name();
        let Some(date_day) = date_name
            .to_str()
            .and_then(parse_date_component_to_unix_day)
        else {
            continue;
        };
        if date_day >= cutoff_day {
            continue;
        }

        report.scanned_date_dirs += 1;
        let date_dir = date_entry.path();
        for store_entry in fs::read_dir(&date_dir)? {
            let store_entry = store_entry?;
            if !store_entry.file_type()?.is_dir() {
                continue;
            }
            if !looks_like_observation_store(&store_entry.path()) {
                report.skipped_unknown_stores += 1;
                continue;
            }
            let Some(daemon_dir_name) = store_entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(cleanup_lock) = try_acquire_cleanup_lock(root_dir, &daemon_dir_name)? else {
                report.skipped_active_stores += 1;
                continue;
            };

            fs::remove_dir_all(store_entry.path())?;
            drop(cleanup_lock);
            match fs::remove_file(owner_lock_path(root_dir, &daemon_dir_name)) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            report.removed_stores += 1;
        }

        if fs::read_dir(&date_dir)?.next().is_none() {
            fs::remove_dir(&date_dir)?;
            report.removed_date_dirs += 1;
        }
    }
    Ok(report)
}

pub fn default_observation_root() -> PathBuf {
    if cfg!(windows) {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rdog")
            .join("observations")
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library")
            .join("Application Support")
            .join("rdog")
            .join("observations")
    } else {
        env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rdog")
            .join("observations")
    }
}

fn discover_existing_dated_store(
    root_dir: &Path,
    daemon_dir_name: &str,
) -> io::Result<Option<PathBuf>> {
    if !root_dir.is_dir() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    let legacy = root_dir.join(daemon_dir_name);
    if legacy.is_dir() && looks_like_observation_store(&legacy) {
        candidates.push(legacy);
    }
    for entry in fs::read_dir(root_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || entry
                .file_name()
                .to_str()
                .and_then(parse_date_component_to_unix_day)
                .is_none()
        {
            continue;
        }
        let candidate = entry.path().join(daemon_dir_name);
        if candidate.is_dir() && looks_like_observation_store(&candidate) {
            candidates.push(candidate);
        }
    }

    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.pop()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "同一 daemon 存在多个 durable observation store: daemon={daemon_dir_name}, candidates={candidates:?}"
            ),
        )),
    }
}

fn looks_like_observation_store(path: &Path) -> bool {
    fs::read(path.join("meta.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DurableObservationMeta>(&bytes).ok())
        .is_some_and(|meta| meta.schema == DURABLE_STATE_SCHEMA)
}

fn acquire_root_maintenance_lock(root_dir: &Path) -> io::Result<File> {
    fs::create_dir_all(root_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root_dir.join(MAINTENANCE_LOCK_NAME))?;
    file.lock()?;
    Ok(file)
}

fn owner_lock_path(root_dir: &Path, daemon_dir_name: &str) -> PathBuf {
    root_dir
        .join(OWNER_DIR_NAME)
        .join(format!("{daemon_dir_name}.lock"))
}

fn acquire_owner_lock(root_dir: &Path, daemon_dir_name: &str) -> io::Result<File> {
    let owners_dir = root_dir.join(OWNER_DIR_NAME);
    fs::create_dir_all(&owners_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(owner_lock_path(root_dir, daemon_dir_name))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("durable observation store 已被活动 daemon 持有: {daemon_dir_name}"),
        )),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

fn try_acquire_cleanup_lock(root_dir: &Path, daemon_dir_name: &str) -> io::Result<Option<File>> {
    let owners_dir = root_dir.join(OWNER_DIR_NAME);
    fs::create_dir_all(&owners_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(owner_lock_path(root_dir, daemon_dir_name))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

fn date_component_from_unix_ms(unix_ms: u64) -> String {
    let (year, month, day) = civil_from_unix_day(unix_ms / DAY_MS);
    format!("{year:04}-{month:02}-{day:02}")
}

fn parse_date_component_to_unix_day(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = value[0..4].parse::<i64>().ok()?;
    let month = value[5..7].parse::<u32>().ok()?;
    let day = value[8..10].parse::<u32>().ok()?;
    let unix_day = unix_day_from_civil(year, month, day)?;
    (unix_day >= 0).then_some(unix_day as u64)
}

fn unix_day_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month {
        return None;
    }

    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_unix_day(unix_day: u64) -> (i64, u32, u32) {
    let shifted = unix_day as i64 + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
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
    append_jsonl_batch(path, std::iter::once(value))
}

fn append_jsonl_batch<'a, T: Serialize + 'a>(
    path: PathBuf,
    values: impl IntoIterator<Item = &'a T>,
) -> io::Result<()> {
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut writer = BufWriter::new(file);
    for value in values {
        serde_json::to_writer(&mut writer, value).map_err(|err| {
            io::Error::other(format!("durable observation JSONL 写入失败: {err}"))
        })?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
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
    let bytes = serde_json::to_vec(value)
        .map_err(|err| io::Error::other(format!("durable observation JSON 序列化失败: {err}")))?;
    fs::write(&tmp_path, bytes)?;
    fs::rename(tmp_path, path)
}

#[derive(Debug)]
struct CompactionRecords {
    observations: Vec<DurableObservationRecord>,
    selectors: Vec<DurableSelectorRecord>,
    ref_cache: Vec<DurableRefCacheRecord>,
}

type SelectorKey = (String, String, String);

impl CompactionRecords {
    fn load(
        state_dir: &Path,
        index: &DurableStateIndex,
        include_ref_cache: bool,
    ) -> io::Result<Self> {
        let observation_ids = index
            .observations
            .iter()
            .map(|item| item.observation_id.clone())
            .collect::<HashSet<_>>();
        let mut selector_counts = selector_key_counts(index);
        Ok(Self {
            observations: read_jsonl_where(
                state_dir.join("observations.jsonl"),
                |item: &DurableObservationRecord| observation_ids.contains(&item.observation_id),
            )?,
            selectors: read_jsonl_where(
                state_dir.join("selectors.jsonl"),
                |item: &DurableSelectorRecord| {
                    take_selector_key(&mut selector_counts, selector_record_key(item))
                },
            )?,
            ref_cache: if include_ref_cache {
                read_jsonl_where(
                    state_dir.join("ref_cache.jsonl"),
                    |item: &DurableRefCacheRecord| observation_ids.contains(&item.observation_id),
                )?
            } else {
                Vec::new()
            },
        })
    }

    fn retain_for_index(&mut self, index: &DurableStateIndex) {
        let observation_ids = index
            .observations
            .iter()
            .map(|item| item.observation_id.as_str())
            .collect::<HashSet<_>>();
        let mut selector_counts = selector_key_counts(index);
        self.observations
            .retain(|item| observation_ids.contains(item.observation_id.as_str()));
        self.selectors
            .retain(|item| take_selector_key(&mut selector_counts, selector_record_key(item)));
        self.ref_cache
            .retain(|item| observation_ids.contains(item.observation_id.as_str()));
    }

    fn selector_sizes(&self) -> io::Result<HashMap<SelectorKey, u64>> {
        let mut sizes = HashMap::<SelectorKey, u64>::new();
        for item in &self.selectors {
            *sizes.entry(selector_record_key(item)).or_default() += serialized_line_bytes(item)?;
        }
        Ok(sizes)
    }
}

fn selector_key_counts(index: &DurableStateIndex) -> HashMap<SelectorKey, usize> {
    let mut counts = HashMap::new();
    for item in &index.selectors {
        *counts.entry(selector_key(item)).or_default() += 1;
    }
    counts
}

fn take_selector_key(counts: &mut HashMap<SelectorKey, usize>, key: SelectorKey) -> bool {
    let Some(count) = counts.get_mut(&key) else {
        return false;
    };
    if *count == 0 {
        return false;
    }
    *count -= 1;
    true
}

fn selector_key(item: &DurableIndexSelector) -> SelectorKey {
    (
        item.selector_id.clone(),
        item.observation_id.clone(),
        item.ref_id.clone(),
    )
}

fn selector_record_key(item: &DurableSelectorRecord) -> SelectorKey {
    (
        item.stable_selector_id(),
        item.observation_id.clone(),
        item.ref_id.clone(),
    )
}

fn compacted_state_bytes(
    index: &DurableStateIndex,
    records: &CompactionRecords,
) -> io::Result<u64> {
    let index_bytes = serde_json::to_vec(index)
        .map_err(|err| io::Error::other(format!("durable observation index 序列化失败: {err}")))?
        .len() as u64;
    Ok(index_bytes
        + serialized_jsonl_bytes(&records.observations)?
        + serialized_jsonl_bytes(&records.selectors)?
        + serialized_jsonl_bytes(&records.ref_cache)?)
}

fn serialized_jsonl_bytes<T: Serialize>(values: &[T]) -> io::Result<u64> {
    values
        .iter()
        .try_fold(0u64, |total, item| Ok(total + serialized_line_bytes(item)?))
}

fn serialized_line_bytes<T: Serialize>(value: &T) -> io::Result<u64> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64 + 1)
        .map_err(|err| io::Error::other(format!("durable observation JSON 序列化失败: {err}")))
}

fn state_file_bytes(state_dir: &Path) -> io::Result<u64> {
    [
        "observations.jsonl",
        "selectors.jsonl",
        "ref_cache.jsonl",
        "index.json",
    ]
    .into_iter()
    .try_fold(0u64, |total, name| {
        match fs::metadata(state_dir.join(name)) {
            Ok(metadata) => Ok(total + metadata.len()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(total),
            Err(err) => Err(err),
        }
    })
}

fn write_jsonl_atomic<T: Serialize>(path: PathBuf, values: &[T]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("durable observation path 缺少 parent"))?;
    let tmp_path = parent.join("tmp").join(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("durable-observation.jsonl")
    ));
    let file = fs::File::create(&tmp_path)?;
    let mut writer = BufWriter::new(file);
    for value in values {
        serde_json::to_writer(&mut writer, value).map_err(|err| {
            io::Error::other(format!("durable observation JSONL 写入失败: {err}"))
        })?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
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
    read_jsonl_where(path, |_| true)
}

fn read_jsonl_where<T, F>(path: PathBuf, mut keep: F) -> io::Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
    F: FnMut(&T) -> bool,
{
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut values = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(&line).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("durable observation JSONL 解析失败: {err}"),
            )
        })?;
        if keep(&value) {
            values.push(value);
        }
    }
    Ok(values)
}

#[cfg(test)]
#[path = "durable/tests.rs"]
mod tests;
