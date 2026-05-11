use crate::tests::proptest::Large;
use crate::update_map::MaxMap;
use crate::utils::{DEFAULT_PACKED_LEAF_INTERNAL_DEPTH, compute_level, int_log, opt_packing_depth};
use crate::{Arc, List, level_iter::LevelNode};
use tree_hash::{Hash256, TreeHash};
use typenum::{U8, U32, U1024};
use vec_map::VecMap;

#[test]
fn level_iter_pop_front_basic_packed() {
    let vec = vec![10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    let list = List::<u64, U32>::new(vec.clone()).unwrap();
    assert_eq!(list.len(), 11);

    let packing_depth =
        opt_packing_depth::<u64>(int_log(32), DEFAULT_PACKED_LEAF_INTERNAL_DEPTH).unwrap_or(0);

    for from in 4..vec.len() {
        let mut list = list.clone();
        for (i, level) in list.level_iter_from(from).unwrap().enumerate() {
            match level {
                LevelNode::PackedLeaf(leaf) => {
                    assert!(
                        (from.trailing_zeros() as usize) < packing_depth,
                        "from = {from}"
                    );
                    assert_eq!(*leaf, vec[i + from]);
                }
                LevelNode::Internal(node) => {
                    let depth = List::<u64, U32>::depth();
                    let level = compute_level(from, depth, packing_depth);
                    assert!(level >= packing_depth);
                    assert!(
                        node.compute_len() <= 1 << level,
                        "from={from}, level={level}, compute_len={}",
                        node.compute_len()
                    );
                }
            }
        }

        list.pop_front(from).unwrap();
        assert_eq!(list.len(), vec.len() - from);
        assert_eq!(list.to_vec().as_slice(), &vec[from..]);
    }
}

#[test]
fn level_iter_pop_front_basic_packed_17() {
    let vec = vec![
        10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 0, 0, 0, 0, 0, 666,
    ];
    let list = List::<u64, U32>::new(vec.clone()).unwrap();
    assert_eq!(list.len(), 17);

    let packing_depth =
        opt_packing_depth::<u64>(int_log(32), DEFAULT_PACKED_LEAF_INTERNAL_DEPTH).unwrap_or(0);

    for from in 0..vec.len() {
        let mut list = list.clone();
        for (i, level) in list.level_iter_from(from).unwrap().enumerate() {
            match level {
                LevelNode::PackedLeaf(leaf) => {
                    assert!(
                        (from.trailing_zeros() as usize) < packing_depth,
                        "from = {from}"
                    );
                    assert_eq!(*leaf, vec[i + from]);
                }
                LevelNode::Internal(node) => {
                    let depth = List::<u64, U32>::depth();
                    let level = compute_level(from, depth, packing_depth);
                    assert!(level >= packing_depth);
                    assert!(
                        node.compute_len() <= 1 << level,
                        "from={from}, level={level}, compute_len={}",
                        node.compute_len()
                    );
                }
            }
        }

        list.pop_front(from).unwrap();
        assert_eq!(list.len(), vec.len() - from);
        assert_eq!(list.to_vec().as_slice(), &vec[from..]);
    }
}

#[test]
fn level_iter_pop_front_basic_large() {
    let vec = (0..7u8)
        .map(|i| Large {
            a: i as u64,
            b: i,
            c: Hash256::repeat_byte(i),
            d: List::empty(),
        })
        .collect::<Vec<_>>();
    let list = List::<Large, U8>::new(vec.clone()).unwrap();
    assert_eq!(list.len(), 7);

    for from in 0..vec.len() {
        let mut list = list.clone();
        list.pop_front(from).unwrap();
        assert_eq!(list.len(), vec.len() - from);
        assert_eq!(list.to_vec().as_slice(), &vec[from..]);
    }
}

#[test]
fn pop_front_zero_noop() {
    let vec = vec![10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    let mut list = List::<u64, U32>::new(vec.clone()).unwrap();

    list.apply_updates().unwrap();
    let mut popped_list = list.clone();
    popped_list.pop_front(0).unwrap();

    for (a, b) in list
        .level_iter_from(0)
        .unwrap()
        .zip(popped_list.level_iter_from(0).unwrap())
    {
        let LevelNode::Internal(a) = a else {
            panic!("internal node expected")
        };
        let LevelNode::Internal(b) = b else {
            panic!("internal node expected")
        };
        assert!(Arc::ptr_eq(a, b));
    }
}

/// Test pop_front with wide packed leaves (PACKED_LEAF_INTERNAL_DEPTH > 0).
/// With BRANCHING_FACTOR=4 and packed_leaf_internal_depth=3, packing_factor = 4 * 2^3 = 32 for u64.
/// This exercises the `pop_front_unaligned` path where level < packing_depth.
#[test]
fn pop_front_wide_packed_leaves() {
    // BRANCHING_FACTOR=4, PACKED_LEAF_INTERNAL_DEPTH=3 → packing_factor = 4 * 8 = 32
    type WideList = List<u64, U1024, MaxMap<VecMap<u64>>, 4, 3>;

    let n = 800usize;
    let vec: Vec<u64> = (0..n as u64).collect();
    let list = WideList::new(vec.clone()).unwrap();

    // Test various pop_front values including unaligned ones
    for &pop_n in &[1, 3, 7, 15, 31, 32, 33, 63, 64, 100, 400, 799] {
        let mut popped = list.clone();
        popped.pop_front(pop_n).unwrap();

        assert_eq!(
            popped.len(),
            n - pop_n,
            "wrong length after pop_front({})",
            pop_n
        );
        assert_eq!(
            popped.to_vec().as_slice(),
            &vec[pop_n..],
            "wrong values after pop_front({})",
            pop_n
        );

        // Verify tree hash matches a freshly-built list
        let fresh = WideList::new(vec[pop_n..].to_vec()).unwrap();
        assert_eq!(
            popped.tree_hash_root(),
            fresh.tree_hash_root(),
            "tree_hash mismatch after pop_front({})",
            pop_n
        );
    }
}

/// Test pop_front with higher branching factor and larger list.
#[test]
fn pop_front_wide_packed_large() {
    // BRANCHING_FACTOR=16, PACKED_LEAF_INTERNAL_DEPTH=2 → packing_factor = 4 * 4 = 16
    type WideList = List<u64, U1024, MaxMap<VecMap<u64>>, 16, 2>;

    let n = 1000usize;
    let vec: Vec<u64> = (0..n as u64).collect();
    let list = WideList::new(vec.clone()).unwrap();

    for &pop_n in &[1, 5, 15, 16, 17, 32, 100, 500, 999] {
        let mut popped = list.clone();
        popped.pop_front(pop_n).unwrap();

        assert_eq!(popped.len(), n - pop_n);
        assert_eq!(popped.to_vec().as_slice(), &vec[pop_n..]);
    }
}
