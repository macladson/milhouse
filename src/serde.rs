use crate::{
    List, UpdateMap, Value,
    utils::{DEFAULT_BRANCHING_FACTOR, DEFAULT_PACKED_LEAF_INTERNAL_DEPTH},
};
use itertools::process_results;
use serde::Deserialize;
use std::marker::PhantomData;
use typenum::Unsigned;

pub struct ListVisitor<
    T,
    N,
    U,
    const BRANCHING_FACTOR: usize = DEFAULT_BRANCHING_FACTOR,
    const PACKED_LEAF_INTERNAL_DEPTH: usize = DEFAULT_PACKED_LEAF_INTERNAL_DEPTH,
> {
    _phantom: PhantomData<(T, N, U)>,
}

impl<T, N, U, const BRANCHING_FACTOR: usize, const PACKED_LEAF_INTERNAL_DEPTH: usize> Default
    for ListVisitor<T, N, U, BRANCHING_FACTOR, PACKED_LEAF_INTERNAL_DEPTH>
{
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<'a, T, N, U, const BRANCHING_FACTOR: usize, const PACKED_LEAF_INTERNAL_DEPTH: usize>
    serde::de::Visitor<'a> for ListVisitor<T, N, U, BRANCHING_FACTOR, PACKED_LEAF_INTERNAL_DEPTH>
where
    T: Deserialize<'a> + Value,
    N: Unsigned,
    U: UpdateMap<T>,
{
    type Value = List<T, N, U, BRANCHING_FACTOR, PACKED_LEAF_INTERNAL_DEPTH>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a list of T")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'a>,
    {
        process_results(
            std::iter::from_fn(|| seq.next_element().transpose()),
            |iter| {
                List::try_from_iter(iter).map_err(|e| {
                    serde::de::Error::custom(format!("Error deserializing List: {e:?}"))
                })
            },
        )?
    }
}
