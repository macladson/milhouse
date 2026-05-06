use crate::tests::proptest::Large;
use crate::utils::{DEFAULT_SUBTREE_DEPTH, int_log, opt_packing_depth};
use crate::{Arc, List, level_iter::LevelNode};
use tree_hash::Hash256;
use typenum::{U8, U32};

#[test]
fn level_iter_pop_front_basic_packed() {
    let vec = vec![10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    let list = List::<u64, U32>::new(vec.clone()).unwrap();
    assert_eq!(list.len(), 11);

    let packing_depth = opt_packing_depth::<u64>(int_log(32), DEFAULT_SUBTREE_DEPTH).unwrap_or(0);

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
                    let level = if from == 0 {
                        list.len().trailing_zeros() as usize + packing_depth
                    } else {
                        from.trailing_zeros() as usize
                    };
                    assert!(level >= packing_depth);
                    assert!(node.compute_len() <= 1 << level);
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

    let packing_depth = opt_packing_depth::<u64>(int_log(32), DEFAULT_SUBTREE_DEPTH).unwrap_or(0);

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
                    let level = if from == 0 {
                        list.len().trailing_zeros() as usize + packing_depth
                    } else {
                        from.trailing_zeros() as usize
                    };
                    assert!(level >= packing_depth);
                    assert!(node.compute_len() <= 1 << level);
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
