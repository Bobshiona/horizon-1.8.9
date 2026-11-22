//! The hash for the world's coordinate-keyed maps. Meshing looks up a column for
//! every neighbour of every block, and std's SipHash spent half of a mesh
//! worker's time there. Coordinates are a few integers, so a multiply-and-mix hash
//! does; a random key per world keeps a server from choosing colliding positions.

use std::hash::{BuildHasher, Hasher};

#[derive(Clone, Debug)]
pub struct Keys {
    key: u64,
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            key: std::collections::hash_map::RandomState::new().hash_one(0x6b65_7973u64) | 1,
        }
    }
}

impl BuildHasher for Keys {
    type Hasher = KeyHasher;

    fn build_hasher(&self) -> KeyHasher {
        KeyHasher { state: self.key }
    }
}

pub struct KeyHasher {
    state: u64,
}

const MULTIPLIER: u64 = 0x9e37_79b9_7f4a_7c15;

impl KeyHasher {
    #[inline]
    fn add(&mut self, value: u64) {
        self.state = (self.state ^ value)
            .wrapping_mul(MULTIPLIER)
            .rotate_left(29);
    }
}

impl Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().expect("8 bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut last = [0u8; 8];
            last[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(last) ^ ((rest.len() as u64) << 59));
        }
    }

    #[inline]
    fn write_i32(&mut self, value: i32) {
        self.add(value as u32 as u64);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        // splitmix64's finaliser: every input bit reaches the low bits the table uses.
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn nearby_columns_spread_over_the_table_and_keys_differ_per_world() {
        let keys = Keys::default();
        // 33 x 33 columns around the origin: a render distance of 16.
        let mut buckets = HashSet::new();
        let mut hashes = HashSet::new();
        for x in -16..=16i32 {
            for z in -16..=16i32 {
                let hash = keys.hash_one((x, z));
                hashes.insert(hash);
                buckets.insert(hash & 2047);
            }
        }
        assert_eq!(hashes.len(), 33 * 33);
        // 1089 keys in 2048 buckets: a uniform hash fills about 860.
        assert!(buckets.len() > 780, "{} buckets", buckets.len());
        assert_ne!(keys.hash_one((1, 2)), keys.hash_one((2, 1)));
        assert_ne!(keys.hash_one((1, 2)), Keys::default().hash_one((1, 2)));
        assert_ne!(keys.hash_one([1, 2, 3]), keys.hash_one([1, 2, 4]));
    }
}
