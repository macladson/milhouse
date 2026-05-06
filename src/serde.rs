use crate::{List, UpdateMap, Value, utils::DEFAULT_SUBTREE_DEPTH};
use itertools::process_results;
use serde::Deserialize;
use std::marker::PhantomData;
use typenum::Unsigned;

pub struct ListVisitor<T, N, U, const MAX_SUBTREE_DEPTH: usize = DEFAULT_SUBTREE_DEPTH> {
    _phantom: PhantomData<(T, N, U)>,
}

impl<T, N, U, const MAX_SUBTREE_DEPTH: usize> Default for ListVisitor<T, N, U, MAX_SUBTREE_DEPTH> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<'a, T, N, U, const MAX_SUBTREE_DEPTH: usize> serde::de::Visitor<'a>
    for ListVisitor<T, N, U, MAX_SUBTREE_DEPTH>
where
    T: Deserialize<'a> + Value,
    N: Unsigned,
    U: UpdateMap<T>,
{
    type Value = List<T, N, U, MAX_SUBTREE_DEPTH>;

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
