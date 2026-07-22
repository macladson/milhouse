//! Tests for `intra_rebase` on all-zero contents.
//!
//! Deduplication is by hash, so the rebased tree may not be structurally equal (`PartialEq`)
//! to the original, but its contents and tree hash must be unchanged.

use crate::{List, Vector};
use tree_hash::TreeHash;
use typenum::{U33, U1024};

#[test]
fn intra_rebase_all_zero_list_semantic_equality() {
    for &n in &[5usize, 21, 33, 85, 100] {
        let values = vec![0u64; n];
        let mut list = List::<u64, U1024>::new(values.clone()).unwrap();
        let root = list.tree_hash_root();
        list.intra_rebase().unwrap();
        assert_eq!(list.tree_hash_root(), root, "n={n}");
        assert_eq!(list.to_vec(), values, "n={n}");
    }
}

#[test]
fn intra_rebase_all_zero_vector_semantic_equality() {
    // A non-power-of-two length gives the tree a zero-padded boundary node.
    let values = vec![0u64; 33];
    let mut vect = Vector::<u64, U33>::new(values.clone()).unwrap();
    let root = vect.tree_hash_root();
    vect.intra_rebase().unwrap();
    assert_eq!(vect.tree_hash_root(), root);
    assert_eq!(vect.to_vec(), values);
}
