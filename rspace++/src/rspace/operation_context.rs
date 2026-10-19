use std::cmp::Ordering;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

pub type PathSegment = (u64, u64);

struct PathNode {
    parent: Option<Arc<PathNode>>,
    ancestors: Vec<Arc<PathNode>>,
    segment: Option<PathSegment>,
    depth: usize,
}

#[derive(Clone)]
pub struct CausalPath {
    tail: Arc<PathNode>,
}

impl CausalPath {
    pub fn new() -> Self {
        Self {
            tail: Arc::new(PathNode {
                parent: None,
                ancestors: Vec::new(),
                segment: None,
                depth: 0,
            }),
        }
    }

    pub fn push_back(&mut self, segment: PathSegment) {
        let parent = self.tail.clone();
        let mut ancestors = vec![parent.clone()];
        loop {
            let level = ancestors.len() - 1;
            let next = ancestors[level].ancestors.get(level).cloned();
            match next {
                Some(next) => ancestors.push(next),
                None => break,
            }
        }
        self.tail = Arc::new(PathNode {
            parent: Some(parent.clone()),
            ancestors,
            segment: Some(segment),
            depth: parent.depth + 1,
        });
    }

    pub fn to_vec(&self) -> Vec<PathSegment> {
        let mut path = Vec::with_capacity(self.tail.depth);
        let mut node = Some(self.tail.as_ref());
        while let Some(current) = node {
            if let Some(segment) = current.segment {
                path.push(segment);
            }
            node = current.parent.as_deref();
        }
        path.reverse();
        path
    }

    fn lift(mut node: Arc<PathNode>, mut steps: usize) -> Arc<PathNode> {
        let mut level = 0;
        while steps != 0 {
            if steps & 1 == 1 {
                node = node.ancestors[level].clone();
            }
            steps >>= 1;
            level += 1;
        }
        node
    }

    fn shared_root(&self, other: &Self) -> bool {
        let left_root = Self::lift(self.tail.clone(), self.tail.depth);
        let right_root = Self::lift(other.tail.clone(), other.tail.depth);
        Arc::ptr_eq(&left_root, &right_root)
    }

    fn shared_cmp(&self, other: &Self) -> Ordering {
        let common_depth = self.tail.depth.min(other.tail.depth);
        let mut left = Self::lift(self.tail.clone(), self.tail.depth - common_depth);
        let mut right = Self::lift(other.tail.clone(), other.tail.depth - common_depth);
        if Arc::ptr_eq(&left, &right) {
            return self.tail.depth.cmp(&other.tail.depth);
        }

        let levels = left.ancestors.len().min(right.ancestors.len());
        for level in (0..levels).rev() {
            let left_ancestor = left.ancestors.get(level).cloned();
            let right_ancestor = right.ancestors.get(level).cloned();
            if let (Some(left_ancestor), Some(right_ancestor)) = (left_ancestor, right_ancestor) {
                if !Arc::ptr_eq(&left_ancestor, &right_ancestor) {
                    left = left_ancestor;
                    right = right_ancestor;
                }
            }
        }

        match left.segment.cmp(&right.segment) {
            Ordering::Equal => self.to_vec().cmp(&other.to_vec()),
            ordering => ordering,
        }
    }
}

impl Default for CausalPath {
    fn default() -> Self { Self::new() }
}

impl From<Vec<PathSegment>> for CausalPath {
    fn from(segments: Vec<PathSegment>) -> Self {
        let mut path = Self::new();
        for segment in segments {
            path.push_back(segment);
        }
        path
    }
}

impl fmt::Debug for CausalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.to_vec()).finish()
    }
}

impl PartialEq for CausalPath {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == Ordering::Equal }
}

impl Eq for CausalPath {}

impl PartialOrd for CausalPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for CausalPath {
    fn cmp(&self, other: &Self) -> Ordering {
        if Arc::ptr_eq(&self.tail, &other.tail) {
            return Ordering::Equal;
        }
        if self.shared_root(other) {
            self.shared_cmp(other)
        } else {
            self.to_vec().cmp(&other.to_vec())
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OperationOrder {
    pub session: [u8; 32],
    pub path: CausalPath,
}

tokio::task_local! {
    static OPERATION_ORDER: OperationOrder;
}

pub async fn scope<T>(order: OperationOrder, future: impl Future<Output = T>) -> T {
    OPERATION_ORDER.scope(order, future).await
}

pub fn current() -> Option<OperationOrder> { OPERATION_ORDER.try_with(Clone::clone).ok() }

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use proptest::prelude::*;

    use super::CausalPath;

    fn comparison_sign(ordering: Ordering) -> i8 {
        match ordering {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    proptest! {
        #[test]
        fn persistent_paths_preserve_vec_order(
            left in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
            right in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
        ) {
            let persistent_left = CausalPath::from(left.clone());
            let persistent_right = CausalPath::from(right.clone());

            prop_assert_eq!(
                comparison_sign(left.cmp(&right)),
                comparison_sign(persistent_left.cmp(&persistent_right)),
            );
            prop_assert_eq!(left == right, persistent_left == persistent_right);
        }

        #[test]
        fn persistent_path_append_preserves_vec_projection(
            prefix in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
            component in (any::<u64>(), any::<u64>()),
        ) {
            let mut expected = prefix.clone();
            expected.push(component);
            let mut persistent = CausalPath::from(prefix);
            persistent.push_back(component);

            prop_assert_eq!(persistent.to_vec(), expected);
        }

        #[test]
        fn shared_prefix_paths_preserve_vec_order(
            prefix in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
            left_suffix in prop::collection::vec((any::<u64>(), any::<u64>()), 0..64),
            right_suffix in prop::collection::vec((any::<u64>(), any::<u64>()), 0..64),
        ) {
            let mut persistent_prefix = CausalPath::from(prefix.clone());
            let mut persistent_left = persistent_prefix.clone();
            let mut persistent_right = persistent_prefix.clone();
            let mut left = prefix.clone();
            let mut right = prefix;

            for segment in left_suffix {
                left.push(segment);
                persistent_left.push_back(segment);
            }
            for segment in right_suffix {
                right.push(segment);
                persistent_right.push_back(segment);
            }

            prop_assert!(persistent_left.shared_root(&persistent_right));

            prop_assert_eq!(
                comparison_sign(left.cmp(&right)),
                comparison_sign(persistent_left.cmp(&persistent_right)),
            );
            prop_assert_eq!(left == right, persistent_left == persistent_right);
            persistent_prefix.push_back((u64::MAX, u64::MAX));
            let extended_prefix = persistent_prefix.to_vec();
            prop_assert_eq!(extended_prefix.last(), Some(&(u64::MAX, u64::MAX)));
        }


        #[test]
        fn causal_path_sort_preserves_vec_sort(
            paths in prop::collection::vec(
                prop::collection::vec((any::<u64>(), any::<u64>()), 0..64),
                0..64,
            ),
        ) {
            let mut expected = paths.clone();
            expected.sort();
            let mut causal = paths
                .into_iter()
                .map(CausalPath::from)
                .collect::<Vec<_>>();
            causal.sort();

            prop_assert_eq!(
                causal.into_iter().map(|path| path.to_vec()).collect::<Vec<_>>(),
                expected,
            );
        }
    }
}
