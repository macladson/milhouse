use crate::utils::{
    DEFAULT_BRANCHING_FACTOR, DEFAULT_PACKED_LEAF_INTERNAL_DEPTH, Length, MaybeArced,
    effective_packed_leaf_depth, opt_packing_depth, opt_packing_factor,
};
use crate::{Arc, Error, MAX_TREE_DEPTH, PackedLeaf, Tree, Value};
use parking_lot::RwLock;
use tree_hash::Hash256;

#[derive(Debug)]
pub struct Builder<T: Value> {
    stack: Vec<MaybeArced<Tree<T>>>,
    /// The depth of the tree excluding the packing depth.
    depth: usize,
    /// The level at which nodes are pushed via `push_node`.
    level: usize,
    length: Length,
    /// Cached value of `opt_packing_factor`.
    packing_factor: Option<usize>,
    /// Cached value of `opt_packing_depth`.
    packing_depth: usize,
    /// Cached value of `effective_packed_leaf_depth`.
    packed_leaf_depth: usize,
    /// log2(branching_factor): binary levels absorbed per node.
    levels_per_node: usize,
    /// Cached value of capacity: 2^(depth + packing_depth).
    capacity: usize,
}

impl<T: Value> Builder<T> {
    pub fn new(depth: usize, level: usize, log_n: usize) -> Result<Self, Error> {
        Self::new_with_options(
            depth,
            level,
            log_n,
            DEFAULT_PACKED_LEAF_INTERNAL_DEPTH,
            DEFAULT_BRANCHING_FACTOR,
        )
    }

    pub fn new_with_options(
        depth: usize,
        level: usize,
        log_n: usize,
        max_packed_leaf_depth: usize,
        branching_factor: usize,
    ) -> Result<Self, Error> {
        let packing_depth = opt_packing_depth::<T>(log_n, max_packed_leaf_depth).unwrap_or(0);
        let packed_leaf_depth = effective_packed_leaf_depth::<T>(log_n, max_packed_leaf_depth);
        if depth.saturating_add(packing_depth) > MAX_TREE_DEPTH {
            Err(Error::BuilderInvalidDepth { depth })
        } else {
            let capacity = 1 << (depth + packing_depth);
            Ok(Self {
                stack: Vec::with_capacity(depth),
                depth,
                level,
                length: Length(0),
                packing_factor: opt_packing_factor::<T>(log_n, max_packed_leaf_depth),
                packing_depth,
                packed_leaf_depth,
                levels_per_node: branching_factor.trailing_zeros() as usize,
                capacity,
            })
        }
    }

    /// Pop `2^merge_depth - 1` items from the stack and combine with `top` (rightmost)
    /// into a single node. Collapses all-zero children into a single Zero.
    fn merge_children(
        &mut self,
        top: MaybeArced<Tree<T>>,
        merge_depth: usize,
    ) -> Result<MaybeArced<Tree<T>>, Error> {
        // Fast path for binary merge: no Vec, no reverse, inline zero check.
        if merge_depth == 1 {
            let left = self
                .stack
                .pop()
                .ok_or(Error::BuilderStackEmptyMerge)?
                .arced();
            let right = top.arced();
            if let (Tree::Zero(d1), Tree::Zero(d2)) = (left.as_ref(), right.as_ref())
                && d1 == d2
            {
                return Ok(MaybeArced::Unarced(Tree::Zero(d1 + 1)));
            }
            return Ok(MaybeArced::Unarced(Tree::Node {
                hash: RwLock::new(Hash256::ZERO),
                children: Box::new([left, right]),
            }));
        }

        let n = 1usize << merge_depth;
        let mut children = Vec::with_capacity(n);
        for _ in 0..n - 1 {
            children.push(
                self.stack
                    .pop()
                    .ok_or(Error::BuilderStackEmptyMerge)?
                    .arced(),
            );
        }
        children.reverse();
        children.push(top.arced());

        if let Some(child_zero_depth) = Self::all_zeros_depth(&children) {
            return Ok(MaybeArced::Unarced(Tree::Zero(
                child_zero_depth + merge_depth,
            )));
        }

        Ok(MaybeArced::Unarced(Tree::Node {
            hash: RwLock::new(Hash256::ZERO),
            children: children.into_boxed_slice(),
        }))
    }

