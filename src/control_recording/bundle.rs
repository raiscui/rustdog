//! 确定性 Recording Bundle 写入器。
//!
//! 这里只负责把已冻结 Journal 和派生 flow 打包为 POSIX USTAR。
//! Replay reader、evidence 收集和真实 compiler 由后续阶段负责。

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::journal::JOURNAL_SCHEMA;

pub const BUNDLE_SCHEMA: &str = "rdog.recording.bundle.v1";
pub const FLOW_SCHEMA: &str = "rdog.flow.v1";
pub const MAX_BUNDLE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum BundleError {
    Io(io::Error),
    Json(serde_json::Error),
    UnsafePath(String),
    TooLarge { size: u64, limit: u64 },
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "bundle I/O: {err}"),
            Self::Json(err) => write!(f, "bundle JSON: {err}"),
            Self::UnsafePath(path) => write!(f, "unsafe bundle path: {path}"),
            Self::TooLarge { size, limit } => write!(f, "bundle too large: {size} > {limit}"),
        }
    }
}

impl std::error::Error for BundleError {}

impl From<io::Error> for BundleError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
struct ManifestFile {
    media_type: &'static str,
    path: &'static str,
    role: &'static str,
    sha256: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct NameVersion<'a> {
    name: &'a str,
    version: &'a str,
}

#[derive(Debug, Serialize)]
struct RedactionSummary {
    segment_count: u64,
}

#[derive(Debug, Serialize)]
struct Manifest<'a> {
    archive_format: &'static str,
    compiler: NameVersion<'static>,
    files: Vec<ManifestFile>,
    flow_schema: &'static str,
    journal_schema: &'static str,
    producer: NameVersion<'static>,
    recording_id: &'a str,
    redaction_summary: RedactionSummary,
    schema: &'static str,
    started_at_unix_ms: u64,
}

/// 写出并原子提交一个最小可回放 Bundle。
pub fn write_bundle(
    output_dir: &Path,
    recording_id: &str,
    started_at_unix_ms: u64,
    journal_path: &Path,
    flow: &Value,
    redaction_segment_count: u64,
) -> Result<Bundle, BundleError> {
    validate_recording_id(recording_id)?;
    fs::create_dir_all(output_dir)?;

    let journal = fs::read(journal_path)?;
    let flow = canonical_json(flow)?;
    let files = vec![
        manifest_file("flow.json", "flow", "application/json", &flow),
        manifest_file("journal.jsonl", "journal", "application/x-ndjson", &journal),
    ];
    let manifest = canonical_json(&serde_json::to_value(Manifest {
        archive_format: "posix-tar",
        compiler: NameVersion {
            name: "rdog-replay-compiler",
            version: "1",
        },
        files,
        flow_schema: FLOW_SCHEMA,
        journal_schema: JOURNAL_SCHEMA,
        producer: NameVersion {
            name: "rdog",
            version: env!("CARGO_PKG_VERSION"),
        },
        recording_id,
        redaction_summary: RedactionSummary {
            segment_count: redaction_segment_count,
        },
        schema: BUNDLE_SCHEMA,
        started_at_unix_ms,
    })?)?;

    let filename = format!("{recording_id}.rdogrec.tar");
    let final_path = output_dir.join(&filename);
    let staging_path = output_dir.join(format!(".{filename}.staging"));
    let result = write_archive(
        &staging_path,
        &[
            ("manifest.json", &manifest),
            ("journal.jsonl", &journal),
            ("flow.json", &flow),
        ],
    );
    if let Err(err) = result {
        let _ = fs::remove_file(&staging_path);
        return Err(err);
    }

    let size_bytes = fs::metadata(&staging_path)?.len();
    if size_bytes > MAX_BUNDLE_BYTES {
        let _ = fs::remove_file(&staging_path);
        return Err(BundleError::TooLarge {
            size: size_bytes,
            limit: MAX_BUNDLE_BYTES,
        });
    }
    let sha256 = hash_file(&staging_path)?;
    fs::rename(&staging_path, &final_path)?;
    File::open(output_dir)?.sync_all()?;
    Ok(Bundle {
        path: final_path,
        size_bytes,
        sha256,
    })
}

fn manifest_file(
    path: &'static str,
    role: &'static str,
    media_type: &'static str,
    data: &[u8],
) -> ManifestFile {
    ManifestFile {
        media_type,
        path,
        role,
        sha256: hash_bytes(data),
        size_bytes: data.len() as u64,
    }
}

/// serde_json 的 `Value::Object` 使用有序 map;递归转换保证配置变化时仍确定。
pub(crate) fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, BundleError> {
    fn sort(value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, sort(v)))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.into_iter().map(sort).collect()),
            other => other,
        }
    }
    let mut bytes = serde_json::to_vec(&sort(serde_json::to_value(value)?))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_archive(path: &Path, entries: &[(&str, &[u8])]) -> Result<(), BundleError> {
    let mut file = File::create(path)?;
    for (name, data) in entries {
        validate_entry_path(name)?;
        file.write_all(&ustar_header(name, data.len() as u64)?)?;
        file.write_all(data)?;
        let padding = (512 - data.len() % 512) % 512;
        file.write_all(&[0_u8; 512][..padding])?;
    }
    file.write_all(&[0_u8; 1024])?;
    file.sync_all()?;
    Ok(())
}

fn ustar_header(name: &str, size: u64) -> Result<[u8; 512], BundleError> {
    validate_entry_path(name)?;
    let mut header = [0_u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut header[100..108], 0o644)?;
    write_octal(&mut header[108..116], 0)?;
    write_octal(&mut header[116..124], 0)?;
    write_octal(&mut header[124..136], size)?;
    write_octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    write_checksum(&mut header[148..156], checksum)?;
    Ok(header)
}

fn write_octal(field: &mut [u8], value: u64) -> Result<(), BundleError> {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    if text.len() >= field.len() {
        return Err(BundleError::TooLarge {
            size: value,
            limit: u64::MAX,
        });
    }
    field[..text.len()].copy_from_slice(text.as_bytes());
    field[field.len() - 1] = 0;
    Ok(())
}

fn write_checksum(field: &mut [u8], value: u64) -> Result<(), BundleError> {
    let text = format!("{value:06o}\0 ");
    if text.len() != field.len() {
        return Err(BundleError::TooLarge {
            size: value,
            limit: u64::MAX,
        });
    }
    field.copy_from_slice(text.as_bytes());
    Ok(())
}

fn validate_recording_id(id: &str) -> Result<(), BundleError> {
    if id.is_empty()
        || id.len() > 80
        || !id.is_ascii()
        || id.contains(['/', '\\'])
        || id == "."
        || id == ".."
    {
        return Err(BundleError::UnsafePath(id.to_owned()));
    }
    Ok(())
}

fn validate_entry_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty()
        || path.len() > 100
        || !path.is_ascii()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(BundleError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hash_file(path: &Path) -> Result<String, BundleError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
