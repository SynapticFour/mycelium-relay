// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
mod matcher;
mod matches;
mod presence;
mod store;

pub use matcher::{MatchScore, ProximityMatcher};
pub use matches::{
    ProximityInbox, ProximityMatchState, ProximityNearbyEntry, ProximityReceivedMessage,
};
pub use presence::{
    PresenceProfile, PresenceSignal, ProximityDirectMessage, ProximityMatchIntent, PROXIMITY_SCOPE,
};
pub use store::ProximityStore;

#[cfg(test)]
mod tests;
