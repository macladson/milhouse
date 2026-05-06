use crate::{Arc, UpdateMap};
#[cfg(feature = "arbitrary")]
use arbitrary::Arbitrary;
use std::collections::BTreeMap;
use tree_hash::{Hash256, TreeHash, TreeHashType};

/// Default depth of the internal Merkle subtree computed within each packed leaf.
///
/// This controls the default maximum "fatness" of packed leaves so that each leaf holds up to
/// `T::tree_hash_packing_factor() << DEFAULT_SUBTREE_DEPTH` values and computes a
/// multi-level internal Merkle tree.
///
/// The effective subtree depth for a given list is `min(MAX_SUBTREE_DEPTH, chunk_tree_depth)`
/// where `chunk_tree_depth = int_log(N) - int_log(T::tree_hash_packing_factor())`.
/// This ensures the fat leaf never exceeds the total tree depth required by the SSZ spec.
///
/// | Value | Chunks/leaf |
/// |-------|-------------|
/// | 0     | 1           |
/// | 1     | 2           |
/// | 2     | 4           |
/// | 3     | 8           |
/// | 10    | 1024        |
///
/// Higher values increase throughput for full-tree recomputation at the cost
/// of coarser hash invalidation granularity (more values share a single
/// cached hash).
pub const DEFAULT_SUBTREE_DEPTH: usize = 2;

/// Type to abstract over whether `T` is wrapped in an `Arc` or not.
#[derive(Debug)]
pub enum MaybeArced<T> {
    Arced(Arc<T>),
    Unarced(T),
}

impl<T> MaybeArced<T> {
    pub fn arced(self) -> Arc<T> {
        match self {
            Self::Arced(arc) => arc,
            Self::Unarced(value) => Arc::new(value),
        }
    }
}

/// Length type, to avoid confusion with depth and other `usize` parameters.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct Length(pub usize);

impl Length {
    #[allow(clippy::should_implement_trait)]
    pub fn as_mut(&mut self) -> &mut usize {
        &mut self.0
    }

    #[inline(always)]
    pub fn as_usize(&self) -> usize {
        self.0
    }
}

/// Compute ceil(log(n))
///
/// Smallest number of bits d so that n <= 2^d
pub fn int_log(n: usize) -> usize {
    match n.checked_next_power_of_two() {
        Some(x) => x.trailing_zeros() as usize,
        None => 8 * std::mem::size_of::<usize>(),
    }
}

/// Compute the depth of the largest subtree which has the `index`th element as its 0th leaf.
///
/// A level is fundamentally the same as a depth, it is a value `0..=depth` such that a subtree
/// at that level (depth) contains up to 2^level elements at the leaves. Level 0 is the level of
/// leaves and packed leaves.
pub fn compute_level(index: usize, depth: usize, packing_depth: usize) -> usize {
    let raw_level = if index == 0 {
        depth + packing_depth
    } else {
        index.trailing_zeros() as usize
    };
    if raw_level < packing_depth {
        0
    } else {
        raw_level
    }
}

/// Compute the effective subtree depth for a list/vector of capacity `N`.
///
/// This is `min(max_subtree_depth, available_chunk_depth)` where `available_chunk_depth`
/// is the number of tree levels that exist above the base-packed chunks for a tree
/// of capacity N.
///
/// `log_n` should be `int_log(N)` where N is the list/vector capacity.
#[inline]
pub fn effective_subtree_depth<T: TreeHash>(log_n: usize, max_subtree_depth: usize) -> usize {
    match T::tree_hash_type() {
        TreeHashType::Basic => {
            let base_packing_depth = int_log(T::tree_hash_packing_factor());
            let available = log_n.saturating_sub(base_packing_depth);
            max_subtree_depth.min(available)
        }
        _ => 0,
    }
}

/// Compute the effective packing factor for a list/vector of capacity N.
///
/// This is `T::tree_hash_packing_factor() << effective_subtree_depth(log_n)`, i.e.
/// the number of `T` values that fit in one fat packed leaf.
///
/// Returns `None` for non-basic (container/list/vector) types.
pub fn opt_packing_factor<T: TreeHash>(log_n: usize, max_subtree_depth: usize) -> Option<usize> {
    match T::tree_hash_type() {
        TreeHashType::Basic => {
            let eff = effective_subtree_depth::<T>(log_n, max_subtree_depth);
            Some(T::tree_hash_packing_factor() << eff)
        }
        TreeHashType::Container | TreeHashType::List | TreeHashType::Vector => None,
    }
}

