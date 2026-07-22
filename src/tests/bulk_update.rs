//! Tests for `bulk_update` validation.

use crate::update_map::MaxMap;
use crate::{Error, List, UpdateMap};
use std::collections::BTreeMap;
use typenum::{U16, U1024};
use vec_map::VecMap;

#[test]
fn bulk_update_rejects_gaps() {
    let mut list = List::<u64, U1024>::new(vec![0, 1, 2, 3]).unwrap();
    let mut updates = MaxMap::<VecMap<u64>>::default();
    updates.insert(6, 66);
    assert_eq!(
        list.bulk_update(updates),
        Err(Error::BulkUpdateGap { first_gap: 4 })
    );
    // A failed bulk update leaves the list unchanged.
    assert!(!list.has_pending_updates());
    assert_eq!(list.to_vec(), vec![0, 1, 2, 3]);
}

#[test]
fn bulk_update_rejects_appends_beyond_capacity() {
    let mut list = List::<u64, U16>::new((0..16).collect()).unwrap();
    let mut updates = MaxMap::<VecMap<u64>>::default();
    updates.insert(16, 1);
    assert_eq!(list.bulk_update(updates), Err(Error::ListFull { len: 16 }));
}

#[test]
fn bulk_update_rejects_inconsistent_max_index() {
    // `get_mut_with` doesn't update the `MaxMap`'s `max_index`, so applying this map would
    // silently drop the appended entry.
    let mut updates = MaxMap::<VecMap<u64>>::default();
    *updates.get_mut_with(3, |_| Some(33)).unwrap() = 33;

    let mut list = List::<u64, U1024>::new(vec![0, 1, 2]).unwrap();
    assert_eq!(
        list.bulk_update(updates),
        Err(Error::BulkUpdateInconsistentMaxIndex {
            reported_len: 3,
            actual_len: 4,
        })
    );
    assert!(!list.has_pending_updates());
}

#[test]
fn bulk_update_accepts_valid_replaces_and_appends() {
    let mut updates = MaxMap::<VecMap<u64>>::default();
    updates.insert(1, 11);
    updates.insert(3, 33);
    updates.insert(4, 44);

    let mut list = List::<u64, U1024>::new(vec![0, 1, 2]).unwrap();
    list.bulk_update(updates).unwrap();
    list.apply_updates().unwrap();
    assert_eq!(list.to_vec(), vec![0, 11, 2, 33, 44]);
}

#[test]
fn bulk_update_accepts_replace_via_get_mut_with_plus_appends() {
    // Replaces via `get_mut_with` don't update `max_index`; that's fine as long as appends
    // (via `insert`) do.
    let mut updates = MaxMap::<VecMap<u64>>::default();
    *updates.get_mut_with(1, |_| Some(0)).unwrap() = 111;
    updates.insert(3, 33);

    let mut list = List::<u64, U1024>::new(vec![0, 1, 2]).unwrap();
    list.bulk_update(updates).unwrap();
    list.apply_updates().unwrap();
    assert_eq!(list.to_vec(), vec![0, 111, 2, 33]);
}

#[test]
fn bulk_update_rejects_entry_at_usize_max() {
    // An entry at `usize::MAX` implies a length of `usize::MAX + 1`, which used to overflow
    // `updated_length`.
    let mut updates = BTreeMap::new();
    updates.insert(usize::MAX, 99u64);

    let mut list = List::<u64, U1024, BTreeMap<usize, u64>>::new(vec![0, 1, 2]).unwrap();
    assert_eq!(
        list.bulk_update(updates),
        Err(Error::ListFull { len: usize::MAX })
    );
    assert!(!list.has_pending_updates());
    assert_eq!(list.to_vec(), vec![0, 1, 2]);
}
