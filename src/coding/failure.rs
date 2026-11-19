use super::model::{CodingError, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

const SLOT_BYTES: usize = 4096;
const HEADER_BYTES: usize = 36;
const FILE_NAME: &str = "host-failure.bin";

pub(super) struct FailureSlot(File);

impl FailureSlot {
    pub(super) fn create(root: &Path, host_instance_id: &str) -> Result<Self> {
        let mut slot = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join(FILE_NAME))?;
        let mut bytes = [0u8; SLOT_BYTES];
        for (index, chunk) in bytes[HEADER_BYTES..].chunks_mut(32).enumerate() {
            let mut hasher = Sha256::new();
            hasher.update(host_instance_id.as_bytes());
            hasher.update(index.to_le_bytes());
            let digest = hasher.finalize();
            chunk.copy_from_slice(&digest[..chunk.len()]);
        }
        slot.write_all(&bytes)?;
        slot.sync_all()?;
        Ok(Self(slot))
    }

    pub(super) fn record(&mut self, host_instance_id: &str, failure: &CodingError) -> Result<()> {
        let body = serde_json::to_vec(&(host_instance_id, failure))?;
        if body.len() > SLOT_BYTES - HEADER_BYTES {
            return Err(CodingError::new(
                ErrorCode::Storage,
                "host failure record is too large",
            ));
        }
        let mut bytes = [0u8; SLOT_BYTES];
        bytes[..4].copy_from_slice(&(body.len() as u32).to_le_bytes());
        bytes[4..HEADER_BYTES].copy_from_slice(&Sha256::digest(&body));
        bytes[HEADER_BYTES..HEADER_BYTES + body.len()].copy_from_slice(&body);
        self.0.seek(SeekFrom::Start(0))?;
        self.0.write_all(&bytes)?;
        self.0.sync_all()?;
        Ok(())
    }

    pub(super) fn read(root: &Path, host_instance_id: &str) -> Result<Option<CodingError>> {
        let mut slot = match File::open(root.join(FILE_NAME)) {
            Ok(slot) => slot,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = [0u8; SLOT_BYTES];
        slot.read_exact(&mut bytes)?;
        let len = u32::from_le_bytes(bytes[..4].try_into().expect("fixed header")) as usize;
        if len == 0 {
            return Ok(None);
        }
        if len > SLOT_BYTES - HEADER_BYTES
            || Sha256::digest(&bytes[HEADER_BYTES..HEADER_BYTES + len]).as_slice()
                != &bytes[4..HEADER_BYTES]
        {
            return Err(CodingError::new(
                ErrorCode::Storage,
                "host failure record is corrupt",
            ));
        }
        let (identity, failure): (String, CodingError) =
            serde_json::from_slice(&bytes[HEADER_BYTES..HEADER_BYTES + len])?;
        if identity != host_instance_id {
            return Err(CodingError::new(
                ErrorCode::HostIdentityMismatch,
                "host failure identity mismatch",
            ));
        }
        Ok(Some(failure))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_record_write_does_not_fabricate_a_failure() {
        let root = std::env::temp_dir().join(format!("rdog-failure-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        drop(FailureSlot::create(&root, "host-1").unwrap());
        let mut read_only = FailureSlot(File::open(root.join(FILE_NAME)).unwrap());
        let attempted = CodingError::new(ErrorCode::Storage, "disk full");
        assert!(read_only.record("host-1", &attempted).is_err());
        drop(read_only);
        assert!(FailureSlot::read(&root, "host-1").unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
