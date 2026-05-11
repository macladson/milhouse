use crate::{Leaf, PackedLeaf, Tree, Value, utils::Length};

#[derive(Debug)]
pub struct Iter<'a, T: Value> {
    /// Stack of (tree node, depth at this entry) pairs.
    /// `depth` is the remaining binary tree depth at this node.
    stack: Vec<(&'a Tree<T>, usize)>,
    /// The list index corresponding to the current position (next element to be yielded).
    index: usize,
    /// Cached packing factor: `1 << packing_depth`.
    packing_factor: usize,
    /// Cached packing depth.
    packing_depth: usize,
    /// Number of items that will be yielded by the iterator.
    length: Length,
}

impl<'a, T: Value> Iter<'a, T> {
    pub fn from_index(
        index: usize,
        root: &'a Tree<T>,
        depth: usize,
        length: Length,
        packing_depth: usize,
    ) -> Self {
        let mut stack = Vec::with_capacity(depth);
        stack.push((root, depth));

        Iter {
            stack,
            index,
            packing_factor: 1 << packing_depth,
            packing_depth,
            length,
        }
    }

    /// Backtrack after yielding a leaf. Pops entries from the stack until we reach
    /// a node whose next child should be visited.
    fn backtrack(&mut self) {
        let mut levels_to_ascend = self.index.trailing_zeros() as usize;

        while levels_to_ascend > 0 {
            match self.stack.last() {
                Some((Tree::Node { children, .. }, _)) => {
                    let node_levels = children.len().trailing_zeros() as usize;
                    if levels_to_ascend >= node_levels {
                        self.stack.pop();
                        levels_to_ascend -= node_levels;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    /// Backtrack after reaching end of a packed leaf.
    fn backtrack_packed(&mut self) {
        let to_pop = self
            .index
            .trailing_zeros()
            .checked_sub(self.packing_depth as u32)
            .expect("index should have at least `packing_depth` trailing zeroes")
            as usize;

        let mut levels_to_ascend = to_pop;

        // Pop the packed leaf itself
        self.stack.pop();

        while levels_to_ascend > 0 {
            match self.stack.last() {
                Some((Tree::Node { children, .. }, _)) => {
                    let node_levels = children.len().trailing_zeros() as usize;
                    if levels_to_ascend >= node_levels {
                        self.stack.pop();
                        levels_to_ascend -= node_levels;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }
}

impl<'a, T: Value> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.length.as_usize() {
            return None;
        }

        match self.stack.last() {
            None | Some((Tree::Zero(_), _)) => None,
            Some((Tree::Leaf(Leaf { value, .. }), _)) => {
                let result = Some(value.as_ref());

                self.index += 1;

                // Pop the leaf itself
                self.stack.pop();
                // Backtrack
                self.backtrack();

                result
            }
            Some((Tree::PackedLeaf(PackedLeaf { values, .. }), _)) => {
                let sub_index = self.index % self.packing_factor;

                let result = values.get(sub_index);

                self.index += 1;

                // Reached end of chunk
                if sub_index + 1 == self.packing_factor {
                    self.backtrack_packed();
                }

                result
            }
            Some((Tree::Node { children, .. }, depth)) => {
                let depth = *depth;
                let node_levels = children.len().trailing_zeros() as usize;
                let new_depth = depth - node_levels;
                let child_index =
                    (self.index >> (new_depth + self.packing_depth)) & (children.len() - 1);
                self.stack.push((&children[child_index], new_depth));
                self.next()
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.length.as_usize().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl<T: Value> ExactSizeIterator for Iter<'_, T> {}