/// Compute the effective packing depth for a list/vector of capacity N.
///
/// This accounts for both the natural packing depth of the type (how many values
/// fit in one 32-byte chunk) and the effective subtree depth for this capacity.
///
/// Returns `None` for non-basic types.
pub fn opt_packing_depth<T: TreeHash>(log_n: usize, max_subtree_depth: usize) -> Option<usize> {
    match T::tree_hash_type() {
        TreeHashType::Basic => {
            let base = int_log(T::tree_hash_packing_factor());
            let eff = effective_subtree_depth::<T>(log_n, max_subtree_depth);
            Some(base + eff)
        }
        TreeHashType::Container | TreeHashType::List | TreeHashType::Vector => None,
    }
}

/// Compute the maximum index of a BTreeMap.
pub fn max_btree_index<T>(map: &BTreeMap<usize, T>) -> Option<usize> {
    map.keys().next_back().copied()
}

/// Compute the length a data structure will have after applying `updates`.
pub fn updated_length<U: UpdateMap<T>, T>(prev_len: Length, updates: &U) -> Length {
    updates.max_index().map_or(prev_len, |max_idx| {
        Length(std::cmp::max(max_idx + 1, prev_len.as_usize()))
    })
}

/// Get the hash of a node at `(depth, prefix)` from an optional HashMap.
pub fn opt_hash(
    hashes: Option<&BTreeMap<(usize, usize), Hash256>>,
    depth: usize,
    prefix: usize,
) -> Option<Hash256> {
    hashes?.get(&(depth, prefix)).copied()
}

#[cfg(feature = "arbitrary")]
pub fn arb_arc<'a, T: Arbitrary<'a>>(
    u: &mut arbitrary::Unstructured<'a>,
) -> arbitrary::Result<Arc<T>> {
    T::arbitrary(u).map(Arc::new)
}

#[cfg(feature = "arbitrary")]
pub fn arb_rwlock<'a, T: Arbitrary<'a>>(
    u: &mut arbitrary::Unstructured<'a>,
) -> arbitrary::Result<parking_lot::RwLock<T>> {
    T::arbitrary(u).map(parking_lot::RwLock::new)
}

#[cfg(test)]
mod test {
    use super::*;

    /// The level of an odd index is always 0.
    #[test]
    fn odd_index_level() {
        let depth = 5;
        let packing_depth = 0;
        for i in (0..2usize.pow(depth as u32)).filter(|i| i % 2 == 1) {
            assert_eq!(compute_level(i, depth, packing_depth), 0);
        }
    }

    /// The level of indices below the packing depth is 0.
    #[test]
    fn packing_depth_level() {
        let depth = 10;
        let packing_depth = 3;
        assert_eq!(
            compute_level(0, depth, packing_depth),
            depth + packing_depth
        );
        assert_eq!(compute_level(1, depth, packing_depth), 0);
        assert_eq!(compute_level(2, depth, packing_depth), 0);
        assert_eq!(compute_level(4, depth, packing_depth), 0);
        assert_eq!(compute_level(8, depth, packing_depth), 3);
    }

    #[test]
    fn effective_subtree_depth_u64() {
        // u64: base packing factor = 4, base packing depth = 2
        assert_eq!(
            effective_subtree_depth::<u64>(int_log(16), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(2)
        );
        assert_eq!(
            effective_subtree_depth::<u64>(int_log(32), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(3)
        );
        assert_eq!(
            effective_subtree_depth::<u64>(int_log(1024), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(8)
        );
        assert_eq!(
            effective_subtree_depth::<u64>(int_log(1048576), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(18)
        );
    }

    #[test]
    fn effective_subtree_depth_u8() {
        // u8: base packing factor = 32, base packing depth = 5
        assert_eq!(
            effective_subtree_depth::<u8>(int_log(32), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(0)
        );
        assert_eq!(
            effective_subtree_depth::<u8>(int_log(256), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(3)
        );
        assert_eq!(
            effective_subtree_depth::<u8>(int_log(1024), DEFAULT_SUBTREE_DEPTH),
            DEFAULT_SUBTREE_DEPTH.min(5)
        );
    }
}
