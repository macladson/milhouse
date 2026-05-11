use crate::List;
use ssz_types::VariableList;
use tree_hash::{Hash256, TreeHash};
use typenum::{U16, U32, Unsigned};

#[test]
fn build_partial_hash256_list() {
    type N = U16;
    let n = N::to_usize();
    let vec = (0..n as u64)
        .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
        .collect::<Vec<_>>();

    for k in 0..n {
        let sub_vec = vec[0..k].to_vec();

        let fast_list = List::<Hash256, N>::try_from_iter(sub_vec.clone()).unwrap();
        let slow_list = List::<Hash256, N>::try_from_iter_slow(sub_vec.clone()).unwrap();

        assert_eq!(fast_list, slow_list);
        assert_eq!(fast_list.iter().cloned().collect::<Vec<_>>(), sub_vec);
    }
}

/// Verify default List<Hash256, U32> tree hash matches ssz_types reference at all lengths.
#[test]
fn tree_hash_hash256_u32() {
    type N = U32;

    // Test various lengths that exercise the padding loop
    for len in 0..=N::to_usize() {
        let values: Vec<Hash256> = (0..len as u64)
            .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
            .collect();

        let milhouse_list = List::<Hash256, N>::try_from_iter(values.clone()).unwrap();
        let ref_list = VariableList::<Hash256, N>::new(values.clone()).unwrap();

        let milhouse_hash = milhouse_list.tree_hash_root();
        let ref_hash = ref_list.tree_hash_root();

        assert_eq!(
            milhouse_hash, ref_hash,
            "tree_hash mismatch for List<Hash256, U32> with {} elements",
            len
        );
    }
}

#[test]
fn build_packed_u64_list() {
    type N = U32;
    let n = N::to_usize();
    let vec = (0..n as u64).collect::<Vec<_>>();

    for k in 0..n {
        let sub_vec = vec[0..k].to_vec();

        let fast_list = List::<u64, N>::try_from_iter(sub_vec.clone()).unwrap();
        let slow_list = List::<u64, N>::try_from_iter(sub_vec.clone()).unwrap();

        assert_eq!(fast_list, slow_list);
        assert_eq!(fast_list.iter().cloned().collect::<Vec<_>>(), sub_vec);
    }
}

/// Verify BRANCHING_FACTOR=8 (8-child nodes) produces correct tree hashes for Hash256.
#[test]
fn wide_branching_8_hash256() {
    type N = U32;
    type WideList = List<Hash256, N, crate::update_map::MaxMap<vec_map::VecMap<Hash256>>, 8, 0>;

    for len in 0..=N::to_usize() {
        let values: Vec<Hash256> = (0..len as u64)
            .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
            .collect();

        let wide_list = WideList::try_from_iter(values.clone()).unwrap();
        let ref_list = VariableList::<Hash256, N>::new(values).unwrap();

        assert_eq!(
            wide_list.tree_hash_root(),
            ref_list.tree_hash_root(),
            "tree_hash mismatch for BRANCHING_FACTOR=8 with {} elements",
            len
        );
    }
}

/// Verify BRANCHING_FACTOR=16 (16-child nodes) produces correct tree hashes for Hash256.
#[test]
fn wide_branching_16_hash256() {
    type N = U16;
    type WideList = List<Hash256, N, crate::update_map::MaxMap<vec_map::VecMap<Hash256>>, 16, 0>;

    for len in 0..=N::to_usize() {
        let values: Vec<Hash256> = (0..len as u64)
            .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
            .collect();

        let wide_list = WideList::try_from_iter(values.clone()).unwrap();
        let ref_list = VariableList::<Hash256, N>::new(values).unwrap();

        assert_eq!(
            wide_list.tree_hash_root(),
            ref_list.tree_hash_root(),
            "tree_hash mismatch for BRANCHING_FACTOR=16 with {} elements",
            len
        );
    }
}

/// Verify higher branching factors produce correct tree hashes for packed u64 lists.
#[test]
fn wide_branching_packed_u64() {
    type N = U32;
    type Wide4 = List<u64, N, crate::update_map::MaxMap<vec_map::VecMap<u64>>, 4, 0>;
    type Wide8 = List<u64, N, crate::update_map::MaxMap<vec_map::VecMap<u64>>, 8, 0>;

    for len in 0..=N::to_usize() {
        let values: Vec<u64> = (0..len as u64).collect();

        let list4 = Wide4::try_from_iter(values.clone()).unwrap();
        let list8 = Wide8::try_from_iter(values.clone()).unwrap();

        let ref_list = VariableList::<u64, N>::new(values).unwrap();
        let ref_hash = ref_list.tree_hash_root();

        assert_eq!(
            list4.tree_hash_root(),
            ref_hash,
            "BRANCHING_FACTOR=4 mismatch at len={}",
            len
        );
        assert_eq!(
            list8.tree_hash_root(),
            ref_hash,
            "BRANCHING_FACTOR=8 mismatch at len={}",
            len
        );
    }
}

/// Test that higher branching factor lists have correct iteration and element access.
#[test]
fn wide_branching_iteration_and_access() {
    type N = U16;
    type WideList = List<Hash256, N, crate::update_map::MaxMap<vec_map::VecMap<Hash256>>, 8, 0>;

    let values: Vec<Hash256> = (0..10u64)
        .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
        .collect();

    let list = WideList::try_from_iter(values.clone()).unwrap();

    // Test iteration
    assert_eq!(list.iter().cloned().collect::<Vec<_>>(), values);

    // Test random access
    for (i, v) in values.iter().enumerate() {
        assert_eq!(list.get(i), Some(v));
    }
    assert_eq!(list.get(10), None);
}

/// Test that mutation works correctly with higher branching factor.
#[test]
fn wide_branching_mutation() {
    type N = U16;
    type WideList = List<Hash256, N, crate::update_map::MaxMap<vec_map::VecMap<Hash256>>, 8, 0>;

    let values: Vec<Hash256> = (0..8u64)
        .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
        .collect();

    let mut list = WideList::try_from_iter(values).unwrap();

    // Mutate an element
    let new_val = Hash256::right_padding_from(&99u64.to_le_bytes());
    *list.get_mut(3).unwrap() = new_val;
    list.apply_updates().unwrap();

    assert_eq!(list.get(3), Some(&new_val));

    // Verify tree hash still matches reference after mutation
    let mut ref_values: Vec<Hash256> = (0..8u64)
        .map(|n| Hash256::right_padding_from(&n.to_le_bytes()))
        .collect();
    ref_values[3] = new_val;
    let ref_list = VariableList::<Hash256, N>::new(ref_values).unwrap();
    assert_eq!(list.tree_hash_root(), ref_list.tree_hash_root());
}
