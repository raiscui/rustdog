use super::{session::Profile, CaptureEvent};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const JOURNAL_SCHEMA: &str = "rdog.recording.v2";

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct SealedRecording {
    pub path: PathBuf,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub cleanup_residual: Option<PathBuf>,
}

#[derive(Debug)]
pub struct StagingCleanupError {
    pub path: PathBuf,
    pub source: io::Error,
}
impl std::fmt::Display for StagingCleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "staging cleanup failed at {}: {}",
            self.path.display(),
            self.source
        )
    }
}
impl std::error::Error for StagingCleanupError {}

#[derive(Serialize)]
struct JournalEntry<'a, T> {
    schema: &'static str,
    recording_id: &'a str,
    journal_seq: u64,
    kind: &'a str,
    monotonic_ns: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_monotonic_ns: Option<u64>,
    payload: T,
}

pub struct JournalWriter {
    writer: Option<BufWriter<File>>,
    staging: PathBuf,
    target: PathBuf,
    id: String,
    seq: u64,
    bytes: u64,
    limit: u64,
    origin: Instant,
    line: Vec<u8>,
    cleanup_attempted: bool,
}
impl JournalWriter {
    pub fn open(
        dir: &Path,
        id: &str,
        profile: Profile,
        anchor_ms: u64,
        limit: u64,
    ) -> io::Result<Self> {
        super::private_files::create_directory(dir)?;
        let staging = dir.join(format!(".{id}.staging"));
        let target = dir.join(format!("{id}.rdogrec.jsonl"));
        let file = super::private_files::create_new(&staging)?;
        let mut result = Self {
            writer: Some(BufWriter::new(file)),
            staging,
            target,
            id: id.to_owned(),
            seq: 0,
            bytes: 0,
            limit,
            origin: Instant::now(),
            line: Vec::with_capacity(1024),
            cleanup_attempted: false,
        };
        if let Err(err) = result.append("session_start", None, json!({"type":"start", "profile":profile, "started_at_unix_ms":anchor_ms, "platform":{"os":"windows", "capture_backend":"windows_helper"}, "coordinate_space":"native_physical_screen", "lanes":{"physical":"available","window":"available","semantic":if profile == Profile::Semantic {"available"} else {"disabled"}}, "coverage":"best_effort", "text_mode":super::TEXT_MODE, "shortcut_mode":"unsupported_without_evidence", "replay_status":"not_compiled"})) { return Err(result.abort(err)); }
        Ok(result)
    }
    fn append<T: Serialize>(
        &mut self,
        kind: &str,
        capture: Option<&CaptureEvent>,
        payload: T,
    ) -> io::Result<()> {
        let entry = JournalEntry {
            schema: JOURNAL_SCHEMA,
            recording_id: &self.id,
            journal_seq: self.seq,
            kind,
            monotonic_ns: self.origin.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            capture_seq: capture.map(|event| event.sequence),
            captured_monotonic_ns: capture.map(|event| event.monotonic_ns),
            payload,
        };
        self.line.clear();
        serde_json::to_writer(&mut self.line, &entry).map_err(io::Error::other)?;
        self.line.push(b'\n');
        if self.bytes.saturating_add(self.line.len() as u64) > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "recording journal budget exceeded",
            ));
        }
        self.writer
            .as_mut()
            .ok_or_else(|| io::Error::other("journal closed"))?
            .write_all(&self.line)?;
        self.bytes += self.line.len() as u64;
        self.seq += 1;
        Ok(())
    }
    pub fn event(&mut self, event: &CaptureEvent) -> io::Result<()> {
        use super::model::CapturedInput;
        let kind = match &event.input {
            CapturedInput::MouseDown { .. }
            | CapturedInput::MouseUp { .. }
            | CapturedInput::MouseMove { .. }
            | CapturedInput::Scroll { .. } => "physical",
            CapturedInput::DisplayContext { .. } | CapturedInput::WindowContext { .. } => "context",
            CapturedInput::Redaction { .. } => "redaction",
            CapturedInput::SemanticCandidate { .. } | CapturedInput::SemanticMissing { .. } => {
                "semantic"
            }
        };
        self.append(kind, Some(event), &event.input)
    }
    pub fn mark(&mut self, mark: &str) -> io::Result<()> {
        self.append("mark", None, json!({"type":mark}))
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    #[cfg(test)]
    pub(super) fn replace_writer_with_read_only_for_test(&mut self) -> io::Result<()> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }
        self.writer = Some(BufWriter::new(File::open(&self.staging)?));
        Ok(())
    }
    pub fn seal(mut self) -> io::Result<SealedRecording> {
        let prepared = self.prepare_seal();
        match prepared {
            Ok((filename, sha256)) => {
                let cleanup_residual = self.remove_staging().err().map(|_| self.staging.clone());
                Ok(SealedRecording {
                    path: self.target.clone(),
                    filename,
                    size_bytes: self.bytes,
                    sha256,
                    cleanup_residual,
                })
            }
            Err(err) => Err(self.abort(err)),
        }
    }
    fn prepare_seal(&mut self) -> io::Result<(String, String)> {
        self.append("session_terminal", None, json!({"type":"recorded"}))?;
        let mut writer = self
            .writer
            .take()
            .ok_or_else(|| io::Error::other("journal closed"))?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        let bytes = fs::read(&self.staging)?;
        if bytes.len() as u64 != self.bytes {
            return Err(io::Error::other("journal size mismatch"));
        }
        validate_sealed(&bytes, &self.id, self.seq)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let filename = self
            .target
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| io::Error::other("invalid filename"))?
            .to_owned();
        fs::hard_link(&self.staging, &self.target)?;
        Ok((filename, sha256))
    }
    fn remove_staging(&mut self) -> io::Result<()> {
        self.writer.take();
        self.cleanup_attempted = true;
        match fs::remove_file(&self.staging) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(io::Error::other(StagingCleanupError {
                path: self.staging.clone(),
                source,
            })),
        }
    }
    pub fn abort(mut self, cause: io::Error) -> io::Error {
        match self.remove_staging() {
            Ok(()) => cause,
            Err(cleanup) => cleanup,
        }
    }
    pub fn discard(mut self) -> io::Result<()> {
        self.remove_staging()
    }
}
impl Drop for JournalWriter {
    fn drop(&mut self) {
        if !self.cleanup_attempted {
            self.writer.take();
            let _ = fs::remove_file(&self.staging);
        }
    }
}
fn validate_sealed(bytes: &[u8], recording_id: &str, expected_lines: u64) -> io::Result<()> {
    if !bytes.ends_with(b"\n") {
        return Err(io::Error::other("journal missing final newline"));
    }
    let mut count = 0u64;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let entry: Value = serde_json::from_slice(line).map_err(io::Error::other)?;
        if entry["schema"] != JOURNAL_SCHEMA
            || entry["recording_id"] != recording_id
            || entry["journal_seq"].as_u64() != Some(count)
        {
            return Err(io::Error::other("invalid journal envelope"));
        }
        if count + 1 == expected_lines
            && (entry["kind"] != "session_terminal" || entry["payload"]["type"] != "recorded")
        {
            return Err(io::Error::other("invalid journal terminal"));
        }
        count += 1;
    }
    if count != expected_lines {
        return Err(io::Error::other("journal line count mismatch"));
    }
    Ok(())
}
