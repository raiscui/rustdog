use super::{
    journal::{JournalWriter, SealedRecording},
    BoundedQueue, CaptureSpec, RecorderCapture,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Semantic,
    Physical,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub u64);

pub(crate) const JOURNAL_MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecordingPolicy {
    pub duration_ms: u64,
    pub lease_ms: u64,
    pub renew_interval_ms: u64,
    pub journal_max_bytes: u64,
}
impl RecordingPolicy {
    pub fn new(duration_ms: Option<u64>) -> Option<Self> {
        let duration_ms = duration_ms.unwrap_or(300_000);
        if duration_ms == 0 || duration_ms > 300_000 {
            return None;
        }
        Some(Self {
            duration_ms,
            lease_ms: 45_000,
            renew_interval_ms: 15_000,
            journal_max_bytes: JOURNAL_MAX_BYTES,
        })
    }
}

pub struct Session {
    pub id: String,
    pub owner: ConnectionId,
    pub policy: RecordingPolicy,
    capture: Box<dyn RecorderCapture>,
    journal: JournalWriter,
    next_seq: u64,
    pub deadline: Instant,
    pub lease_deadline: Instant,
}
impl Session {
    pub fn start(
        id: String,
        owner: ConnectionId,
        profile: Profile,
        spec: CaptureSpec,
        policy: RecordingPolicy,
        dir: &Path,
        mut capture: Box<dyn RecorderCapture>,
    ) -> Result<Self, (io::Error, Box<dyn RecorderCapture>)> {
        if let Err(err) = capture.start(&spec) {
            return Err((io::Error::other(err), capture));
        }
        let journal = match JournalWriter::open(
            dir,
            &id,
            profile,
            capture.wall_clock_anchor_ms(),
            policy.journal_max_bytes,
        ) {
            Ok(journal) => journal,
            Err(err) => {
                let _ = capture.stop(&mut discard_events);
                return Err((err, capture));
            }
        };
        let now = Instant::now();
        Ok(Self {
            id,
            owner,
            policy,
            capture,
            journal,
            next_seq: 0,
            deadline: now + Duration::from_millis(policy.duration_ms),
            lease_deadline: now + Duration::from_millis(policy.lease_ms),
        })
    }
    pub fn renew(&mut self) {
        self.lease_deadline = Instant::now() + Duration::from_millis(self.policy.lease_ms);
    }
    pub fn drain(&mut self) -> io::Result<()> {
        if self.capture.queue().overflowed() || !self.capture.is_healthy() {
            return Err(io::Error::other("capture fault or queue overflow"));
        }
        drain_events(self.capture.queue(), &mut self.journal, &mut self.next_seq)
    }
    pub fn nearing_limit(&self) -> bool {
        self.journal.bytes() >= self.policy.journal_max_bytes.saturating_sub(1024 * 1024)
    }
    pub fn mark(&mut self, mark: &str) -> io::Result<()> {
        if mark == "parameter_boundary" {
            self.capture
                .parameter_boundary()
                .map_err(io::Error::other)?;
        }
        self.journal.mark(mark)
    }
    pub fn seal(mut self) -> Result<SealedRecording, (io::Error, Box<dyn RecorderCapture>)> {
        let journal = &mut self.journal;
        let next_seq = &mut self.next_seq;
        let stop = match self
            .capture
            .stop(&mut |queue| drain_events(queue, journal, next_seq))
        {
            Ok(stop) => stop,
            Err(err) => return Err((self.journal.abort(io::Error::other(err)), self.capture)),
        };
        if self.capture.cleanup_pending() {
            return Err((
                self.journal
                    .abort(io::Error::other("capture cleanup pending")),
                self.capture,
            ));
        }
        if let Err(err) = self.drain_frozen() {
            return Err((self.journal.abort(err), self.capture));
        }
        if stop.last_sequence != self.next_seq.checked_sub(1) {
            return Err((
                self.journal
                    .abort(io::Error::other("capture freeze boundary mismatch")),
                self.capture,
            ));
        }
        self.journal.seal().map_err(|err| (err, self.capture))
    }
    fn drain_frozen(&mut self) -> io::Result<()> {
        drain_events(self.capture.queue(), &mut self.journal, &mut self.next_seq)
    }
    pub fn cancel(mut self) -> Result<(), (io::Error, Box<dyn RecorderCapture>)> {
        let stopped = self
            .capture
            .stop(&mut discard_events)
            .map_err(io::Error::other);
        let removed = self.journal.discard();
        let pending = self.capture.cleanup_pending();
        match removed.and(stopped.map(|_| ())) {
            Err(err) => Err((err, self.capture)),
            Ok(()) if pending => Err((io::Error::other("capture cleanup pending"), self.capture)),
            Ok(()) => Ok(()),
        }
    }
}

fn discard_events(queue: &BoundedQueue) -> io::Result<()> {
    queue.drain_all(drop);
    Ok(())
}

fn drain_events(
    queue: &BoundedQueue,
    journal: &mut JournalWriter,
    next: &mut u64,
) -> io::Result<()> {
    if queue.overflowed() {
        return Err(io::Error::other("capture queue overflow"));
    }
    let mut error = None;
    queue.drain_all(|event| {
        if error.is_some() {
            return;
        }
        if event.sequence != *next {
            error = Some(io::Error::other("capture sequence gap"));
            return;
        }
        if let Err(err) = journal.event(&event) {
            error = Some(err);
            return;
        }
        *next += 1;
    });
    if queue.overflowed() {
        return Err(io::Error::other("capture queue overflow"));
    }
    error.map_or(Ok(()), Err)
}
