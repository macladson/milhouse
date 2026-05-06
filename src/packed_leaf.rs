use crate::{Error, UpdateMap};
use educe::Educe;
use ethereum_hashing::hash32_concat;
use parking_lot::RwLock;
use std::ops::ControlFlow;
use tree_hash::{BYTES_PER_CHUNK, Hash256, TreeHash};

#[derive(Debug, Educe)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[educe(PartialEq, Hash)]
pub struct PackedLeaf<T: TreeHash + Clone> {
    #[educe(PartialEq(ignore), Hash(ignore))]
    #[cfg_attr(feature = "arbitrary", arbitrary(with = crate::utils::arb_rwlock))]
    pub hash: RwLock<Hash256>,
    pub values: Vec<T>,
}

impl<T> Clone for PackedLeaf<T>
where
    T: TreeHash + Clone,
{
    fn clone(&self) -> Self {
        Self {
            hash: RwLock::new(*self.hash.read()),
            values: self.values.clone(),
        }
    }
}

impl<T: TreeHash + Clone> PackedLeaf<T> {
    #[inline(always)]
    fn capacity_for_subtree_depth(subtree_depth: usize) -> usize {
        T::tree_hash_packing_factor() << subtree_depth
    }

    /// Hash adjacent pairs of Hash256 values in-place, reducing count by half.
    /// Returns the new count (count / 2).
    fn hash_layer_inplace(buf: &mut [Hash256], count: usize) -> usize {
        debug_assert!(count.is_power_of_two(), "count must be a power of two");
        let pairs = count / 2;
        for i in 0..pairs {
            let hash = hash32_concat(buf[2 * i].as_slice(), buf[2 * i + 1].as_slice());
            buf[i] = Hash256::from(hash);
        }
        pairs
    }

    pub fn tree_hash(&self, subtree_depth: usize) -> Hash256 {
        let read_lock = self.hash.read();
        let existing = *read_lock;
        drop(read_lock);

        if !existing.is_zero() {
            return existing;
        }

        let hash = if subtree_depth == 0 {
            // Original behavior: pack all values into a single Hash256 chunk, no hashing.
            let mut chunk = Hash256::ZERO;
            let chunk_bytes = chunk.as_mut_slice();

            let value_len = BYTES_PER_CHUNK / T::tree_hash_packing_factor();
            for (i, value) in self.values.iter().enumerate() {
                chunk_bytes[i * value_len..(i + 1) * value_len]
                    .copy_from_slice(&value.tree_hash_packed_encoding());
            }
            chunk
        } else {
            // Fat packed leaf: pack values into multiple chunks then reduce via merkle hashing.
            let num_chunks = 1usize << subtree_depth;
            let chunk_packing_factor = T::tree_hash_packing_factor();
            let value_len = BYTES_PER_CHUNK / chunk_packing_factor;

            // Use a stack buffer for small depths, heap for larger ones.
            let mut heap_chunks;
            let mut stack_chunks = [Hash256::ZERO; 16];
            let chunks: &mut [Hash256] = if num_chunks <= 16 {
                &mut stack_chunks[..num_chunks]
            } else {
                heap_chunks = vec![Hash256::ZERO; num_chunks];
                &mut heap_chunks[..]
            };

            // Pack values into chunks.
            for (i, value) in self.values.iter().enumerate() {
                let chunk_idx = i / chunk_packing_factor;
                let pos_in_chunk = i % chunk_packing_factor;
                let encoding = value.tree_hash_packed_encoding();
                chunks[chunk_idx].as_mut_slice()
                    [pos_in_chunk * value_len..(pos_in_chunk + 1) * value_len]
                    .copy_from_slice(&encoding);
            }

            // Reduce via hash_layer_inplace.
            let mut count = num_chunks;
            while count > 1 {
                count = Self::hash_layer_inplace(chunks, count);
            }

            chunks[0]
        };

        *self.hash.write() = hash;
        hash
    }

    pub fn empty(subtree_depth: usize) -> Self {
        let capacity = Self::capacity_for_subtree_depth(subtree_depth);
        PackedLeaf {
            hash: RwLock::new(Hash256::ZERO),
            values: Vec::with_capacity(capacity),
        }
    }

    pub fn single(value: T, subtree_depth: usize) -> Self {
        let capacity = Self::capacity_for_subtree_depth(subtree_depth);
        let mut values = Vec::with_capacity(capacity);
        values.push(value);

        PackedLeaf {
            hash: RwLock::new(Hash256::ZERO),
            values,
        }
    }

    pub fn repeat(value: T, n: usize, subtree_depth: usize) -> Self {
        let capacity = Self::capacity_for_subtree_depth(subtree_depth);
        assert!(n <= capacity);
        let mut values = Vec::with_capacity(capacity);
        values.resize(n, value);
        PackedLeaf {
            hash: RwLock::new(Hash256::ZERO),
            values,
        }
    }

    pub fn insert_at_index(
        &self,
        index: usize,
        value: T,
        subtree_depth: usize,
    ) -> Result<Self, Error> {
        let capacity = Self::capacity_for_subtree_depth(subtree_depth);
        let mut updated = PackedLeaf {
            hash: RwLock::new(Hash256::ZERO),
            values: self.values.clone(),
        };
        let sub_index = index % capacity;
        updated.insert_mut(sub_index, value)?;
        Ok(updated)
    }

    pub fn update<U: UpdateMap<T>>(
        &self,
        prefix: usize,
        hash: Hash256,
        updates: &U,
        subtree_depth: usize,
    ) -> Result<Self, Error> {
        let capacity = Self::capacity_for_subtree_depth(subtree_depth);
        let mut updated = PackedLeaf {
            hash: RwLock::new(hash),
            values: self.values.clone(),
        };

        let start = prefix;
        let end = prefix + capacity;
        updates.for_each_range(start, end, |index, value| {
            ControlFlow::Continue(updated.insert_mut(index % capacity, value.clone()))
        })?;
        Ok(updated)
    }

    pub fn insert_mut(&mut self, sub_index: usize, value: T) -> Result<(), Error> {
        // Ensure hash is 0.
        *self.hash.get_mut() = Hash256::ZERO;

        if sub_index == self.values.len() {
            self.values.push(value);
        } else if sub_index < self.values.len() {
            self.values[sub_index] = value;
        } else {
            return Err(Error::PackedLeafOutOfBounds {
                sub_index,
                len: self.values.len(),
            });
        }
        Ok(())
    }

    pub fn push(&mut self, value: T, subtree_depth: usize) -> Result<(), Error> {
        let capacity = Self::capacity_for_subtree_depth(subtree_depth);
        if self.values.len() == capacity {
            return Err(Error::PackedLeafFull {
                len: self.values.len(),
            });
        }
        self.values.push(value);
        Ok(())
    }
}
