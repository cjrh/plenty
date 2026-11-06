//! The linear-probe index of a hashed dictionary or set (issue #761).
//!
//! A bucket is one `u64`, zero when empty. With `shift` = log2 of the table
//! length, its low `shift` bits hold the entry's stable slot plus one and the
//! bits above hold the low `64 - shift` bits of the key's hash. Slots stay
//! below half the table length (`Collection::try_reserve` keeps the load at
//! most one half), so the slot field never overflows and the encoding costs no
//! memory beyond the index alone.
//!
//! The stored bits serve three purposes:
//! - a probe compares keys only when the stored bits match its hash, so a
//!   collision costs one integer comparison rather than a key read;
//! - growth to a table of length `2^b` rebuilds from stored bits while
//!   `b <= 64 - shift`, that is for tables up to `2^32` buckets, without
//!   reading or hashing any key;
//! - removal finds a displaced bucket's home from its stored bits while
//!   `shift <= 32`.
//!
//! Larger tables fall back to hashing keys, the only cost before this
//! encoding.
use crate::memory::{self, AllocError};

/// Whether a bucket of a table with `shift` holds the `home_bits` low hash
/// bits that a table of `2^home_bits` buckets needs to place it.
fn covers(shift: u32, home_bits: u32) -> bool {
    home_bits <= stored_bits(shift)
}

#[cfg(not(test))]
fn stored_bits(shift: u32) -> u32 {
    64 - shift
}

/// Tests cap the usable stored bits so small tables take the fallbacks that
/// otherwise run only above `2^32` buckets.
#[cfg(test)]
fn stored_bits(shift: u32) -> u32 {
    (64 - shift).min(STORED_BITS_LIMIT.with(std::cell::Cell::get))
}

#[cfg(test)]
thread_local! {
    static STORED_BITS_LIMIT: std::cell::Cell<u32> = const { std::cell::Cell::new(u32::MAX) };
}

/// Cap the stored hash bits that growth and removal may use on this thread
/// until the guard drops.
#[cfg(test)]
pub(super) fn limit_stored_bits(bits: u32) -> impl Drop {
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            STORED_BITS_LIMIT.with(|limit| limit.set(self.0));
        }
    }
    Restore(STORED_BITS_LIMIT.with(|limit| limit.replace(bits)))
}

pub(super) struct Buckets {
    slots: Vec<u64>,
}

