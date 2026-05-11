use crate::utils::{Length, effective_packed_leaf_depth, int_log, opt_packing_factor};
use crate::{Arc, Error, Leaf, List, PackedLeaf, Tree, UpdateMap, Value};
use parking_lot::RwLock;
use smallvec::{SmallVec, smallvec};
use tree_hash::Hash256;
use typenum::Unsigned;

/// Efficiently construct a list from `n` copies of `elem`.
pub fn repeat_list<
    T,
    N,
    U,
    const BRANCHING_FACTOR: usize,
    const PACKED_LEAF_INTERNAL_DEPTH: usize,
>(
    elem: T,
    n: usize,
) -> Result<List<T, N, U, BRANCHING_FACTOR, PACKED_LEAF_INTERNAL_DEPTH>, Error>
where
    T: Value,
    N: Unsigned,
    U: UpdateMap<T>,
{
    if n == 0 {
        return Ok(List::empty());
    }

    let log_n = int_log(N::to_usize());
    let packing_factor = opt_packing_factor::<T>(log_n, PACKED_LEAF_INTERNAL_DEPTH);
    let packed_leaf_depth = effective_packed_leaf_depth::<T>(log_n, PACKED_LEAF_INTERNAL_DEPTH);
    let tree_depth = List::<T, N, U, BRANCHING_FACTOR, PACKED_LEAF_INTERNAL_DEPTH>::depth();
    let bf_log2 = BRANCHING_FACTOR.trailing_zeros() as usize;

    let mut layer: SmallVec<[_; 2]> = if let Some(packing_factor) = packing_factor {
        let repeat_count = n / packing_factor;
        let lonely_count = n % packing_factor;
        let repeat_leaf = Arc::new(Tree::PackedLeaf(PackedLeaf::repeat(
            elem.clone(),
            packing_factor,
            packed_leaf_depth,
        )));
        let lonely_leaf = Arc::new(Tree::PackedLeaf(PackedLeaf::repeat(
            elem,
            lonely_count,
            packed_leaf_depth,
        )));
        match (repeat_count, lonely_count) {
            (0, 0) => unreachable!("n != 0"),
            (_, 0) => smallvec![(repeat_leaf, repeat_count)],
            (0, _) => smallvec![(lonely_leaf, 1)],
            (_, _) => {
                smallvec![(repeat_leaf, repeat_count), (lonely_leaf, 1)]
            }
        }
    } else {
        smallvec![(Arc::new(Tree::Leaf(Leaf::new(elem))), n)]
    };

    // Build the tree bottom-up, merging in groups of bf_log2 levels.
    // We process `tree_depth` binary levels total, but merge `bf_log2` at a time.
    // The FIRST merge from the bottom uses `bottom_merge_width` (= tree_depth % bf_log2),
    // and subsequent merges use `min(bf_log2, remaining_levels)`.
    // This matches the builder's greedy-from-top layout.
    let bottom_merge_width = {
        let remainder = tree_depth % bf_log2;
        if remainder == 0 { bf_log2 } else { remainder }
    };
    let mut levels_processed = 0;
    while levels_processed < tree_depth {
        let levels_this_group = if levels_processed == 0 {
            bottom_merge_width
        } else {
            std::cmp::min(bf_log2, tree_depth - levels_processed)
        };
        let group_width = 1usize << levels_this_group;

        // Each iteration of this loop merges `levels_this_group` binary levels.
        let new_layer = match &layer[..] {
            [(repeat_leaf, 1)] => {
                // Single item: pad with zeros to fill a node of `group_width` children.
                let zero_depth = levels_processed + packed_leaf_depth;
                let zero = Tree::packed_zero(zero_depth, 0);
                let mut children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                children.push(repeat_leaf.clone());
                for _ in 1..group_width {
                    children.push(zero.clone());
                }
                smallvec![(
                    Arc::new(Tree::Node {
                        hash: RwLock::new(Hash256::ZERO),
                        children: children.into_boxed_slice(),
                    }),
                    1,
                )]
            }
            [(repeat_leaf, repeat_count)] if repeat_count.is_multiple_of(group_width) => {
                // Perfect division: create uniform nodes.
                let mut children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                for _ in 0..group_width {
                    children.push(repeat_leaf.clone());
                }
                smallvec![(
                    Arc::new(Tree::Node {
                        hash: RwLock::new(Hash256::ZERO),
                        children: children.into_boxed_slice(),
                    }),
                    repeat_count / group_width,
                )]
            }
            [(repeat_leaf, repeat_count)] => {
                // Not evenly divisible: full nodes + one partial node with zeros.
                let full_nodes = repeat_count / group_width;
                let remainder = repeat_count % group_width;

                let mut full_children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                for _ in 0..group_width {
                    full_children.push(repeat_leaf.clone());
                }
                let full_node = Arc::new(Tree::Node {
                    hash: RwLock::new(Hash256::ZERO),
                    children: full_children.into_boxed_slice(),
                });

                let zero_depth = levels_processed + packed_leaf_depth;
                let zero = Tree::packed_zero(zero_depth, 0);
                let mut partial_children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                for _ in 0..remainder {
                    partial_children.push(repeat_leaf.clone());
                }
                for _ in remainder..group_width {
                    partial_children.push(zero.clone());
                }
                let partial_node = Arc::new(Tree::Node {
                    hash: RwLock::new(Hash256::ZERO),
                    children: partial_children.into_boxed_slice(),
                });

                if full_nodes > 0 {
                    smallvec![(full_node, full_nodes), (partial_node, 1)]
                } else {
                    smallvec![(partial_node, 1)]
                }
            }
            [(repeat_leaf, 1), (lonely_leaf, 1)] => {
                // Two items: place in a node, pad with zeros.
                let zero_depth = levels_processed + packed_leaf_depth;
                let zero = Tree::packed_zero(zero_depth, 0);
                let mut children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                children.push(repeat_leaf.clone());
                children.push(lonely_leaf.clone());
                for _ in 2..group_width {
                    children.push(zero.clone());
                }
                smallvec![(
                    Arc::new(Tree::Node {
                        hash: RwLock::new(Hash256::ZERO),
                        children: children.into_boxed_slice(),
                    }),
                    1,
                )]
            }
            [(repeat_leaf, repeat_count), (lonely_leaf, 1)] => {
                let full_nodes = repeat_count / group_width;
                let remainder = *repeat_count % group_width;

                let mut full_children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                for _ in 0..group_width {
                    full_children.push(repeat_leaf.clone());
                }
                let full_node = Arc::new(Tree::Node {
                    hash: RwLock::new(Hash256::ZERO),
                    children: full_children.into_boxed_slice(),
                });

                // The last node has `remainder` repeat items + lonely + zeros
                let zero_depth = levels_processed + packed_leaf_depth;
                let zero = Tree::packed_zero(zero_depth, 0);
                let mut partial_children: Vec<Arc<Tree<T>>> = Vec::with_capacity(group_width);
                for _ in 0..remainder {
                    partial_children.push(repeat_leaf.clone());
                }
                partial_children.push(lonely_leaf.clone());
                for _ in (remainder + 1)..group_width {
                    partial_children.push(zero.clone());
                }
                let partial_node = Arc::new(Tree::Node {
                    hash: RwLock::new(Hash256::ZERO),
                    children: partial_children.into_boxed_slice(),
                });

                if full_nodes > 0 {
                    smallvec![(full_node, full_nodes), (partial_node, 1)]
                } else {
                    smallvec![(partial_node, 1)]
                }
            }
            _ => unreachable!("not possible"),
        };
        drop(std::mem::replace(&mut layer, new_layer));
        levels_processed += levels_this_group;
    }

    let (root, count) = layer.pop().ok_or(Error::BuilderStackEmptyFinalize)?;
    if !layer.is_empty() || count != 1 {
        return Err(Error::BuilderStackLeftover);
    }

    Ok(List::from_parts(root, tree_depth, Length(n)))
}