    /// Merge at the standard levels_per_node width.
    fn merge_standard(&mut self, top: MaybeArced<Tree<T>>) -> Result<MaybeArced<Tree<T>>, Error> {
        self.merge_children(top, self.levels_per_node)
    }

    /// Total binary merge levels above the pushed items.
    fn effective_merge_depth(&self) -> usize {
        if self.level == 0 {
            self.depth
        } else {
            self.depth + self.packing_depth - self.level
        }
    }

    /// Merge width exponent for the lowest node level (remainder of depth / levels_per_node).
    fn bottom_merge_width(&self) -> usize {
        let effective = self.effective_merge_depth();
        let remainder = effective % self.levels_per_node;
        if remainder == 0 {
            self.levels_per_node
        } else {
            remainder
        }
    }

    /// If all children are Zero(d), returns Some(d). Otherwise None.
    fn all_zeros_depth(children: &[Arc<Tree<T>>]) -> Option<usize> {
        let mut zero_depth = None;
        for child in children {
            match child.as_ref() {
                Tree::Zero(d) => match zero_depth {
                    Some(prev) if prev == *d => {}
                    None => zero_depth = Some(*d),
                    _ => return None,
                },
                _ => return None,
            }
        }
        zero_depth
    }

    pub fn push(&mut self, value: T) -> Result<(), Error> {
        if self.length.as_usize() == self.capacity {
            return Err(Error::BuilderFull);
        }
        let index = self.length.as_usize();
        let next_index = index + 1;

        let mut new_stack_top = if let Some(packing_factor) = self.packing_factor {
            if index.is_multiple_of(packing_factor) {
                MaybeArced::Unarced(Tree::PackedLeaf(PackedLeaf::single(
                    value,
                    self.packed_leaf_depth,
                )))
            } else if let Some(MaybeArced::Unarced(Tree::PackedLeaf(mut leaf))) = self.stack.pop() {
                leaf.push(value, self.packed_leaf_depth)?;
                MaybeArced::Unarced(Tree::PackedLeaf(leaf))
            } else {
                return Err(Error::BuilderExpectedLeaf);
            }
        } else {
            MaybeArced::Unarced(Tree::leaf_unboxed(value))
        };

        let mut values_to_merge = next_index
            .trailing_zeros()
            .saturating_sub(self.packing_depth as u32) as usize;

        let bottom_merge_width = self.bottom_merge_width();
        if values_to_merge >= bottom_merge_width {
            new_stack_top = self.merge_children(new_stack_top, bottom_merge_width)?;
            values_to_merge -= bottom_merge_width;
            while values_to_merge >= self.levels_per_node {
                new_stack_top = self.merge_standard(new_stack_top)?;
                values_to_merge -= self.levels_per_node;
            }
        }

        self.stack.push(new_stack_top);
        *self.length.as_mut() += 1;

        Ok(())
    }

