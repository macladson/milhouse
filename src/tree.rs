use crate::utils::{Length, opt_hash};
use crate::{Arc, Error, Leaf, PackedLeaf, UpdateMap, Value};
use educe::Educe;
use ethereum_hashing::{ZERO_HASHES, hash32_concat};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::ops::ControlFlow;
use tree_hash::Hash256;

#[derive(Debug, Educe)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[educe(PartialEq(bound(T: Value)), Hash)]
pub enum Tree<T: Value> {
    Leaf(Leaf<T>),
    PackedLeaf(PackedLeaf<T>),
    /// An intermediate node with `2^k` children (k >= 1).
    ///
    /// When k=1, this is a standard binary node (2 children).
    /// When k>1, this is a wide node that absorbs multiple binary tree levels.
    /// The number of children is always a power of 2.
    Node {
        #[educe(PartialEq(ignore), Hash(ignore))]
        #[cfg_attr(feature = "arbitrary", arbitrary(with = crate::utils::arb_rwlock))]
        hash: RwLock<Hash256>,
        #[cfg_attr(feature = "arbitrary", arbitrary(with = crate::utils::arb_node_children))]
        children: Box<[Arc<Self>]>,
    },
    /// A subtree of all zeros. The `usize` is the depth of the zero subtree.
    Zero(usize),
}

impl<T: Value> Clone for Tree<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Leaf(leaf) => Self::Leaf(leaf.clone()),
            Self::PackedLeaf(leaf) => Self::PackedLeaf(leaf.clone()),
            Self::Node { hash, children } => Self::Node {
                hash: RwLock::new(*hash.read()),
                children: children.clone(),
            },
            Self::Zero(depth) => Self::Zero(*depth),
        }
    }
}

impl<T: Value> Tree<T> {
    pub fn empty(depth: usize) -> Arc<Self> {
        Self::zero(depth)
    }

    /// Create an intermediate node from a boxed slice of children.
    pub fn node(children: Box<[Arc<Self>]>, hash: Hash256) -> Arc<Self> {
        Arc::new(Self::Node {
            hash: RwLock::new(hash),
            children,
        })
    }

    /// Create a binary (2-child) intermediate node.
    pub fn binary_node(left: Arc<Self>, right: Arc<Self>, hash: Hash256) -> Arc<Self> {
        Self::node(Box::new([left, right]), hash)
    }

    pub fn zero(depth: usize) -> Arc<Self> {
        Arc::new(Self::Zero(depth))
    }

    /// Create a zero node at a position where a `PackedLeaf` would appear.
    pub fn packed_zero(logical_depth: usize, packed_leaf_depth: usize) -> Arc<Self> {
        Arc::new(Self::Zero(logical_depth + packed_leaf_depth))
    }

    pub fn leaf(value: T) -> Arc<Self> {
        Arc::new(Self::Leaf(Leaf::new(value)))
    }

    pub fn leaf_with_hash(value: T, hash: Hash256) -> Arc<Self> {
        Arc::new(Self::Leaf(Leaf::with_hash(value, hash)))
    }

    /// Create an unboxed binary node (for builder stack usage).
    pub fn node_unboxed(left: Arc<Self>, right: Arc<Self>) -> Self {
        Self::Node {
            hash: RwLock::new(Hash256::ZERO),
            children: Box::new([left, right]),
        }
    }

    pub fn zero_unboxed(depth: usize) -> Self {
        Self::Zero(depth)
    }

    pub fn leaf_unboxed(value: T) -> Self {
        Self::Leaf(Leaf::new(value))
    }

    pub fn get_recursive(&self, index: usize, depth: usize, packing_depth: usize) -> Option<&T> {
        match self {
            Self::Leaf(Leaf { value, .. }) if depth == 0 => Some(value),
            Self::PackedLeaf(PackedLeaf { values, .. }) if depth == 0 => {
                values.get(index % (1 << packing_depth))
            }
            Self::Node { children, .. } if depth > 0 => {
                let node_levels = children.len().trailing_zeros() as usize;
                let new_depth = depth - node_levels;
                let child_index = (index >> (new_depth + packing_depth)) & (children.len() - 1);
                children[child_index].get_recursive(index, new_depth, packing_depth)
            }
            _ => None,
        }
    }

