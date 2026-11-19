use std::sync::atomic::{AtomicU64, Ordering};

use super::session::ConnectionId;

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct RecordingOwner(ConnectionId);

impl RecordingOwner {
    pub(crate) fn new() -> Self {
        let mut current = NEXT_CONNECTION.load(Ordering::Relaxed);
        let id = loop {
            let next = current
                .checked_add(1)
                .expect("recording connection identifiers exhausted");
            match NEXT_CONNECTION.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(id) => break id,
                Err(observed) => current = observed,
            }
        };
        Self(ConnectionId(id))
    }

    pub(crate) fn id(&self) -> ConnectionId {
        self.0
    }
}

impl Drop for RecordingOwner {
    fn drop(&mut self) {
        super::control_handler::recording_owner_closed(self.0);
    }
}

pub(crate) struct BridgeCredential {
    generation: uuid::Uuid,
    nonce: uuid::Uuid,
}

impl BridgeCredential {
    pub(crate) fn new() -> Self {
        Self {
            generation: uuid::Uuid::new_v4(),
            nonce: uuid::Uuid::new_v4(),
        }
    }

    pub(crate) fn wire(&self) -> String {
        format!("{}:{}", self.generation, self.nonce)
    }

    pub(crate) fn unwrap<'a>(&self, message: &'a str) -> Option<&'a str> {
        let (header, line) = message.split_once('\n')?;
        let (generation, nonce) = header
            .strip_prefix("__rdog_record_bridge__:")?
            .split_once(':')?;
        if uuid::Uuid::parse_str(generation).ok()? != self.generation
            || uuid::Uuid::parse_str(nonce).ok()? != self.nonce
        {
            return None;
        }
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_rejects_wrong_nonce_or_generation_and_old_generation() {
        let current = BridgeCredential::new();
        let previous = BridgeCredential::new();
        let command = "@record-stop#7:{\"recording_id\":\"recording\"}";
        let valid = format!("__rdog_record_bridge__:{}\n{command}", current.wire());
        assert_eq!(current.unwrap(&valid), Some(command));
        let old = format!("__rdog_record_bridge__:{}\n{command}", previous.wire());
        assert_eq!(current.unwrap(&old), None);
        let wrong_nonce = format!(
            "__rdog_record_bridge__:{}:{}\n{command}",
            current.generation, previous.nonce
        );
        let wrong_generation = format!(
            "__rdog_record_bridge__:{}:{}\n{command}",
            previous.generation, current.nonce
        );
        assert_eq!(current.unwrap(&wrong_nonce), None);
        assert_eq!(current.unwrap(&wrong_generation), None);
    }
}
