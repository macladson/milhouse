use crate::Arc;
use educe::Educe;
use std::sync::OnceLock;
use tree_hash::Hash256;

#[derive(Debug, Educe)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[educe(PartialEq, Hash)]
pub struct Leaf<T> {
    #[educe(PartialEq(ignore), Hash(ignore))]
    #[cfg_attr(feature = "arbitrary", arbitrary(with = crate::utils::arb_oncelock))]
    pub hash: OnceLock<Hash256>,
    #[cfg_attr(feature = "arbitrary", arbitrary(with = crate::utils::arb_arc))]
    pub value: Arc<T>,
}

impl<T> Clone for Leaf<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            hash: match self.hash.get() {
                Some(&h) => OnceLock::from(h),
                None => OnceLock::new(),
            },
            value: self.value.clone(),
        }
    }
}

impl<T> Leaf<T> {
    pub fn new(value: T) -> Self {
        Self::with_hash(value, None)
    }

    pub fn with_hash(value: T, hash: Option<Hash256>) -> Self {
        Self {
            hash: match hash {
                Some(h) => OnceLock::from(h),
                None => OnceLock::new(),
            },
            value: Arc::new(value),
        }
    }
}