    /// Push a pre-built packed leaf directly onto the stack. Only valid for level-0
    /// builders when length is aligned to `packing_factor`.
    pub fn push_leaf_node(&mut self, node: Arc<Tree<T>>, subtree_len: usize) -> Result<(), Error> {
        debug_assert_eq!(
            self.level, 0,
            "push_leaf_node only works on level-0 builders"
        );
        let packing_factor = self.packing_factor.unwrap_or(1);
        debug_assert!(
            self.length.as_usize().is_multiple_of(packing_factor),
            "push_leaf_node requires packing-aligned length, got {}",
            self.length.as_usize()
        );

        if self.length.as_usize() + subtree_len > self.capacity {
            return Err(Error::BuilderFull);
        }

        *self.length.as_mut() += subtree_len;
        let next_index = self.length.as_usize();

        let mut new_stack_top = MaybeArced::Arced(node);

        let mut values_to_merge = next_index
            .trailing_zeros()
            .saturating_sub(self.packing_depth as u32) as usize;

        let bottom_merge_width = self.bottom_merge_width();
        if values_to_merge >= bottom_merge_width {
            new_stack_top = self.merge_children(new_stack_top, bottom_merge_width)?;
            values_to_merge -= bottom_merge_width;
            while values_to_merge >= self.levels_per_node {
                new_stack_top = self.merge_standard(new_stack_top)?;
                values_to_merge -= self.levels_per_node;
            }
        }

        self.stack.push(new_stack_top);
        Ok(())
    }

    pub fn push_node(&mut self, node: Arc<Tree<T>>, len: usize) -> Result<(), Error> {
        if self.length.as_usize() == self.capacity {
            return Err(Error::BuilderFull);
        }

        let index_on_level = self.length.as_usize() >> self.level;
        let next_index_on_level = index_on_level + 1;

        let mut new_stack_top = MaybeArced::Arced(node);

        let mut values_to_merge = if self.level == 0 {
            next_index_on_level
                .trailing_zeros()
                .saturating_sub(self.packing_depth as u32) as usize
        } else {
            next_index_on_level.trailing_zeros() as usize
        };

        let bottom_merge_width = self.bottom_merge_width();
        if values_to_merge >= bottom_merge_width {
            new_stack_top = self.merge_children(new_stack_top, bottom_merge_width)?;
            values_to_merge -= bottom_merge_width;
            while values_to_merge >= self.levels_per_node {
                new_stack_top = self.merge_standard(new_stack_top)?;
                values_to_merge -= self.levels_per_node;
            }
        }

        self.stack.push(new_stack_top);
        *self.length.as_mut() += len;

        Ok(())
    }

