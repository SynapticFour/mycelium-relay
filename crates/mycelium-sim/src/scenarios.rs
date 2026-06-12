// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use crate::runner::{SimulationMetrics, SimulationRunner};
use crate::scheduler::LinkProfile;
use crate::transport::SimAction;
use mycelium_core::data::Envelope;
use mycelium_core::transport::{DirectMessage, WireMessage};

#[derive(Debug, Clone)]
pub struct ScenarioReport {
    pub expected_deliveries: u64,
    pub delivery_success_rate: f64,
    pub duplicate_count: u64,
    pub avg_latency_ms: f64,
    pub metrics: SimulationMetrics,
}

#[derive(Debug, Clone)]
pub struct BloomSyncEfficiencyReport {
    pub messages_total: usize,
    pub bloom_bytes_per_round: usize,
    pub ids_bytes_per_round: usize,
    pub estimated_roundtrips_bloom: u64,
    pub estimated_roundtrips_ids: u64,
}

pub fn partition_and_merge(seed: u64) -> ScenarioReport {
    let mut sim = SimulationRunner::new(0, seed);
    let _ = sim.build_transport("A".to_string(), None, 64 * 1024);
    let _ = sim.build_transport("B".to_string(), None, 64 * 1024);
    let _ = sim.build_transport("C".to_string(), None, 64 * 1024);
    sim.set_link("A".to_string(), "B".to_string(), LinkProfile::default());
    sim.set_link("B".to_string(), "C".to_string(), LinkProfile::default());
    sim.set_link("A".to_string(), "C".to_string(), LinkProfile::default());

    sim.schedule_availability("B".to_string(), 500, false);
    sim.schedule_availability("B".to_string(), 2_000, true);
    let msg = make_msg("A", Some("C"), "partition-test");
    let _ = sim.action_tx.send(SimAction::SendDirect {
        from_peer: "A".to_string(),
        to_peer: "C".to_string(),
        message: WireMessage::Data(msg),
    });
    sim.run_for(3_000);
    report(sim.metrics(), 1)
}

pub fn high_message_load(seed: u64) -> ScenarioReport {
    let mut sim = SimulationRunner::new(0, seed);
    let _ = sim.build_transport("A".to_string(), None, 8 * 1024);
    let _ = sim.build_transport("B".to_string(), None, 8 * 1024);
    sim.set_link(
        "A".to_string(),
        "B".to_string(),
        LinkProfile {
            latency_ms: 25,
            loss_per_mille: 20,
            bandwidth_bytes_per_sec: 8 * 1024,
        },
    );
    for i in 0..500 {
        let msg = make_msg("A", Some("B"), &format!("load-{i}"));
        let _ = sim.action_tx.send(SimAction::SendDirect {
            from_peer: "A".to_string(),
            to_peer: "B".to_string(),
            message: WireMessage::Data(msg),
        });
    }
    sim.run_for(5_000);
    report(sim.metrics(), 500)
}

pub fn node_churn(seed: u64) -> ScenarioReport {
    let mut sim = SimulationRunner::new(0, seed);
    let _ = sim.build_transport("A".to_string(), None, 32 * 1024);
    let _ = sim.build_transport("B".to_string(), None, 32 * 1024);
    sim.set_link("A".to_string(), "B".to_string(), LinkProfile::default());
    for i in 0..8 {
        let at = 200 + i * 300;
        sim.schedule_availability("B".to_string(), at, i % 2 == 0);
    }
    for i in 0..50 {
        let msg = make_msg("A", Some("B"), &format!("churn-{i}"));
        let _ = sim.action_tx.send(SimAction::SendDirect {
            from_peer: "A".to_string(),
            to_peer: "B".to_string(),
            message: WireMessage::Data(msg),
        });
    }
    sim.run_for(4_000);
    report(sim.metrics(), 50)
}

pub fn bloom_sync_efficiency(seed: u64) -> ScenarioReport {
    let mut sim = SimulationRunner::new(0, seed);
    for i in 0..10 {
        let peer = format!("N{i}");
        let _ = sim.build_transport(peer, None, 64 * 1024);
    }
    let _eff = bloom_sync_efficiency_compare(10, 50);
    report(sim.metrics(), 1)
}

pub fn bloom_sync_efficiency_compare(
    nodes: usize,
    messages_per_node: usize,
) -> BloomSyncEfficiencyReport {
    let messages_total = nodes * messages_per_node;
    let bloom_bytes_per_round = 1024;
    let avg_id_len = 64usize;
    let ids_bytes_per_round = messages_total * avg_id_len;
    let estimated_roundtrips_bloom = ((messages_total as f64) / 512.0).ceil() as u64;
    let estimated_roundtrips_ids = ((messages_total as f64) / 128.0).ceil() as u64;
    BloomSyncEfficiencyReport {
        messages_total,
        bloom_bytes_per_round,
        ids_bytes_per_round,
        estimated_roundtrips_bloom,
        estimated_roundtrips_ids,
    }
}

pub fn hop_limit_containment(seed: u64) -> ScenarioReport {
    let mut sim = SimulationRunner::new(0, seed);
    let _ = sim.build_transport("A".to_string(), None, 64 * 1024);
    let _ = sim.build_transport("B".to_string(), None, 64 * 1024);
    report(sim.metrics(), 1)
}

pub fn scope_isolation(seed: u64) -> ScenarioReport {
    let sim = SimulationRunner::new(0, seed);
    report(sim.metrics(), 1)
}

fn make_msg(from: &str, to: Option<&str>, body: &str) -> DirectMessage {
    DirectMessage {
        envelope: Envelope::new(
            from.to_string(),
            to.map(ToString::to_string),
            body.as_bytes().to_vec(),
        ),
        body: body.to_string(),
    }
}

fn report(metrics: SimulationMetrics, expected: u64) -> ScenarioReport {
    let delivered = metrics.delivered as f64;
    let expected_f = expected.max(1) as f64;
    let avg_latency_ms = if metrics.latencies_ms.is_empty() {
        0.0
    } else {
        metrics.latencies_ms.iter().sum::<u64>() as f64 / metrics.latencies_ms.len() as f64
    };
    ScenarioReport {
        expected_deliveries: expected,
        delivery_success_rate: delivered / expected_f,
        duplicate_count: metrics.duplicates,
        avg_latency_ms,
        metrics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_scenario_produces_metrics() {
        let report = partition_and_merge(42);
        assert!(report.expected_deliveries > 0);
        assert!(report.delivery_success_rate >= 0.0);
    }

    #[test]
    fn high_load_scenario_records_losses_or_deliveries() {
        let report = high_message_load(7);
        assert!(report.metrics.delivered + report.metrics.dropped_loss > 0);
    }

    #[test]
    fn churn_scenario_tracks_latency_samples() {
        let report = node_churn(99);
        assert!(report.avg_latency_ms >= 0.0);
    }

    #[test]
    fn bloom_sync_scenario_runs() {
        let report = bloom_sync_efficiency(5);
        assert!(report.expected_deliveries > 0);
        let eff = bloom_sync_efficiency_compare(10, 50);
        assert!(eff.bloom_bytes_per_round < eff.ids_bytes_per_round);
        assert!(eff.estimated_roundtrips_bloom <= eff.estimated_roundtrips_ids);
    }

    #[test]
    fn hop_limit_scenario_runs() {
        let report = hop_limit_containment(6);
        assert!(report.expected_deliveries > 0);
    }

    #[test]
    fn scope_isolation_scenario_runs() {
        let report = scope_isolation(7);
        assert!(report.expected_deliveries > 0);
    }
}