impl Buckets {
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }
    /// An empty table of `len` buckets, a power of two.
    pub fn try_with_len(len: usize) -> Result<Self, AllocError> {
        debug_assert!(len.is_power_of_two());
        let mut slots = Vec::new();
        memory::try_reserve(&mut slots, len)?;
        slots.resize(len, 0);
        Ok(Self { slots })
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    #[cfg(test)]
    pub fn capacity_bytes(&self) -> usize {
        self.slots.capacity() * std::mem::size_of::<u64>()
    }
    #[cfg(test)]
    pub fn occupied(&self) -> usize {
        self.slots.iter().filter(|slot| **slot != 0).count()
    }
    /// Empty every bucket, keeping the allocation.
    pub fn clear(&mut self) {
        self.slots.fill(0);
    }
    fn mask(&self) -> usize {
        self.slots.len() - 1
    }
    fn shift(&self) -> u32 {
        self.slots.len().trailing_zeros()
    }
    /// The entry slot referenced by `bucket`, or `None` if it is empty.
    #[inline]
    pub fn entry(&self, bucket: usize) -> Option<usize> {
        (self.slots[bucket] as usize & self.mask()).checked_sub(1)
    }
    /// The bucket whose stored hash matches `hash` and whose entry satisfies
    /// `same`, or the empty bucket that ends the probe. The table must be
    /// non-empty.
    #[inline]
    pub fn probe(&self, hash: u64, mut same: impl FnMut(usize) -> bool) -> usize {
        let mask = self.mask();
        let tag = hash << self.shift();
        let mut bucket = hash as usize & mask;
        loop {
            let slot = self.slots[bucket];
            if slot == 0 || slot & !(mask as u64) == tag && same((slot as usize & mask) - 1) {
                return bucket;
            }
            bucket = (bucket + 1) & mask;
        }
    }
    /// The empty bucket ending `hash`'s probe, for a key known to be absent.
    pub fn vacant(&self, hash: u64) -> usize {
        let mask = self.mask();
        let mut bucket = hash as usize & mask;
        while self.slots[bucket] != 0 {
            bucket = (bucket + 1) & mask;
        }
        bucket
    }
    /// Point the empty `bucket` at entry slot `entry`, whose key hashes to `hash`.
    #[inline]
    pub fn set(&mut self, bucket: usize, hash: u64, entry: usize) {
        debug_assert!(self.slots[bucket] == 0 && entry < self.mask());
        self.slots[bucket] = hash << self.shift() | (entry as u64 + 1);
    }
    /// Add a reference to entry slot `entry`, whose key is absent.
    pub fn insert_unique(&mut self, hash: u64, entry: usize) {
        let bucket = self.vacant(hash);
        self.set(bucket, hash, entry);
    }
    /// Rebuild these references into the empty `target` from stored hash
    /// bits alone. Returns false, leaving `target` empty, when they hold
    /// fewer bits than `target`'s home buckets need. `target` must be at
    /// least as long: a shorter one would need hash bits a bucket does not
    /// store, giving wrong tags and silent lookup misses.
    pub fn rehash_into(&self, target: &mut Self) -> bool {
        if self.is_empty() {
            return true;
        }
        debug_assert!(target.shift() >= self.shift(), "tables only grow");
        if !covers(self.shift(), target.shift()) {
            return false;
        }
        let (mask, shift) = (self.mask(), self.shift());
        let (target_mask, target_shift) = (target.mask(), target.shift());
        for &slot in &self.slots {
            if slot != 0 {
                let stored = slot >> shift;
                let mut bucket = stored as usize & target_mask;
                while target.slots[bucket] != 0 {
                    bucket = (bucket + 1) & target_mask;
                }
                target.slots[bucket] = stored << target_shift | (slot & mask as u64);
            }
        }
        true
    }
    /// Empty `hole` and repair its probe cluster by backward shifting.
    /// `hash_of` rehashes an entry's key, needed only when stored bits do not
    /// cover the home bucket (tables over `2^32` buckets).
    pub fn remove(&mut self, mut hole: usize, mut hash_of: impl FnMut(usize) -> u64) {
        let (mask, shift) = (self.mask(), self.shift());
        let stored_home = covers(shift, shift);
        let mut probe = (hole + 1) & mask;
        while self.slots[probe] != 0 {
            let slot = self.slots[probe];
            let home = if stored_home {
                (slot >> shift) as usize & mask
            } else {
                hash_of((slot as usize & mask) - 1) as usize & mask
            };
            // Move only references whose circular probe path crosses the
            // hole. An empty bucket terminates even across wraparound.
            if (hole.wrapping_sub(home) & mask) < (probe.wrapping_sub(home) & mask) {
                self.slots[hole] = slot;
                hole = probe;
            }
            probe = (probe + 1) & mask;
        }
        self.slots[hole] = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::Buckets;

    #[test]
    fn probe_skips_mismatched_stored_hashes() {
        let mut table = Buckets::try_with_len(16).unwrap();
        // Same home bucket, different stored bits: only the second matches.
        table.insert_unique(0x10_0003, 0);
        table.insert_unique(0x20_0003, 1);
        let mut compared = Vec::new();
        let bucket = table.probe(0x20_0003, |entry| {
            compared.push(entry);
            true
        });
        assert_eq!(table.entry(bucket), Some(1));
        assert_eq!(compared, [1]);
        // A full-hash collision still defers to the key comparison.
        let bucket = table.probe(0x20_0003, |_| false);
        assert_eq!(table.entry(bucket), None);
    }

    #[test]
    fn rehash_preserves_references_and_stops_at_missing_bits() {
        let mut table = Buckets::try_with_len(1 << 4).unwrap();
        let hashes: Vec<u64> = (0..8u64)
            .map(|i| i.wrapping_mul(0x9e37_79b9_7f4a_7c15))
            .collect();
        for (entry, &hash) in hashes.iter().enumerate() {
            table.insert_unique(hash, entry);
        }
        let mut grown = Buckets::try_with_len(1 << 10).unwrap();
        assert!(table.rehash_into(&mut grown));
        for (entry, &hash) in hashes.iter().enumerate() {
            let bucket = grown.probe(hash, |found| found == entry);
            assert_eq!(grown.entry(bucket), Some(entry));
        }
    }

    #[test]
    fn stored_bits_cover_tables_up_to_2_pow_32() {
        // Checked without allocating the tables: growth from 2^31 to 2^32
        // buckets and removal at 2^32 use stored bits; beyond, keys rehash.
        assert!(super::covers(31, 32));
        assert!(super::covers(32, 32));
        assert!(!super::covers(32, 33));
        assert!(!super::covers(33, 33));
    }
}