    pub fn finish(mut self) -> Result<(Arc<Tree<T>>, usize, Length), Error> {
        if self.stack.is_empty() {
            return Ok((
                Tree::packed_zero(self.depth, self.packed_leaf_depth),
                self.depth,
                Length(0),
            ));
        }

        let length = self.length.as_usize();
        let level_capacity = 1 << self.level;
        let mut next_index_on_level = length.div_ceil(level_capacity);

        // Finish any partially-filled packed leaf.
        if let Some(packing_factor) = self.packing_factor {
            let skip_indices = packing_factor
                .saturating_sub(self.length.as_usize() % packing_factor)
                % packing_factor;

            if skip_indices > 0 && self.level == 0 {
                next_index_on_level += skip_indices;

                // Merge the partial packed leaf at the bottom level.
                let mut values_to_merge = (next_index_on_level.trailing_zeros() as usize)
                    .saturating_sub(self.packing_depth);
                let bottom_merge_width = self.bottom_merge_width();
                if values_to_merge >= bottom_merge_width && !self.stack.is_empty() {
                    let mut new_stack_top =
                        self.stack.pop().ok_or(Error::BuilderStackEmptyMerge)?;
                    new_stack_top = self.merge_children(new_stack_top, bottom_merge_width)?;
                    values_to_merge -= bottom_merge_width;
                    while values_to_merge >= self.levels_per_node {
                        new_stack_top = self.merge_standard(new_stack_top)?;
                        values_to_merge -= self.levels_per_node;
                    }
                    self.stack.push(new_stack_top);
                }
            }
        }

        // Pad with zeros up to full capacity. For each incomplete node group,
        // fill remaining slots with zeros then merge.
        let bottom_merge_width = self.bottom_merge_width();
        let base_zero_offset =
            self.level.saturating_sub(self.packing_depth) + self.packed_leaf_depth;

        while next_index_on_level << self.level != self.capacity {
            // Trailing zeros tell us how many merge levels have completed at this position.
            let tz = if self.level == 0 {
                (next_index_on_level.trailing_zeros() as usize).saturating_sub(self.packing_depth)
            } else {
                next_index_on_level.trailing_zeros() as usize
            };

            // Item depth = largest completed merge boundary <= tz.
            let item_depth = if tz < bottom_merge_width {
                0
            } else {
                let levels_above_bottom = tz - bottom_merge_width;
                let complete_upper_groups = levels_above_bottom / self.levels_per_node;
                bottom_merge_width + complete_upper_groups * self.levels_per_node
            };

            let group_merge_width = if item_depth < bottom_merge_width {
                bottom_merge_width
            } else {
                self.levels_per_node
            };

            let items_in_group = if item_depth == 0 {
                let group_size = 1usize << bottom_merge_width;
                let pos_in_items = if self.level == 0 {
                    next_index_on_level >> self.packing_depth
                } else {
                    next_index_on_level
                };
                pos_in_items % group_size
            } else {
                let group_size = 1usize << self.levels_per_node;
                let pos_at_level = if self.level == 0 {
                    next_index_on_level >> (item_depth + self.packing_depth)
                } else {
                    next_index_on_level >> item_depth
                };
                pos_at_level % group_size
            };

            let total_group_items = 1usize << group_merge_width;
            let zeros_needed = total_group_items - items_in_group;
            let positions_per_item = if self.level == 0 {
                1usize << (item_depth + self.packing_depth)
            } else {
                1usize << item_depth
            };

            for _ in 0..zeros_needed {
                let zero = Tree::packed_zero(item_depth + base_zero_offset, 0);
                self.stack.push(MaybeArced::Arced(zero));
                next_index_on_level += positions_per_item;
            }

            let mut new_stack_top = self.stack.pop().ok_or(Error::BuilderStackEmptyMerge)?;
            new_stack_top = self.merge_children(new_stack_top, group_merge_width)?;

            // Cascade further merges if trailing zeros exceed what we just merged.
            let merged_to = item_depth + group_merge_width;
            let new_tz = if self.level == 0 {
                (next_index_on_level.trailing_zeros() as usize).saturating_sub(self.packing_depth)
            } else {
                next_index_on_level.trailing_zeros() as usize
            };
            let mut remaining = new_tz.saturating_sub(merged_to);
            while remaining >= self.levels_per_node {
                new_stack_top = self.merge_standard(new_stack_top)?;
                remaining -= self.levels_per_node;
            }

            self.stack.push(new_stack_top);
        }

        if self.stack.len() == 1 {
            let tree = self.stack.pop().expect("stack should have 1 item").arced();
            Ok((tree, self.depth, self.length))
        } else {
            let remaining = self.stack.len();
            debug_assert!(
                remaining.is_power_of_two(),
                "remaining stack items should be a power of 2, got {}",
                remaining
            );
            let mut children: Vec<Arc<Tree<T>>> = Vec::with_capacity(remaining);
            for item in self.stack.drain(..) {
                children.push(item.arced());
            }
            let tree = Tree::node(children.into_boxed_slice(), Hash256::ZERO);
            Ok((tree, self.depth, self.length))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn depth_upper_limit() {
        let log_n = 64;
        let packing_depth =
            opt_packing_depth::<u64>(log_n, DEFAULT_PACKED_LEAF_INTERNAL_DEPTH).unwrap();
        let invalid_depth = MAX_TREE_DEPTH + 1 - packing_depth;
        let valid_depth = MAX_TREE_DEPTH - packing_depth;

        assert_eq!(
            Builder::<u64>::new(invalid_depth, 0, log_n).unwrap_err(),
            Error::BuilderInvalidDepth {
                depth: invalid_depth
            }
        );
        assert_eq!(
            Builder::<u64>::new(valid_depth, 0, log_n).unwrap().depth,
            valid_depth
        );
    }
}
