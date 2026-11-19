use super::{BoundedQueue, CaptureSpec, CaptureStop, RecorderCapture, RecorderError};

pub struct StubCapture {
    queue: BoundedQueue,
}
impl StubCapture {
    pub fn new() -> Self {
        Self {
            queue: BoundedQueue::new(),
        }
    }
}
impl RecorderCapture for StubCapture {
    fn start(&mut self, _: &CaptureSpec) -> Result<(), RecorderError> {
        Err(RecorderError::Unavailable)
    }
    fn stop(
        &mut self,
        _: &mut dyn FnMut(&BoundedQueue) -> std::io::Result<()>,
    ) -> Result<CaptureStop, RecorderError> {
        Err(RecorderError::Unavailable)
    }
    fn parameter_boundary(&mut self) -> Result<(), RecorderError> {
        Err(RecorderError::Unavailable)
    }
    fn queue(&self) -> &BoundedQueue {
        &self.queue
    }
    fn wall_clock_anchor_ms(&self) -> u64 {
        0
    }
    fn cleanup_pending(&mut self) -> bool {
        false
    }
    fn is_healthy(&self) -> bool {
        false
    }
}
