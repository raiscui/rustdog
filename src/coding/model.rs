use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, io, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Disabled,
    AuthenticationRequired,
    UnsupportedPlatform,
    InvalidRequest,
    NotFound,
    Busy,
    Conflict,
    Starting,
    SessionClosed,
    HostUnavailable,
    HostIdentityMismatch,
    UnknownOutcome,
    Storage,
    JournalIncomplete,
    JournalCorrupt,
    CursorInvalid,
    CursorExpired,
    QuotaExceeded,
    InteractionClosed,
    Engine,
    Io,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingError {
    pub code: ErrorCode,
    pub message: String,
}
impl CodingError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl fmt::Display for CodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for CodingError {}
impl From<io::Error> for CodingError {
    fn from(e: io::Error) -> Self {
        Self::new(ErrorCode::Io, e.to_string())
    }
}
impl From<serde_json::Error> for CodingError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::Storage, e.to_string())
    }
}
pub type Result<T, E = CodingError> = std::result::Result<T, E>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CodingConfig {
    pub enabled: bool,
    pub state_dir: Option<PathBuf>,
    pub omp_executable: Option<PathBuf>,
    pub profile: String,
    pub environment: BTreeMap<String, String>,
    pub extensions: Vec<PathBuf>,
    pub max_journal_bytes: u64,
}
impl Default for CodingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            state_dir: None,
            omp_executable: None,
            profile: "default".into(),
            environment: BTreeMap::new(),
            extensions: vec![],
            max_journal_bytes: 64 * 1024 * 1024,
        }
    }
}
impl CodingConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.profile.trim().is_empty()
            || self.profile.contains('\0')
            || self.max_journal_bytes == 0
            || self.state_dir.as_ref().is_some_and(|p| !p.is_absolute())
            || self
                .omp_executable
                .as_ref()
                .is_some_and(|p| !p.is_absolute())
            || self.extensions.iter().any(|p| !p.is_absolute())
            || self.environment.iter().any(|(k, v)| {
                k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0')
            })
        {
            return Err(CodingError::new(
                ErrorCode::InvalidRequest,
                "invalid coding configuration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InteractionAnswer {
    Confirm { confirmed: bool },
    Select { value: Option<String> },
    Input { value: Option<String> },
    Editor { value: Option<String> },
}
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionView {
    pub interaction_id: String,
    pub task_id: String,
    pub kind: String,
    pub title: String,
    pub message: Option<String>,
    pub options: Vec<String>,
    pub state: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum CodingRequest {
    #[serde(rename = "session.open")]
    SessionOpen {
        request_id: String,
        cwd: PathBuf,
        profile: Option<String>,
    },
    #[serde(rename = "session.list")]
    SessionList {},
    #[serde(rename = "session.status")]
    SessionStatus { session_id: String },
    #[serde(rename = "session.close")]
    SessionClose {
        session_id: String,
        request_id: String,
    },
    #[serde(rename = "task.submit")]
    TaskSubmit {
        session_id: String,
        request_id: String,
        prompt: String,
    },
    #[serde(rename = "task.status")]
    TaskStatus { session_id: String, task_id: String },
    #[serde(rename = "task.output")]
    TaskOutput {
        session_id: String,
        task_id: String,
        cursor: Option<String>,
        limit: Option<u16>,
    },
    #[serde(rename = "task.cancel")]
    TaskCancel {
        session_id: String,
        task_id: String,
        request_id: String,
    },
    #[serde(rename = "interaction.list")]
    InteractionList { session_id: String },
    #[serde(rename = "interaction.respond")]
    InteractionRespond {
        session_id: String,
        interaction_id: String,
        request_id: String,
        answer: InteractionAnswer,
    },
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn valid_absolute_path(path: &std::path::Path) -> bool {
    let Some(value) = path.to_str() else {
        return false;
    };
    if value.is_empty() || value.len() > 32767 || value.contains('\0') {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes[0] == b'/' {
        return true;
    }
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return true;
    }
    if let Some(rest) = value.strip_prefix("\\\\") {
        let mut parts = rest.split('\\');
        return parts.next().is_some_and(|p| !p.is_empty())
            && parts.next().is_some_and(|p| !p.is_empty());
    }
    false
}
impl CodingRequest {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::SessionOpen { .. } | Self::SessionList {} => None,
            Self::SessionStatus { session_id }
            | Self::SessionClose { session_id, .. }
            | Self::TaskSubmit { session_id, .. }
            | Self::TaskStatus { session_id, .. }
            | Self::TaskOutput { session_id, .. }
            | Self::TaskCancel { session_id, .. }
            | Self::InteractionList { session_id }
            | Self::InteractionRespond { session_id, .. } => Some(session_id),
        }
    }
    pub fn validate(&self) -> Result<()> {
        let valid = self.session_id().is_none_or(valid_id)
            && match self {
                Self::SessionOpen {
                    request_id,
                    cwd,
                    profile,
                } => {
                    valid_id(request_id)
                        && valid_absolute_path(cwd)
                        && profile
                            .as_ref()
                            .is_none_or(|p| !p.trim().is_empty() && !p.contains('\0'))
                }
                Self::SessionClose { request_id, .. } => valid_id(request_id),
                Self::TaskSubmit {
                    request_id, prompt, ..
                } => {
                    valid_id(request_id) && !prompt.trim().is_empty() && prompt.len() <= 128 * 1024
                }
                Self::TaskStatus { task_id, .. } => valid_id(task_id),
                Self::TaskOutput { task_id, limit, .. } => {
                    valid_id(task_id) && limit.is_none_or(|n| (1..=128).contains(&n))
                }
                Self::TaskCancel {
                    task_id,
                    request_id,
                    ..
                } => valid_id(task_id) && valid_id(request_id),
                Self::InteractionRespond {
                    interaction_id,
                    request_id,
                    ..
                } => valid_id(interaction_id) && valid_id(request_id),
                Self::SessionList {}
                | Self::SessionStatus { .. }
                | Self::InteractionList { .. } => true,
            };
        if !valid || serde_json::to_vec(self)?.len() > 256 * 1024 {
            return Err(CodingError::new(
                ErrorCode::InvalidRequest,
                "invalid coding request",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingReply {
    pub schema: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CodingError>,
}
impl CodingReply {
    #[cfg(windows)]
    pub fn success(result: Value) -> Self {
        Self {
            schema: "rdog.coding.v1".into(),
            ok: true,
            result: Some(result),
            error: None,
        }
    }
    pub fn failure(error: CodingError) -> Self {
        Self {
            schema: "rdog.coding.v1".into(),
            ok: false,
            result: None,
            error: Some(error),
        }
    }
    pub fn is_ok(&self) -> bool {
        self.ok
    }
}
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMetadata {
    pub session_id: String,
    pub host_instance_id: String,
    pub cursor_epoch: String,
    pub created_at_ms: u64,
    pub cwd: PathBuf,
    pub profile: String,
    pub omp_executable: PathBuf,
    pub extensions: Vec<PathBuf>,
    pub max_journal_bytes: u64,
}
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub created_at: u64,
}
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineIdentity {
    pub process: ProcessIdentity,
    pub native_session_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_validates_target_paths_independently_of_client_platform() {
        for path in [
            "/tmp/project",
            r"C:\work\project",
            r"\\server\share\project",
        ] {
            let request = CodingRequest::SessionOpen {
                request_id: "open".into(),
                cwd: path.into(),
                profile: None,
            };
            assert!(request.validate().is_ok(), "{path}");
        }
        for path in ["relative", r"C:relative", r"\\server"] {
            let request = CodingRequest::SessionOpen {
                request_id: "open".into(),
                cwd: path.into(),
                profile: None,
            };
            assert!(request.validate().is_err(), "{path}");
        }
    }
}
