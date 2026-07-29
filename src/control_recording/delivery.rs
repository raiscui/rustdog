//! Recording Bundle 的 owner-only 单帧交付。

use std::{
    collections::{HashMap, VecDeque},
    fs,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

use crate::control_frames::SaveFileFrame;

use super::{
    bundle::{Bundle, MAX_BUNDLE_BYTES},
    session::ConnectionId,
};

pub const MAX_DELIVERY_FRAME_BYTES: u64 = 384 * 1024 * 1024;
const STOP_LIMIT: usize = 5;
const STOP_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryFailureReason {
    ChecksumMismatch,
    BundleTooLarge,
    Cancelled,
    RateLimited,
}

impl DeliveryFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::BundleTooLarge => "bundle_too_large",
            Self::Cancelled => "cancelled",
            Self::RateLimited => "rate_limited",
        }
    }
}

#[derive(Debug)]
pub enum DeliveryError {
    NotOwner,
    Rejected(DeliveryFailureReason),
    Io(std::io::Error),
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOwner => f.write_str("recording bundle delivery is owner-only"),
            Self::Rejected(reason) => write!(f, "delivery failed: {}", reason.as_str()),
            Self::Io(err) => write!(f, "delivery I/O: {err}"),
        }
    }
}
impl std::error::Error for DeliveryError {}
impl From<std::io::Error> for DeliveryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Default)]
pub struct DeliveryManager {
    stop_attempts: HashMap<ConnectionId, VecDeque<Instant>>,
}

impl DeliveryManager {
    /// 每连接最多 5 次 stop/秒。第 6 次 fail closed。
    pub fn check_record_stop(&mut self, connection: ConnectionId) -> Result<(), DeliveryError> {
        self.check_record_stop_at(connection, Instant::now())
    }

    fn check_record_stop_at(
        &mut self,
        connection: ConnectionId,
        now: Instant,
    ) -> Result<(), DeliveryError> {
        let attempts = self.stop_attempts.entry(connection).or_default();
        while attempts
            .front()
            .is_some_and(|at| now.duration_since(*at) >= STOP_WINDOW)
        {
            attempts.pop_front();
        }
        if attempts.len() >= STOP_LIMIT {
            return Err(DeliveryError::Rejected(DeliveryFailureReason::RateLimited));
        }
        attempts.push_back(now);
        Ok(())
    }

    pub fn frame_for_owner(
        &self,
        owner: ConnectionId,
        recipient: ConnectionId,
        request_id: Option<u64>,
        bundle: &Bundle,
    ) -> Result<SaveFileFrame, DeliveryError> {
        if recipient != owner {
            return Err(DeliveryError::NotOwner);
        }
        if bundle.size_bytes > MAX_BUNDLE_BYTES {
            return Err(DeliveryError::Rejected(
                DeliveryFailureReason::BundleTooLarge,
            ));
        }
        let bytes = fs::read(&bundle.path)?;
        let encoded = BASE64_STANDARD.encode(bytes);
        if encoded.len() as u64 > MAX_DELIVERY_FRAME_BYTES {
            return Err(DeliveryError::Rejected(
                DeliveryFailureReason::BundleTooLarge,
            ));
        }
        let filename = bundle
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(DeliveryError::Rejected(DeliveryFailureReason::Cancelled))?;
        Ok(SaveFileFrame {
            request_id,
            filename: filename.to_owned(),
            mime: "application/vnd.rdog.recording-bundle".to_owned(),
            encoding: "base64".to_owned(),
            data: encoded,
            quality: None,
            width: None,
            height: None,
        })
    }
}