    /// Create a new tree where the `index`th leaf is set to `new_value`.
    ///
    /// `branching_factor` controls the width of nodes created when splitting Zero nodes.
    /// It is the desired number of children (must be a power of two, at least 2).
    /// The width at any depth `d` is `min(branching_factor, 2^d)`.
    pub fn with_updated_leaf(
        &self,
        index: usize,
        new_value: T,
        depth: usize,
        packing_depth: usize,
        packed_leaf_depth: usize,
        branching_factor: usize,
    ) -> Result<Arc<Self>, Error> {
        let bf_log2 = branching_factor.trailing_zeros() as usize;
        match self {
            Self::Leaf(_) if depth == 0 => Ok(Self::leaf(new_value)),
            Self::PackedLeaf(leaf) if depth == 0 => Ok(Arc::new(Self::PackedLeaf(
                leaf.insert_at_index(index, new_value, packed_leaf_depth)?,
            ))),
            Self::Node { children, .. } if depth > 0 => {
                let node_levels = children.len().trailing_zeros() as usize;
                let new_depth = depth - node_levels;
                let child_index = (index >> (new_depth + packing_depth)) & (children.len() - 1);
                let mut new_children = children.clone();
                new_children[child_index] = children[child_index].with_updated_leaf(
                    index,
                    new_value,
                    new_depth,
                    packing_depth,
                    packed_leaf_depth,
                    branching_factor,
                )?;
                Ok(Self::node(new_children, Hash256::ZERO))
            }
            Self::Zero(zero_depth) if *zero_depth == depth + packed_leaf_depth => {
                if depth == 0 {
                    if packing_depth > 0 {
                        Ok(Arc::new(Self::PackedLeaf(PackedLeaf::single(
                            new_value,
                            packed_leaf_depth,
                        ))))
                    } else {
                        Ok(Self::leaf(new_value))
                    }
                } else {
                    // Split zero node into a node of the correct width.
                    // Width = min(branching_factor, 2^depth).
                    let width_exp = std::cmp::min(bf_log2, depth);
                    let child_zero_depth = (depth - width_exp) + packed_leaf_depth;
                    let width = 1usize << width_exp;
                    let children: Box<[Arc<Self>]> =
                        (0..width).map(|_| Self::zero(child_zero_depth)).collect();
                    let node = Arc::new(Self::Node {
                        hash: RwLock::new(Hash256::ZERO),
                        children,
                    });
                    node.with_updated_leaf(
                        index,
                        new_value,
                        depth,
                        packing_depth,
                        packed_leaf_depth,
                        branching_factor,
                    )
                }
            }
            _ => Err(Error::UpdateLeafError),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_updated_leaves<U: UpdateMap<T>>(
        &self,
        updates: &U,
        prefix: usize,
        depth: usize,
        hashes: Option<&BTreeMap<(usize, usize), Hash256>>,
        packing_depth: usize,
        packed_leaf_depth: usize,
        branching_factor: usize,
    ) -> Result<Arc<Self>, Error> {
        let bf_log2 = branching_factor.trailing_zeros() as usize;
        let hash = opt_hash(hashes, depth, prefix).unwrap_or_default();

        match self {
            Self::Leaf(_) if depth == 0 => {
                let index = prefix;
                let value = updates
                    .get(index)
                    .cloned()
                    .ok_or(Error::LeafUpdateMissing { index })?;
                Ok(Self::leaf_with_hash(value, hash))
            }
            Self::PackedLeaf(packed_leaf) if depth == 0 => Ok(Arc::new(Self::PackedLeaf(
                packed_leaf.update(prefix, hash, updates, packed_leaf_depth)?,
            ))),
            Self::Node { children, .. } if depth > 0 => {
                let node_levels = children.len().trailing_zeros() as usize;
                let num_children = children.len();
                let new_depth = depth - node_levels;

                let mut new_children = children.clone();
                let mut any_updated = false;

                for i in 0..num_children {
                    let child_prefix = prefix | (i << (new_depth + packing_depth));
                    let child_end = child_prefix + (1 << (new_depth + packing_depth));

                    let mut has_updates = false;
                    updates.for_each_range(child_prefix, child_end, |_, _| {
                        has_updates = true;
                        ControlFlow::Break(())
                    })?;

                    if has_updates {
                        new_children[i] = children[i].with_updated_leaves(
                            updates,
                            child_prefix,
                            new_depth,
                            hashes,
                            packing_depth,
                            packed_leaf_depth,
                            branching_factor,
                        )?;
                        any_updated = true;
                    }
                }

                if !any_updated {
                    return Err(Error::NodeUpdatesMissing { prefix });
                }

                Ok(Self::node(new_children, hash))
            }
            Self::Zero(zero_depth) if *zero_depth == depth + packed_leaf_depth => {
                if depth == 0 {
                    if packing_depth > 0 {
                        let packed_leaf = PackedLeaf::empty(packed_leaf_depth).update(
                            prefix,
                            hash,
                            updates,
                            packed_leaf_depth,
                        )?;
                        Ok(Arc::new(Self::PackedLeaf(packed_leaf)))
                    } else {
                        let index = prefix;
                        let value = updates
                            .get(index)
                            .cloned()
                            .ok_or(Error::LeafUpdateMissing { index })?;
                        Ok(Self::leaf_with_hash(value, hash))
                    }
                } else {
                    // Split zero into a node of the correct width and recurse.
                    let width_exp = std::cmp::min(bf_log2, depth);
                    let child_zero_depth = (depth - width_exp) + packed_leaf_depth;
                    let width = 1usize << width_exp;
                    let children: Box<[Arc<Self>]> =
                        (0..width).map(|_| Self::zero(child_zero_depth)).collect();
                    let node = Self::Node {
                        hash: RwLock::new(Hash256::ZERO),
                        children,
                    };
                    node.with_updated_leaves(
                        updates,
                        prefix,
                        depth,
                        hashes,
                        packing_depth,
                        packed_leaf_depth,
                        branching_factor,
                    )
                }
            }
            _ => Err(Error::UpdateLeavesError),
        }
    }

    /// Compute the number of elements stored in this subtree.
    ///
    /// This method should be avoided if possible. Prefer to read the length cached in a `List` or
    /// similar.
    pub fn compute_len(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::PackedLeaf(leaf) => leaf.values.len(),
            Self::Node { children, .. } => children.iter().map(|c| c.compute_len()).sum(),
            Self::Zero(_) => 0,
        }
    }
}

pub enum RebaseAction<'a, T> {
    /// Nodes are not equal: no replacement needed.
    NotEqualNoop,
    /// Nodes are not equal: replacement with a new constructed node.
    NotEqualReplace(Arc<T>),
    /// Nodes are equal: no replacement needed (already pointer-equal or structurally identical).
    EqualNoop,
    /// Nodes are equal: replacement with the base node (for structural sharing).
    EqualReplace(&'a Arc<T>),
}

pub enum IntraRebaseAction<T> {
    Noop,
    Replace(Arc<T>),
}

impl<T: Value> Tree<T> {
    pub fn rebase_on<'a>(
        orig: &'a Arc<Self>,
        base: &'a Arc<Self>,
        lengths: Option<(Length, Length)>,
        full_depth: usize,
    ) -> Result<RebaseAction<'a, Self>, Error> {
        if Arc::ptr_eq(orig, base) {
            return Ok(RebaseAction::EqualNoop);
        }
        match (&**orig, &**base) {
            (Self::Leaf(l1), Self::Leaf(l2)) => {
                if l1.value == l2.value {
                    Ok(RebaseAction::EqualReplace(base))
                } else {
                    Ok(RebaseAction::NotEqualNoop)
                }
            }
            (Self::PackedLeaf(l1), Self::PackedLeaf(l2)) => {
                if l1 == l2 {
                    Ok(RebaseAction::EqualReplace(base))
                } else {
                    Ok(RebaseAction::NotEqualNoop)
                }
            }
            (Self::Zero(z1), Self::Zero(z2)) if z1 == z2 => Ok(RebaseAction::EqualReplace(base)),
            (
                Self::Node {
                    hash: orig_hash_lock,
                    children: c1,
                },
                Self::Node {
                    hash: base_hash_lock,
                    children: c2,
                },
            ) if c1.len() == c2.len() && full_depth > 0 => {
                use RebaseAction::*;

                let orig_hash = *orig_hash_lock.read();
                let base_hash = *base_hash_lock.read();

                // If hashes *and* lengths are equal then we can short-cut the recursion
                // and immediately replace `orig` by the `base` node. If `lengths` are `None`
                // then we know they are already equal (e.g. we're in a vector).
                if !orig_hash.is_zero()
                    && orig_hash == base_hash
                    && lengths.is_none_or(|(orig_length, base_length)| orig_length == base_length)
                {
                    return Ok(EqualReplace(base));
                }

                let node_levels = c1.len().trailing_zeros() as usize;
                let new_full_depth = full_depth - node_levels;

                let mut any_replaced = false;
                let mut all_equal = true;
                let mut new_children = c1.clone();

                for i in 0..c1.len() {
                    let child_lengths = lengths.map(|(orig_length, base_length)| {
                        let child_capacity = 1usize << new_full_depth;
                        let child_offset = i * child_capacity;
                        let orig_child_len = Length(
                            orig_length
                                .as_usize()
                                .saturating_sub(child_offset)
                                .min(child_capacity),
                        );
                        let base_child_len = Length(
                            base_length
                                .as_usize()
                                .saturating_sub(child_offset)
                                .min(child_capacity),
                        );
                        (orig_child_len, base_child_len)
                    });

                    match Tree::rebase_on(&c1[i], &c2[i], child_lengths, new_full_depth)? {
                        EqualReplace(r) => {
                            new_children[i] = r.clone();
                            any_replaced = true;
                        }
                        NotEqualReplace(r) => {
                            new_children[i] = r;
                            any_replaced = true;
                            all_equal = false;
                        }
                        EqualNoop => {}
                        NotEqualNoop => {
                            all_equal = false;
                        }
                    }
                }

                if all_equal && !any_replaced {
                    Ok(EqualNoop)
                } else if all_equal {
                    Ok(EqualReplace(base))
                } else if any_replaced {
                    Ok(NotEqualReplace(Arc::new(Self::Node {
                        hash: RwLock::new(orig_hash),
                        children: new_children,
                    })))
                } else {
                    Ok(NotEqualNoop)
                }
            }
            (Self::Zero(_), _) | (_, Self::Zero(_)) => Ok(RebaseAction::NotEqualNoop),
            // Mismatched node widths or depth=0 nodes
            (Self::Node { .. }, Self::Node { .. }) => Err(Error::InvalidRebaseNode),
            (Self::Leaf(_) | Self::PackedLeaf(_), _) | (_, Self::Leaf(_) | Self::PackedLeaf(_)) => {
                Err(Error::InvalidRebaseLeaf)
            }
        }
    }

    /// Exploit structural sharing between identical parts of the tree.
    ///
    /// This method traverses a fully-hashed tree and replaces identical subtrees with clones of
    /// the first equal subtree. The result is a tree that shares memory for common subtrees, and
    /// thus uses less memory overall.
    ///
    /// You MUST pass a fully-hashed tree to this function, or an `Error::IntraRebaseZeroHash`
    /// error will be returned.
    ///
    /// Arguments are:
    ///
    /// - `orig`: The tree to rebase.
    /// - `known_subtrees`: map from `(depth, tree_hash_root)` to `Arc<Node>`. This should be empty
    ///   for the top-level call. The recursive calls fill it in. It can be discarded after the
    ///   method returns.
    /// - `current_depth`: The depth of the tree `orig`. This will be decremented as we recurse
    ///   down the tree towards the leaves.
    ///
    /// Presently leaves are left untouched by this procedure, so it will only produce savings in
    /// trees with equal internal nodes (i.e. equal subtrees with at least two leaves/packed leaves
    /// under them).
    ///
    /// The input tree must be fully-hashed, and the result will also remain fully-hashed.
    pub fn intra_rebase(
        orig: &Arc<Self>,
        known_subtrees: &mut HashMap<(usize, Hash256), Arc<Self>>,
        current_depth: usize,
    ) -> Result<IntraRebaseAction<Self>, Error> {
        match &**orig {
            Self::Leaf(_) | Self::PackedLeaf(_) | Self::Zero(_) => Ok(IntraRebaseAction::Noop),
            Self::Node { hash, children } if current_depth > 0 => {
                let hash_val = *hash.read();

                // Tree must be fully hashed prior to intra-rebase.
                if hash_val.is_zero() {
                    return Err(Error::IntraRebaseZeroHash);
                }

                if let Some(known) = known_subtrees.get(&(current_depth, hash_val)) {
                    return Ok(IntraRebaseAction::Replace(known.clone()));
                }

                let node_levels = children.len().trailing_zeros() as usize;
                let child_depth = current_depth - node_levels;

                let mut any_replaced = false;
                let mut new_children = children.clone();

                for i in 0..children.len() {
                    match Self::intra_rebase(&children[i], known_subtrees, child_depth)? {
                        IntraRebaseAction::Noop => {}
                        IntraRebaseAction::Replace(new_child) => {
                            new_children[i] = new_child;
                            any_replaced = true;
                        }
                    }
                }

                let action = if any_replaced {
                    IntraRebaseAction::Replace(Self::node(new_children, hash_val))
                } else {
                    IntraRebaseAction::Noop
                };

                let new_subtree = match &action {
                    IntraRebaseAction::Noop => orig.clone(),
                    IntraRebaseAction::Replace(new) => new.clone(),
                };

                if known_subtrees
                    .insert((current_depth, hash_val), new_subtree)
                    .is_some()
                {
                    return Err(Error::IntraRebaseRepeatVisit);
                }

                Ok(action)
            }
            Self::Node { .. } => Err(Error::IntraRebaseZeroDepth),
        }
    }
}

