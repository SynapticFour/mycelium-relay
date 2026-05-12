use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDigest {
    pub bloom_filter: Vec<u8>,
    pub object_count: u64,
}

#[derive(Debug, Clone)]
pub struct BloomFilter {
    bits: Vec<u64>,
    k: u8,
}

impl BloomFilter {
    pub fn new() -> Self {
        Self {
            bits: vec![0u64; 128],
            k: 3,
        }
    }

    pub fn insert(&mut self, id: &str) {
        for i in 0..self.k {
            let bit = self.hash(id, i as u64) % 8192;
            self.bits[(bit / 64) as usize] |= 1u64 << (bit % 64);
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        for i in 0..self.k {
            let bit = self.hash(id, i as u64) % 8192;
            if self.bits[(bit / 64) as usize] & (1u64 << (bit % 64)) == 0 {
                return false;
            }
        }
        true
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bits.iter().flat_map(|b| b.to_le_bytes()).collect()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 1024 {
            return None;
        }
        let mut bits = Vec::with_capacity(128);
        for chunk in bytes.chunks(8) {
            let arr: [u8; 8] = chunk.try_into().ok()?;
            bits.push(u64::from_le_bytes(arr));
        }
        Some(Self { bits, k: 3 })
    }

    fn hash(&self, id: &str, seed: u64) -> u64 {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        id.hash(&mut h);
        h.finish()
    }
}

impl Default for BloomFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for BloomFilter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(&self.to_bytes())
    }
}

impl<'de> Deserialize<'de> for BloomFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        BloomFilter::from_bytes(&bytes).ok_or_else(|| serde::de::Error::custom("invalid bloom bytes"))
    }
}
