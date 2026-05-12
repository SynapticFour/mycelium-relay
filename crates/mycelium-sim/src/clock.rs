pub trait VirtualClock: Send {
    fn now_ms(&self) -> u64;
    fn advance_to(&mut self, timestamp_ms: u64);
}

#[derive(Debug, Default, Clone)]
pub struct DeterministicClock {
    now_ms: u64,
}

impl DeterministicClock {
    pub fn new(start_ms: u64) -> Self {
        Self { now_ms: start_ms }
    }
}

impl VirtualClock for DeterministicClock {
    fn now_ms(&self) -> u64 {
        self.now_ms
    }

    fn advance_to(&mut self, timestamp_ms: u64) {
        self.now_ms = self.now_ms.max(timestamp_ms);
    }
}
