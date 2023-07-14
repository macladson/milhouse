use crate::{Error, UpdateMap, Value};
use arbitrary::Arbitrary;
use core::marker::PhantomData;
use derivative::Derivative;
use std::ops::ControlFlow;
use tree_hash::{Hash256, BYTES_PER_CHUNK};

#[derive(Debug, Derivative, Arbitrary)]
#[derivative(PartialEq, Hash)]
pub struct PackedLeaf<T: Value> {
    pub hash: Hash256,
    pub length: u8,
    _phantom: PhantomData<T>,
}

impl<T> Clone for PackedLeaf<T>
where
    T: Value,
{
    fn clone(&self) -> Self {
        Self {
            hash: self.hash,
            length: self.length,
            _phantom: PhantomData,
        }
    }
}

impl<T: Value> PackedLeaf<T> {
    pub fn length(&self) -> usize {
        self.length as usize
    }

    fn value_len() -> usize {
        BYTES_PER_CHUNK / T::tree_hash_packing_factor()
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.length() {
            return None;
        }
        let hash_base_ptr: *const Hash256 = &self.hash;
        let base_ptr: *const T = hash_base_ptr as *const T;
        let elem_ptr: *const T = unsafe { base_ptr.add(index) };
        Some(unsafe { &*elem_ptr })
    }

    pub fn tree_hash(&self) -> Hash256 {
        self.hash
    }

    pub fn empty() -> Self {
        PackedLeaf {
            hash: Hash256::zero(),
            length: 0,
            _phantom: PhantomData,
        }
    }

    pub fn single(value: T) -> Self {
        let mut hash = Hash256::zero();
        let hash_bytes = hash.as_bytes_mut();

        let value_len = Self::value_len();
        hash_bytes[0..value_len].copy_from_slice(&value.as_ssz_bytes());

        PackedLeaf {
            hash,
            length: 1,
            _phantom: PhantomData,
        }
    }

    pub fn repeat(value: T, n: usize) -> Self {
        assert!(n <= T::tree_hash_packing_factor());

        let mut hash = Hash256::zero();
        let hash_bytes = hash.as_bytes_mut();

        let value_len = Self::value_len();

        for (i, value) in vec![value; n].iter().enumerate() {
            hash_bytes[i * value_len..(i + 1) * value_len].copy_from_slice(&value.as_ssz_bytes());
        }

        PackedLeaf {
            hash,
            length: n as u8,
            _phantom: PhantomData,
        }
    }

    pub fn insert_at_index(&self, index: usize, value: T) -> Result<Self, Error> {
        let mut updated = self.clone();

        updated.insert_mut(index, value)?;

        Ok(updated)
    }

    // FIXME: remove _hash/work out what's going on
    pub fn update<U: UpdateMap<T>>(
        &self,
        prefix: usize,
        _hash: Hash256,
        updates: &U,
    ) -> Result<Self, Error> {
        let packing_factor = T::tree_hash_packing_factor();
        let start = prefix;
        let end = prefix + packing_factor;

        let mut updated = self.clone();

        updates.for_each_range(start, end, |index, value| {
            ControlFlow::Continue(updated.insert_mut(index % packing_factor, value.clone()))
        })?;

        Ok(updated)
    }

    pub fn insert_mut(&mut self, index: usize, value: T) -> Result<(), Error> {
        // Convert the index to the index of the underlying bytes.
        let sub_index = index * Self::value_len();

        if sub_index >= BYTES_PER_CHUNK {
            return Err(Error::PackedLeafOutOfBounds {
                sub_index,
                len: self.length(),
            });
        }

        let value_len = Self::value_len();

        let mut hash = self.hash;
        let hash_bytes = hash.as_bytes_mut();

        hash_bytes[sub_index..sub_index + value_len].copy_from_slice(&value.as_ssz_bytes());

        self.hash = hash;

        if index == self.length() {
            self.length += 1;
        } else if index > self.length() {
            panic!("This is bad");
        }

        Ok(())
    }

    pub fn push(&mut self, value: T) -> Result<(), Error> {
        // Ensure a new T will not overflow the leaf.
        if self.length() >= T::tree_hash_packing_factor() {
            return Err(Error::PackedLeafFull { len: self.length() });
        }

        self.insert_mut(self.length(), value)?;

        Ok(())
    }
}
