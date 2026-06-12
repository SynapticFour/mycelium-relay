// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use super::presence::PresenceSignal;

pub struct ProximityMatcher {
    pub my_looking_for: Option<String>,
    pub my_interests: Vec<String>,
}

impl ProximityMatcher {
    pub fn score(&self, other: &PresenceSignal) -> MatchScore {
        let mut score = 0u32;

        let common_interests: Vec<String> = self
            .my_interests
            .iter()
            .filter(|i| other.profile.interests.contains(i))
            .cloned()
            .collect();
        score += common_interests.len() as u32 * 10;

        if other.profile.display_name.is_some() {
            score += 5;
        }
        if other.profile.bio.is_some() {
            score += 5;
        }
        if other.profile.photo_base64.is_some() {
            score += 10;
        }

        MatchScore {
            score,
            common_interests,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchScore {
    pub score: u32,
    pub common_interests: Vec<String>,
}
