use crate::{
    Arc, PackedLeaf, Tree, Value,
    utils::{Length, compute_level},
};

/// Iterator over the internal nodes at a given `depth` (level) in a tree.
#[derive(Debug)]
pub struct LevelIter<'a, T: Value> {
    /// Stack of (tree node, depth at this entry) pairs.
    stack: Vec<(&'a Arc<Tree<T>>, usize)>,
    /// The list index corresponding to the current position (next element to be yielded).
    index: usize,
    /// The level of the tree being iterated.
    level: usize,
    /// Cached packing factor: `1 << packing_depth`.
    packing_factor: usize,
    /// Cached packing depth.
    packing_depth: usize,
    /// Number of elements in the list being iterated.
    length: Length,
}

/// Item yielded by a `LevelIter`.
#[derive(Debug)]
pub enum LevelNode<'a, T: Value> {
    Internal(&'a Arc<Tree<T>>),
    PackedLeaf(&'a T),
}

impl<'a, T: Value> LevelIter<'a, T> {
    pub fn from_index(
        index: usize,
        root: &'a Arc<Tree<T>>,
        depth: usize,
        length: Length,
        packing_depth: usize,
        branching_factor: usize,
    ) -> Self {
        let bf_log2 = branching_factor.trailing_zeros() as usize;
        let mut level = compute_level(index, depth, packing_depth);

        // Round level down to align with branching factor boundaries.
        // Valid node depths are: D, D-F, D-2F, ..., D%F (where D=depth, F=bf_log2).
        // In terms of levels (level = depth + packing_depth), valid levels are:
        // packing_depth + D%F, packing_depth + D%F + F, ..., packing_depth + D
        if bf_log2 >= 2 && level > packing_depth {
            let target_depth = level - packing_depth;
            // Valid depths are: D - k*F for k=0,1,...  i.e. D, D-F, D-2F, ..., D%F
            // We want the largest valid depth <= target_depth.
            // That is: D - ceil((D - target_depth) / F) * F
            let k = depth.saturating_sub(target_depth).div_ceil(bf_log2);
            let valid_depth = depth.saturating_sub(k * bf_log2);
            level = packing_depth + valid_depth;
        }

        Self::from_index_at_level(index, root, depth, length, packing_depth, level)
    }

    /// Create a level iterator with an explicitly specified level.
    pub fn from_index_at_level(
        index: usize,
        root: &'a Arc<Tree<T>>,
        depth: usize,
        length: Length,
        packing_depth: usize,
        level: usize,
    ) -> Self {
        let mut stack = Vec::with_capacity(depth);
        stack.push((root, depth));

        LevelIter {
            stack,
            index,
            level,
            packing_factor: 1 << packing_depth,
            packing_depth,
            length,
        }
    }

    /// Backtrack by the given number of binary levels.
    fn backtrack(&mut self, mut levels_to_ascend: usize) {
        while levels_to_ascend > 0 {
            match self.stack.last() {
                Some((node, _)) => match node.as_ref() {
                    Tree::Node { children, .. } => {
                        let node_levels = children.len().trailing_zeros() as usize;
                        if levels_to_ascend >= node_levels {
                            self.stack.pop();
                            levels_to_ascend -= node_levels;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                },
                None => break,
            }
        }
    }
}

impl<'a, T: Value> Iterator for LevelIter<'a, T> {
    type Item = LevelNode<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.length.as_usize() {
            return None;
        }

        let (node, depth) = *self.stack.last()?;
        match node.as_ref() {
            Tree::Zero(_) => None,
            Tree::Leaf(_) => {
                let result = Some(LevelNode::Internal(node));

                debug_assert_eq!(self.level, 0);
                self.index += 1;

                self.stack.pop();
                let levels = self.index.trailing_zeros() as usize;
                self.backtrack(levels);

                result
            }
            Tree::PackedLeaf(PackedLeaf { values, .. }) => {
                let node_depth = depth + self.packing_depth;

                if node_depth == self.level {
                    let result = Some(LevelNode::Internal(node));

                    self.index += 1 << self.level;

                    let trailing_zeros = self.index.trailing_zeros() as usize;
                    debug_assert!(trailing_zeros >= self.level);
                    let levels = trailing_zeros.saturating_sub(self.level);

                    self.stack.pop();
                    self.backtrack(levels);

                    return result;
                }

                let sub_index = self.index % self.packing_factor;
                let result = values.get(sub_index).map(LevelNode::PackedLeaf);

                debug_assert_eq!(self.level, 0);
                self.index += 1;

                if sub_index + 1 == self.packing_factor {
                    let to_pop = self
                        .index
                        .trailing_zeros()
                        .checked_sub(self.packing_depth as u32)
                        .expect("index should have at least `packing_depth` trailing zeroes")
                        as usize;

                    self.stack.pop();
                    self.backtrack(to_pop);
                }

                result
            }
            Tree::Node { children, .. } => {
                let node_levels = children.len().trailing_zeros() as usize;
                let new_depth = depth.saturating_sub(node_levels);
                let node_depth = depth + self.packing_depth;

                if node_depth == self.level {
                    let result = Some(LevelNode::Internal(node));

                    self.index += 1 << self.level;

                    let trailing_zeros = self.index.trailing_zeros() as usize;
                    debug_assert!(trailing_zeros >= self.level);
                    let levels = trailing_zeros.saturating_sub(self.level);

                    self.stack.pop();
                    self.backtrack(levels);

                    result
                } else {
                    let child_index =
                        (self.index >> (new_depth + self.packing_depth)) & (children.len() - 1);
                    self.stack.push((&children[child_index], new_depth));
                    self.next()
                }
            }
        }
    }
}
