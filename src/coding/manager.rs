use super::model::{CodingConfig, CodingError, CodingReply, CodingRequest, ErrorCode, Result};

pub struct CodingService {
    #[cfg(windows)]
    inner: windows::Manager,
}

impl CodingService {
    pub fn new(config: CodingConfig) -> Result<Self> {
        if !config.enabled {
            return Err(CodingError::new(ErrorCode::Disabled, "coding is disabled"));
        }
        config.validate()?;
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows::Manager::new(config)?,
            })
        }
        #[cfg(not(windows))]
        {
            Err(CodingError::new(
                ErrorCode::UnsupportedPlatform,
                "coding host requires Windows",
            ))
        }
    }

    pub fn execute(&self, request: CodingRequest) -> CodingReply {
        #[cfg(windows)]
        {
            self.inner.execute(request)
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            CodingReply::failure(CodingError::new(
                ErrorCode::UnsupportedPlatform,
                "coding host requires Windows",
            ))
        }
    }
}

#[cfg(windows)]
pub(super) fn write_json_atomic(
    path: &std::path::Path,
    value: &impl serde::Serialize,
) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| CodingError::new(ErrorCode::Storage, "file has no parent"))?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        if path
            .file_name()
            .is_some_and(|name| name == "host_identity.json" || name == "manifest.json")
        {
            match std::fs::hard_link(&temporary, path) {
                Ok(()) => std::fs::remove_file(&temporary)?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing: serde_json::Value =
                        serde_json::from_reader(std::fs::File::open(path)?)?;
                    if existing != serde_json::to_value(value)? {
                        return Err(CodingError::new(
                            ErrorCode::HostIdentityMismatch,
                            "immutable file differs",
                        ));
                    }
                    std::fs::remove_file(&temporary)?;
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            std::fs::rename(&temporary, path)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
mod windows {
    use std::fs::{self, File, OpenOptions, TryLockError};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    use super::super::model::{ProcessIdentity, SessionMetadata};
    use super::super::{ipc, platform, store::Store};
    use super::{
        write_json_atomic, CodingConfig, CodingError, CodingReply, CodingRequest, ErrorCode, Result,
    };

    #[derive(Serialize, Deserialize)]
    struct CreateRecord {
        digest: String,
        metadata: SessionMetadata,
        phase: CreatePhase,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "state", rename_all = "snake_case")]
    enum CreatePhase {
        Creating,
        Launching { error: Option<CodingError> },
        Running { identity: ProcessIdentity },
        Failed { error: CodingError },
    }

    pub(super) struct Manager {
        config: CodingConfig,
        root: PathBuf,
    }

    fn error(code: ErrorCode, message: impl Into<String>) -> CodingError {
        CodingError::new(code, message)
    }

    fn existing_directory(path: &Path) -> Result<()> {
        match fs::symlink_metadata(path) {
            Ok(_) => platform::secure_directory(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(error(ErrorCode::NotFound, "session not found"))
            }
            Err(e) => Err(e.into()),
        }
    }

    fn executable(config: &CodingConfig) -> Result<PathBuf> {
        if let Some(path) = &config.omp_executable {
            if !path.is_file() {
                return Err(error(
                    ErrorCode::NotFound,
                    "configured omp executable is not a file",
                ));
            }
            return Ok(fs::canonicalize(path)?);
        }
        let path = config
            .environment
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| std::ffi::OsString::from(value))
            .or_else(|| std::env::var_os("PATH"))
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidRequest,
                    "omp executable is not configured and PATH is unavailable",
                )
            })?;
        for dir in std::env::split_paths(&path) {
            if dir.is_absolute() {
                let candidate = dir.join("omp.exe");
                if candidate.is_file() {
                    return Ok(fs::canonicalize(candidate)?);
                }
            }
        }
        Err(error(ErrorCode::NotFound, "omp.exe not found on PATH"))
    }

    impl Manager {
        pub(super) fn new(config: CodingConfig) -> Result<Self> {
            let root = match &config.state_dir {
                Some(path) => path.clone(),
                None => {
                    let path = platform::default_state_dir()?;
                    let parent = path.parent().ok_or_else(|| {
                        error(ErrorCode::Storage, "default state directory has no parent")
                    })?;
                    if !parent.exists() {
                        platform::secure_directory(parent)?;
                    }
                    path
                }
            };
            platform::secure_directory(&root)?;
            platform::secure_directory(&root.join("sessions"))?;
            platform::secure_directory(&root.join("requests"))?;
            Ok(Self { config, root })
        }

        pub(super) fn execute(&self, request: CodingRequest) -> CodingReply {
            match self.route(request) {
                Ok(reply) => reply,
                Err(e) => CodingReply::failure(e),
            }
        }

        fn route(&self, request: CodingRequest) -> Result<CodingReply> {
            request.validate()?;
            match &request {
                CodingRequest::SessionOpen { .. } => self.open(&request),
                CodingRequest::SessionList {} => Ok(CodingReply::success(self.list()?)),
                _ => self.session_request(request),
            }
        }

        fn lock(&self) -> Result<File> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(self.root.join("requests").join("index.lock"))?;
            match file.try_lock() {
                Ok(()) => Ok(file),
                Err(TryLockError::WouldBlock) => {
                    Err(error(ErrorCode::Busy, "create index is busy"))
                }
                Err(TryLockError::Error(e)) => Err(e.into()),
            }
        }

        fn open(&self, request: &CodingRequest) -> Result<CodingReply> {
            let CodingRequest::SessionOpen {
                request_id,
                cwd,
                profile,
            } = request
            else {
                return Err(error(ErrorCode::InvalidRequest, "expected session open"));
            };
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(request)?));
            let key = format!("{:x}.json", Sha256::digest(request_id.as_bytes()));
            let index = self.root.join("requests").join(key);
            let _lock = self.lock()?;
            let mut record: CreateRecord = match File::open(&index) {
                Ok(file) => serde_json::from_reader(file)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let created_at_ms = u64::try_from(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_err(|e| error(ErrorCode::Storage, e.to_string()))?
                            .as_millis(),
                    )
                    .map_err(|_| error(ErrorCode::Storage, "clock value is out of range"))?;
                    let record = CreateRecord {
                        digest: digest.clone(),
                        metadata: SessionMetadata {
                            session_id: format!("session-{}", Uuid::new_v4()),
                            host_instance_id: format!("host-{}", Uuid::new_v4()),
                            cursor_epoch: Uuid::new_v4().to_string(),
                            created_at_ms,
                            cwd: cwd.clone(),
                            profile: profile
                                .clone()
                                .unwrap_or_else(|| self.config.profile.clone()),
                            omp_executable: executable(&self.config)?,
                            extensions: self.config.extensions.clone(),
                            max_journal_bytes: self.config.max_journal_bytes,
                        },
                        phase: CreatePhase::Creating,
                    };
                    write_json_atomic(&index, &record)?;
                    record
                }
                Err(e) => return Err(e.into()),
            };
            if record.digest != digest {
                return Err(error(
                    ErrorCode::Conflict,
                    "request id reused with different payload",
                ));
            }
            let dir = self.root.join("sessions").join(&record.metadata.session_id);
            if matches!(record.phase, CreatePhase::Creating) {
                let preparation = (|| -> Result<()> {
                    platform::secure_directory(&dir)?;
                    let manifest = dir.join("manifest.json");
                    match File::open(&manifest) {
                        Ok(file) => {
                            let existing: SessionMetadata = serde_json::from_reader(file)?;
                            if serde_json::to_value(&existing)?
                                != serde_json::to_value(&record.metadata)?
                            {
                                return Err(error(
                                    ErrorCode::HostIdentityMismatch,
                                    "session manifest differs from create record",
                                ));
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            write_json_atomic(&manifest, &record.metadata)?
                        }
                        Err(e) => return Err(e.into()),
                    }
                    Ok(())
                })();
                if let Err(e) = preparation {
                    record.phase = CreatePhase::Failed { error: e.clone() };
                    write_json_atomic(&index, &record)?;
                    return Err(e);
                }
                record.phase = CreatePhase::Launching { error: None };
                write_json_atomic(&index, &record)?;
                let identity =
                    match platform::spawn_host(&dir, &record.metadata, &self.config.environment) {
                        Ok(identity) => identity,
                        Err(e) => {
                            record.phase = CreatePhase::Launching {
                                error: Some(e.clone()),
                            };
                            write_json_atomic(&index, &record)?;
                            return Err(e);
                        }
                    };
                record.phase = CreatePhase::Running { identity };
                write_json_atomic(&index, &record)?;
            }
            if let CreatePhase::Failed { error } = &record.phase {
                return Err(error.clone());
            }
            match &record.phase {
                CreatePhase::Running { .. } => Ok(CodingReply::success(json!({
                    "session_id": record.metadata.session_id,
                    "host_instance_id": record.metadata.host_instance_id,
                    "status": "accepted"
                }))),
                CreatePhase::Launching { error: Some(e) } => Err(e.clone()),
                CreatePhase::Launching { error: None } => Ok(CodingReply::success(json!({
                    "session_id": record.metadata.session_id,
                    "host_instance_id": record.metadata.host_instance_id,
                    "status": "unknown"
                }))),
                _ => Err(error(ErrorCode::Storage, "invalid create state")),
            }
        }

        fn host_identity(
            &self,
            dir: &Path,
            expected: Option<&ProcessIdentity>,
        ) -> Result<Option<ProcessIdentity>> {
            let identity: ProcessIdentity = match File::open(dir.join("host_identity.json")) {
                Ok(file) => serde_json::from_reader(file)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            if expected.is_some_and(|value| value != &identity) {
                return Err(error(
                    ErrorCode::HostIdentityMismatch,
                    "host identity does not match launch identity",
                ));
            }
            Ok(Some(identity))
        }

        fn expected_host(&self, meta: &SessionMetadata) -> Result<Option<ProcessIdentity>> {
            let _lock = self.lock()?;
            let mut found = None;
            for entry in fs::read_dir(self.root.join("requests"))? {
                let entry = entry?;
                if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let record: CreateRecord = serde_json::from_reader(File::open(entry.path())?)?;
                if record.metadata.session_id != meta.session_id {
                    continue;
                }
                if found.is_some()
                    || serde_json::to_value(&record.metadata)? != serde_json::to_value(meta)?
                {
                    return Err(error(
                        ErrorCode::HostIdentityMismatch,
                        "create record and manifest differ",
                    ));
                }
                found = Some(match record.phase {
                    CreatePhase::Running { identity } => Some(identity),
                    CreatePhase::Launching { .. } => None,
                    CreatePhase::Failed { error } => return Err(error),
                    CreatePhase::Creating => {
                        return Err(error(ErrorCode::Starting, "session is being created"))
                    }
                });
            }
            found.ok_or_else(|| error(ErrorCode::Storage, "session create record is missing"))
        }

        fn metadata(&self, id: &str) -> Result<(PathBuf, SessionMetadata)> {
            let dir = self.root.join("sessions").join(id);
            existing_directory(&dir)?;
            let meta = Store::metadata(&dir)?;
            if meta.session_id != id {
                return Err(error(
                    ErrorCode::HostIdentityMismatch,
                    "session manifest identity mismatch",
                ));
            }
            Ok((dir, meta))
        }

        fn session_request(&self, request: CodingRequest) -> Result<CodingReply> {
            let id = request
                .session_id()
                .ok_or_else(|| error(ErrorCode::InvalidRequest, "missing session id"))?;
            let (dir, meta) = self.metadata(id)?;
            let expected = self.expected_host(&meta)?;
            let identity = self.host_identity(&dir, expected.as_ref())?;
            if let Some(identity) = identity {
                if platform::identity_alive(&identity)? {
                    return ipc::call(&meta, &identity, &request);
                }
            } else if let Some(identity) = &expected {
                if platform::identity_alive(identity)? {
                    return Err(error(
                        ErrorCode::Starting,
                        "host identity is not yet available",
                    ));
                }
            } else {
                return Err(error(
                    ErrorCode::UnknownOutcome,
                    "host launch outcome is unknown",
                ));
            }
            let mut result = Store::query_offline(&dir, request.clone(), meta.max_journal_bytes)?;
            match &request {
                CodingRequest::SessionStatus { .. } => {
                    if let Some(object) = result.as_object_mut() {
                        object.insert("host_alive".into(), json!(false));
                        if object.get("status").and_then(Value::as_str) != Some("closed") {
                            object.insert("status".into(), json!("host_unavailable"));
                        }
                    }
                }
                CodingRequest::TaskStatus { .. } => {
                    if let Some(object) = result.as_object_mut() {
                        object.insert("host_alive".into(), json!(false));
                        if object
                            .get("result_status")
                            .and_then(Value::as_str)
                            .is_none()
                            && !matches!(
                                object.get("status").and_then(Value::as_str),
                                Some("cancelled" | "aborted")
                            )
                        {
                            object.insert("status".into(), json!("unknown"));
                        }
                    }
                }
                _ => {}
            }
            Ok(CodingReply::success(result))
        }

        fn list(&self) -> Result<Value> {
            let mut sessions = Vec::new();
            for entry in fs::read_dir(self.root.join("sessions"))? {
                let entry = entry?;
                let name = entry.file_name();
                let id = name
                    .to_str()
                    .ok_or_else(|| error(ErrorCode::Storage, "invalid session directory name"))?;
                let request = CodingRequest::SessionStatus {
                    session_id: id.to_owned(),
                };
                request.validate()?;
                let (dir, meta) = self.metadata(id)?;
                let expected = self.expected_host(&meta)?;
                let alive = match self.host_identity(&dir, expected.as_ref())? {
                    Some(identity) if platform::identity_alive(&identity)? => {
                        let reply = ipc::call(&meta, &identity, &request)?;
                        if !reply.is_ok() {
                            return Err(reply.error.unwrap_or_else(|| {
                                error(ErrorCode::HostUnavailable, "invalid host reply")
                            }));
                        }
                        Some(true)
                    }
                    Some(_) => Some(false),
                    None => None,
                };
                sessions.push(json!({"session_id": meta.session_id, "created_at_ms": meta.created_at_ms, "host_alive": alive}));
            }
            sessions.sort_by(|a, b| a["session_id"].as_str().cmp(&b["session_id"].as_str()));
            Ok(json!({"sessions": sessions}))
        }
    }
}
