// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone)]
pub struct LinkProfile {
    pub latency_ms: u64,
    pub loss_per_mille: u16,
    pub bandwidth_bytes_per_sec: u64,
}

impl Default for LinkProfile {
    fn default() -> Self {
        Self {
            latency_ms: 10,
            loss_per_mille: 0,
            bandwidth_bytes_per_sec: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledEvent<T> {
    pub at_ms: u64,
    pub seq: u64,
    pub payload: T,
}

impl<T> Ord for ScheduledEvent<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .at_ms
            .cmp(&self.at_ms)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl<T> PartialOrd for ScheduledEvent<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> PartialEq for ScheduledEvent<T> {
    fn eq(&self, other: &Self) -> bool {
        self.at_ms == other.at_ms && self.seq == other.seq
    }
}

impl<T> Eq for ScheduledEvent<T> {}

#[derive(Debug)]
pub struct EventScheduler<T> {
    next_seq: u64,
    queue: BinaryHeap<ScheduledEvent<T>>,
}

impl<T> EventScheduler<T> {
    pub fn push(&mut self, at_ms: u64, payload: T) {
        self.next_seq += 1;
        self.queue.push(ScheduledEvent {
            at_ms,
            seq: self.next_seq,
            payload,
        });
    }

    pub fn pop_ready(&mut self, now_ms: u64) -> Option<ScheduledEvent<T>> {
        let ready = self
            .queue
            .peek()
            .map(|event| event.at_ms <= now_ms)
            .unwrap_or(false);
        if ready {
            self.queue.pop()
        } else {
            None
        }
    }

    pub fn next_timestamp(&self) -> Option<u64> {
        self.queue.peek().map(|event| event.at_ms)
    }
}

impl<T> Default for EventScheduler<T> {
    fn default() -> Self {
        Self {
            next_seq: 0,
            queue: BinaryHeap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EventScheduler;

    #[test]
    fn scheduler_orders_by_time_then_sequence() {
        let mut s = EventScheduler::default();
        s.push(20, "b");
        s.push(10, "a1");
        s.push(10, "a2");

        let first = s.pop_ready(10).expect("first");
        let second = s.pop_ready(10).expect("second");
        let third = s.pop_ready(20).expect("third");

        assert_eq!(first.payload, "a1");
        assert_eq!(second.payload, "a2");
        assert_eq!(third.payload, "b");
    }

    #[test]
    fn scheduler_waits_until_ready() {
        let mut s = EventScheduler::default();
        s.push(50, 1u8);
        assert!(s.pop_ready(49).is_none());
        assert!(s.pop_ready(50).is_some());
    }
}
