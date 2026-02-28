use std::hash::Hasher;

/// A deterministic FNV-1a 64-bit hasher.
/// `DefaultHasher` from `std::collections` uses a random seed per process,
/// which breaks deterministic compile-time hashing across different compiler runs.
pub struct DeterministicHasher {
    hash: u64,
}

impl DeterministicHasher {
    pub fn new() -> Self {
        Self {
            hash: 0xcbf29ce484222325, // FNV-1a 64-bit offset basis
        }
    }
}

impl Default for DeterministicHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for DeterministicHasher {
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, bytes: &[u8]) {
        let prime: u64 = 0x100000001b3; // FNV-1a 64-bit prime
        for &byte in bytes {
            self.hash ^= byte as u64;
            self.hash = self.hash.wrapping_mul(prime);
        }
    }
}