impl<T: Value + Send + Sync> Tree<T> {
    pub(crate) fn tree_hash(&self, packed_leaf_depth: usize) -> Hash256 {
        match self {
            Self::Leaf(Leaf { hash, value }) => {
                // FIXME(sproul): upgradeable RwLock?
                let read_lock = hash.read();
                let existing_hash = *read_lock;
                drop(read_lock);

                // NOTE: We re-compute the hash whenever it is non-zero. Computed hashes may
                // legitimately be zero, but this only occurs at the leaf level when the value is
                // entirely zeroes (e.g. [0u64, 0, 0, 0]). In order to avoid storing an
                // `Option<Hash256>` we choose to re-compute the hash in this case. In practice
                // this is unlikely to provide any performance penalty except at very small list
                // lengths (<= 32), because a node higher in the tree will cache a non-zero hash
                // preventing its children from being visited more than once.
                if !existing_hash.is_zero() {
                    existing_hash
                } else {
                    let tree_hash = value.tree_hash_root();
                    *hash.write() = tree_hash;
                    tree_hash
                }
            }
            Self::PackedLeaf(leaf) => leaf.tree_hash(packed_leaf_depth),
            Self::Zero(depth) => Hash256::from(ZERO_HASHES[*depth]),
            Self::Node { hash, children } => {
                let read_lock = hash.read();
                let existing_hash = *read_lock;
                drop(read_lock);

                if !existing_hash.is_zero() {
                    return existing_hash;
                }

                let num_children = children.len();

                if num_children == 2 {
                    // Binary node: use rayon::join for top-level parallelism
                    let (left_hash, right_hash) = rayon::join(
                        || children[0].tree_hash(packed_leaf_depth),
                        || children[1].tree_hash(packed_leaf_depth),
                    );
                    let tree_hash =
                        Hash256::from(hash32_concat(left_hash.as_slice(), right_hash.as_slice()));
                    *hash.write() = tree_hash;
                    tree_hash
                } else {
                    let node_levels = num_children.trailing_zeros() as usize;

                    let mut heap_buf;
                    let mut stack_buf = [Hash256::ZERO; 16];
                    // If branching factor <= 16, keep all hashes on the stack.
                    let buf: &mut [Hash256] = if num_children <= 16 {
                        &mut stack_buf[..num_children]
                    } else {
                        heap_buf = vec![Hash256::ZERO; num_children];
                        &mut heap_buf[..]
                    };

                    if num_children >= 4 {
                        use rayon::prelude::*;
                        buf[..num_children]
                            .par_iter_mut()
                            .zip(children.par_iter())
                            .for_each(|(dest, child)| {
                                *dest = child.tree_hash(packed_leaf_depth);
                            });
                    } else {
                        for (i, child) in children.iter().enumerate() {
                            buf[i] = child.tree_hash(packed_leaf_depth);
                        }
                    }

                    let mut count = num_children;
                    for _ in 0..node_levels {
                        let pairs = count / 2;
                        for i in 0..pairs {
                            let h = hash32_concat(buf[2 * i].as_slice(), buf[2 * i + 1].as_slice());
                            buf[i] = Hash256::from(h);
                        }
                        count = pairs;
                    }

                    let tree_hash = buf[0];
                    *hash.write() = tree_hash;
                    tree_hash
                }
            }
        }
    }
}
