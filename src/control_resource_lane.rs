//! GUI mutation 的 daemon-owned 资源调度。
//!
//! 同一物理资源按 `resource_key` 串行执行,并在 dispatch 前递增 write epoch。
//! observation 只保存当时的 epoch 快照,不拥有全局可变版本。

use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEpochSnapshot {
    pub resource_key: String,
    pub epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleResourceEpoch {
    pub resource_key: String,
    pub expected_epoch: u64,
    pub current_epoch: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceEpochCapture {
    epochs: HashMap<String, u64>,
}

impl ResourceEpochCapture {
    pub fn snapshot(&self, resource_key: &str) -> ResourceEpochSnapshot {
        ResourceEpochSnapshot {
            resource_key: resource_key.to_owned(),
            epoch: self.epochs.get(resource_key).copied().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Default)]
struct ResourceCoordinator {
    // ponytail: PID 状态在 daemon 生命周期内保留。若长期 PID churn 造成可测内存压力,
    // 再按已过期 observation 的最小 epoch 做安全回收,不能用普通 LRU 丢失正确性。
    lanes: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    // epoch map 是所有资源版本的单一真相源。完整 clone 提供 capture-start 一致视图。
    epochs: Mutex<HashMap<String, u64>>,
}

impl ResourceCoordinator {
    fn lane(&self, resource_key: &str) -> Arc<Mutex<()>> {
        let mut lanes = self.lanes.lock().expect("resource lane map poisoned");
        lanes
            .entry(resource_key.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    #[cfg(test)]
    fn snapshot(&self, resource_key: &str) -> ResourceEpochSnapshot {
        let epochs = self.epochs.lock().expect("resource epoch map poisoned");
        ResourceEpochSnapshot {
            resource_key: resource_key.to_owned(),
            epoch: epochs.get(resource_key).copied().unwrap_or_default(),
        }
    }

    fn capture(&self) -> ResourceEpochCapture {
        let epochs = self.epochs.lock().expect("resource epoch map poisoned");
        ResourceEpochCapture {
            epochs: epochs.clone(),
        }
    }

    fn write<T>(
        &self,
        snapshot: &ResourceEpochSnapshot,
        dispatch: impl FnOnce() -> io::Result<T>,
    ) -> Result<io::Result<T>, StaleResourceEpoch> {
        let lane = self.lane(&snapshot.resource_key);
        let _lane_guard = lane.lock().expect("resource lane poisoned");
        let mut epochs = self.epochs.lock().expect("resource epoch map poisoned");
        let current_epoch = epochs
            .get(&snapshot.resource_key)
            .copied()
            .unwrap_or_default();
        if current_epoch != snapshot.epoch {
            return Err(StaleResourceEpoch {
                resource_key: snapshot.resource_key.clone(),
                expected_epoch: snapshot.epoch,
                current_epoch,
            });
        }

        let Some(dispatch_epoch) = current_epoch.checked_add(1) else {
            return Ok(Err(io::Error::other(format!(
                "资源 {} 的 write epoch 已耗尽",
                snapshot.resource_key
            ))));
        };
        let Some(completed_epoch) = dispatch_epoch.checked_add(1) else {
            return Ok(Err(io::Error::other(format!(
                "资源 {} 的 write epoch 已耗尽",
                snapshot.resource_key
            ))));
        };

        // 奇数 epoch 表示 mutation 正在执行。capture 在 dispatch 前或期间开始,
        // 都会在完成后的第二次递增中失效。
        epochs.insert(snapshot.resource_key.clone(), dispatch_epoch);
        drop(epochs);
        let result = dispatch();

        let mut epochs = self.epochs.lock().expect("resource epoch map poisoned");
        epochs.insert(snapshot.resource_key.clone(), completed_epoch);
        Ok(result)
    }
}

static RESOURCE_COORDINATOR: OnceLock<ResourceCoordinator> = OnceLock::new();

fn coordinator() -> &'static ResourceCoordinator {
    RESOURCE_COORDINATOR.get_or_init(ResourceCoordinator::default)
}

#[cfg(test)]
pub fn snapshot_resource_epoch(resource_key: &str) -> ResourceEpochSnapshot {
    coordinator().snapshot(resource_key)
}

pub fn capture_resource_epochs() -> ResourceEpochCapture {
    coordinator().capture()
}

pub fn with_resource_write<T>(
    snapshot: &ResourceEpochSnapshot,
    dispatch: impl FnOnce() -> io::Result<T>,
) -> Result<io::Result<T>, StaleResourceEpoch> {
    coordinator().write(snapshot, dispatch)
}

/// 从 AX/window backend id 提取 PID 资源键。
///
/// 输入示例: `pid:123/window:0/path:7.3` -> `pid:123`。
pub fn resource_key_from_backend_id(backend_id: &str) -> Option<String> {
    let pid = backend_id.strip_prefix("pid:")?.split('/').next()?;
    (!pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit())).then(|| format!("pid:{pid}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Barrier,
        },
        thread,
        time::Duration,
    };

    fn unique_resource(label: &str) -> String {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        format!("pid:{}-{label}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    #[test]
    fn stale_snapshot_is_rejected_after_first_write() {
        let resource = unique_resource("stale");
        let snapshot = snapshot_resource_epoch(&resource);

        with_resource_write(&snapshot, || Ok(()))
            .expect("first write should enter")
            .unwrap();
        let stale = with_resource_write(&snapshot, || -> io::Result<()> {
            panic!("stale write must not dispatch")
        })
        .expect_err("second write should be stale");

        assert_eq!(stale.expected_epoch, 0);
        assert_eq!(stale.current_epoch, 2);
    }

    #[test]
    fn failed_dispatch_still_invalidates_old_snapshot() {
        let resource = unique_resource("failed");
        let snapshot = snapshot_resource_epoch(&resource);

        let first = with_resource_write(&snapshot, || {
            Err::<(), _>(io::Error::other("injected dispatch failure"))
        })
        .expect("snapshot should be current");
        assert!(first.is_err());
        assert!(with_resource_write(&snapshot, || Ok(())).is_err());
    }

    #[test]
    fn capture_started_during_dispatch_is_stale_after_dispatch() {
        let resource = unique_resource("capture-during-dispatch");
        let snapshot = snapshot_resource_epoch(&resource);
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        let thread_release = release.clone();
        let thread_snapshot = snapshot.clone();

        let writer = thread::spawn(move || {
            with_resource_write(&thread_snapshot, || {
                entered_tx.send(()).expect("entry signal should send");
                thread_release.wait();
                Ok(())
            })
            .expect("write should enter")
            .expect("write should complete");
        });
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("dispatch should enter");
        let during_dispatch = capture_resource_epochs().snapshot(&resource);
        release.wait();
        writer.join().expect("writer should finish");

        assert!(with_resource_write(&during_dispatch, || -> io::Result<()> {
            panic!("capture taken during dispatch must be stale")
        })
        .is_err());
    }

    #[test]
    fn different_resources_can_dispatch_in_parallel() {
        let first = snapshot_resource_epoch(&unique_resource("parallel-a"));
        let second = snapshot_resource_epoch(&unique_resource("parallel-b"));
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(3));

        let spawn = |snapshot: ResourceEpochSnapshot,
                     entered_tx: mpsc::Sender<()>,
                     release: Arc<Barrier>| {
            thread::spawn(move || {
                with_resource_write(&snapshot, || {
                    entered_tx.send(()).expect("entry signal should send");
                    release.wait();
                    Ok(())
                })
                .expect("resource snapshot should be current")
                .expect("dispatch should succeed");
            })
        };
        let first_thread = spawn(first, entered_tx.clone(), release.clone());
        let second_thread = spawn(second, entered_tx, release.clone());
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first resource should enter");
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second resource should enter without waiting for first");
        release.wait();
        first_thread.join().expect("first thread should finish");
        second_thread.join().expect("second thread should finish");
    }

    #[test]
    fn backend_id_parser_accepts_only_pid_prefix() {
        assert_eq!(
            resource_key_from_backend_id("pid:123/window:0/path:7.3").as_deref(),
            Some("pid:123")
        );
        assert_eq!(resource_key_from_backend_id("visual:1"), None);
        assert_eq!(resource_key_from_backend_id("pid:bad/window:0"), None);
    }
}
