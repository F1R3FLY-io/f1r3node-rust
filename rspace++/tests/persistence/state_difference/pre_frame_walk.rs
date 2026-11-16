use std::cmp::Ordering;

use crate::rspace::persistence::canonical::{CanonicalFormatError, ObjectRefV1, checked_len};
use crate::rspace::persistence::closure::ClosureError;
use crate::rspace::persistence::manifest::{SemanticNamespace, SemanticRoots};
use crate::rspace::persistence::object::{
    CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView, DagChildrenView, EndpointValue,
};
use crate::rspace::persistence::semantic::load_reference;
use crate::rspace::persistence::store::ImmutableObjectStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StateDifferenceLimits {
    pub(crate) max_object_reads: u64,
    pub(crate) max_object_read_bytes: u64,
    pub(crate) max_worklist: u64,
    pub(crate) max_path_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StateDifferenceReport {
    pub(crate) object_reads: u64,
    pub(crate) object_read_bytes: u64,
    pub(crate) maximum_worklist: u64,
    pub(crate) maximum_path_bytes: u64,
    pub(crate) differences: u64,
}

pub(crate) fn visit_state_difference<S, F>(
    store: &S,
    from: SemanticRoots,
    to: SemanticRoots,
    limits: StateDifferenceLimits,
    visit: F,
) -> Result<StateDifferenceReport, ClosureError<S::Error>>
where
    S: ImmutableObjectStore,
    F: FnMut(
        SemanticNamespace,
        &[u8],
        EndpointValue,
        EndpointValue,
    ) -> Result<(), ClosureError<S::Error>>,
{
    if from == to {
        return Ok(StateDifferenceReport::default());
    }
    let mut walk = DifferenceWalk::reserve(store, limits, visit)?;
    for ((namespace, from_root), (_, to_root)) in from.iter().into_iter().zip(to.iter()) {
        if from_root != to_root {
            walk.compare_namespace(namespace, from_root, to_root)?;
        }
    }
    Ok(walk.report)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StateSide {
    From,
    To,
}

enum PendingSubtree<O> {
    Pair {
        from: ObjectRefV1,
        to: ObjectRefV1,
    },
    Whole {
        side: StateSide,
        reference: ObjectRefV1,
    },
    Remainder {
        side: StateSide,
        reference: ObjectRefV1,
        object: O,
        offset: usize,
    },
}

struct PendingWork<O> {
    subtree: PendingSubtree<O>,
    parent_path_len: usize,
    edge: Option<u8>,
}

struct DifferenceWalk<'store, S, F>
where S: ImmutableObjectStore
{
    store: &'store S,
    limits: StateDifferenceLimits,
    visit: F,
    work: Vec<PendingWork<S::Object>>,
    path: Vec<u8>,
    report: StateDifferenceReport,
}

impl<'store, S, F> DifferenceWalk<'store, S, F>
where
    S: ImmutableObjectStore,
    F: FnMut(
        SemanticNamespace,
        &[u8],
        EndpointValue,
        EndpointValue,
    ) -> Result<(), ClosureError<S::Error>>,
{
    fn reserve(
        store: &'store S,
        limits: StateDifferenceLimits,
        visit: F,
    ) -> Result<Self, ClosureError<S::Error>> {
        let mut work = Vec::new();
        work.try_reserve_exact(capacity(limits.max_worklist, "state difference worklist")?)
            .map_err(|_| {
                ClosureError::Format(CanonicalFormatError::AllocationFailure {
                    requested: limits.max_worklist,
                })
            })?;
        let mut path = Vec::new();
        path.try_reserve_exact(capacity(limits.max_path_bytes, "state difference path bytes")?)
            .map_err(|_| {
                ClosureError::Format(CanonicalFormatError::AllocationFailure {
                    requested: limits.max_path_bytes,
                })
            })?;
        Ok(Self {
            store,
            limits,
            visit,
            work,
            path,
            report: StateDifferenceReport::default(),
        })
    }

    fn compare_namespace(
        &mut self,
        namespace: SemanticNamespace,
        from_root: ObjectRefV1,
        to_root: ObjectRefV1,
    ) -> Result<(), ClosureError<S::Error>> {
        self.push(
            PendingSubtree::Pair {
                from: from_root,
                to: to_root,
            },
            0,
            None,
        )?;
        while let Some(pending) = self.work.pop() {
            self.path.truncate(pending.parent_path_len);
            if let Some(edge) = pending.edge {
                self.append(&[edge])?;
            }
            match pending.subtree {
                PendingSubtree::Pair { from, to } => self.align(namespace, from, to)?,
                PendingSubtree::Whole { side, reference } => {
                    let object = self.read(reference)?;
                    let node = parse_node(object.as_ref(), reference)?;
                    self.one_sided(namespace, side, node, 0)?;
                }
                PendingSubtree::Remainder {
                    side,
                    reference,
                    object,
                    offset,
                } => {
                    let node = parse_node(object.as_ref(), reference)?;
                    self.one_sided(namespace, side, node, offset)?;
                }
            }
        }
        Ok(())
    }

    fn align(
        &mut self,
        namespace: SemanticNamespace,
        from: ObjectRefV1,
        to: ObjectRefV1,
    ) -> Result<(), ClosureError<S::Error>> {
        let mut from_reference = from;
        let mut from_object = self.read(from_reference)?;
        let mut from_node = parse_node(from_object.as_ref(), from_reference)?;
        let mut from_offset = 0usize;
        let mut to_reference = to;
        let mut to_object = self.read(to_reference)?;
        let mut to_node = parse_node(to_object.as_ref(), to_reference)?;
        let mut to_offset = 0usize;
        loop {
            let from_rest = &from_node.prefix()[from_offset..];
            let to_rest = &to_node.prefix()[to_offset..];
            let common = common_prefix_len(from_rest, to_rest);
            self.append(&from_rest[..common])?;
            match (from_rest.get(common).copied(), to_rest.get(common).copied()) {
                (None, None) => {
                    self.compare(namespace, from_node.endpoint(), to_node.endpoint())?;
                    return self.merge_children(from_node.children(), to_node.children());
                }
                (None, Some(next)) => {
                    self.compare(namespace, from_node.endpoint(), EndpointValue::Absent)?;
                    let Some(continued) =
                        self.push_children_except(StateSide::From, from_node.children(), next)?
                    else {
                        return self.one_sided(
                            namespace,
                            StateSide::To,
                            to_node,
                            to_offset + common,
                        );
                    };
                    self.append(&[next])?;
                    to_offset += common + 1;
                    from_reference = continued;
                    from_object = self.read(from_reference)?;
                    from_node = parse_node(from_object.as_ref(), from_reference)?;
                    from_offset = 0;
                }
                (Some(next), None) => {
                    self.compare(namespace, EndpointValue::Absent, to_node.endpoint())?;
                    let Some(continued) =
                        self.push_children_except(StateSide::To, to_node.children(), next)?
                    else {
                        return self.one_sided(
                            namespace,
                            StateSide::From,
                            from_node,
                            from_offset + common,
                        );
                    };
                    self.append(&[next])?;
                    from_offset += common + 1;
                    to_reference = continued;
                    to_object = self.read(to_reference)?;
                    to_node = parse_node(to_object.as_ref(), to_reference)?;
                    to_offset = 0;
                }
                (Some(_), Some(_)) => {
                    let parent_path_len = self.path.len();
                    self.push(
                        PendingSubtree::Remainder {
                            side: StateSide::From,
                            reference: from_reference,
                            object: from_object,
                            offset: from_offset + common,
                        },
                        parent_path_len,
                        None,
                    )?;
                    return self.one_sided(namespace, StateSide::To, to_node, to_offset + common);
                }
            }
        }
    }

    fn merge_children(
        &mut self,
        from: DagChildrenView<'_>,
        to: DagChildrenView<'_>,
    ) -> Result<(), ClosureError<S::Error>> {
        let parent_path_len = self.path.len();
        let mut from_children = from.iter();
        let mut to_children = to.iter();
        let mut from_child = from_children.next();
        let mut to_child = to_children.next();
        loop {
            match (from_child, to_child) {
                (None, None) => return Ok(()),
                (Some(child), None) => {
                    self.push_whole(StateSide::From, child.target, parent_path_len, child.edge)?;
                    from_child = from_children.next();
                }
                (None, Some(child)) => {
                    self.push_whole(StateSide::To, child.target, parent_path_len, child.edge)?;
                    to_child = to_children.next();
                }
                (Some(from_entry), Some(to_entry)) => match from_entry.edge.cmp(&to_entry.edge) {
                    Ordering::Less => {
                        self.push_whole(
                            StateSide::From,
                            from_entry.target,
                            parent_path_len,
                            from_entry.edge,
                        )?;
                        from_child = from_children.next();
                    }
                    Ordering::Greater => {
                        self.push_whole(
                            StateSide::To,
                            to_entry.target,
                            parent_path_len,
                            to_entry.edge,
                        )?;
                        to_child = to_children.next();
                    }
                    Ordering::Equal => {
                        if from_entry.target != to_entry.target {
                            self.push(
                                PendingSubtree::Pair {
                                    from: from_entry.target,
                                    to: to_entry.target,
                                },
                                parent_path_len,
                                Some(from_entry.edge),
                            )?;
                        }
                        from_child = from_children.next();
                        to_child = to_children.next();
                    }
                },
            }
        }
    }

    fn push_children_except(
        &mut self,
        side: StateSide,
        children: DagChildrenView<'_>,
        continuation: u8,
    ) -> Result<Option<ObjectRefV1>, ClosureError<S::Error>> {
        let parent_path_len = self.path.len();
        let mut continued = None;
        for child in children.iter() {
            if child.edge == continuation {
                continued = Some(child.target);
            } else {
                self.push_whole(side, child.target, parent_path_len, child.edge)?;
            }
        }
        Ok(continued)
    }

    fn one_sided(
        &mut self,
        namespace: SemanticNamespace,
        side: StateSide,
        node: CanonicalDagNodeView<'_>,
        offset: usize,
    ) -> Result<(), ClosureError<S::Error>> {
        self.append(&node.prefix()[offset..])?;
        match side {
            StateSide::From => self.compare(namespace, node.endpoint(), EndpointValue::Absent)?,
            StateSide::To => self.compare(namespace, EndpointValue::Absent, node.endpoint())?,
        }
        let parent_path_len = self.path.len();
        for child in node.children().iter() {
            self.push_whole(side, child.target, parent_path_len, child.edge)?;
        }
        Ok(())
    }

    fn compare(
        &mut self,
        namespace: SemanticNamespace,
        from_value: EndpointValue,
        to_value: EndpointValue,
    ) -> Result<(), ClosureError<S::Error>> {
        if from_value == to_value {
            return Ok(());
        }
        self.report.differences =
            self.report
                .differences
                .checked_add(1)
                .ok_or(ClosureError::Format(CanonicalFormatError::CounterOverflow(
                    "state difference endpoints",
                )))?;
        (self.visit)(namespace, &self.path, from_value, to_value)
    }

    fn read(&mut self, reference: ObjectRefV1) -> Result<S::Object, ClosureError<S::Error>> {
        let reads = self
            .report
            .object_reads
            .checked_add(1)
            .ok_or(ClosureError::Format(CanonicalFormatError::CounterOverflow(
                "state difference object reads",
            )))?;
        check_limit("state difference object reads", self.limits.max_object_reads, reads)?;
        let read_bytes = self
            .report
            .object_read_bytes
            .checked_add(reference.encoded_len())
            .ok_or(ClosureError::Format(CanonicalFormatError::CounterOverflow(
                "state difference object read bytes",
            )))?;
        check_limit(
            "state difference object read bytes",
            self.limits.max_object_read_bytes,
            read_bytes,
        )?;
        self.report.object_reads = reads;
        self.report.object_read_bytes = read_bytes;
        load_reference(self.store, reference)
    }

    fn push_whole(
        &mut self,
        side: StateSide,
        reference: ObjectRefV1,
        parent_path_len: usize,
        edge: u8,
    ) -> Result<(), ClosureError<S::Error>> {
        self.push(PendingSubtree::Whole { side, reference }, parent_path_len, Some(edge))
    }

    fn push(
        &mut self,
        subtree: PendingSubtree<S::Object>,
        parent_path_len: usize,
        edge: Option<u8>,
    ) -> Result<(), ClosureError<S::Error>> {
        let attempted = checked_len(self.work.len(), "state difference worklist")
            .map_err(ClosureError::Format)?
            .checked_add(1)
            .ok_or(ClosureError::Format(CanonicalFormatError::CounterOverflow(
                "state difference worklist",
            )))?;
        check_limit("state difference worklist", self.limits.max_worklist, attempted)?;
        self.work.push(PendingWork {
            subtree,
            parent_path_len,
            edge,
        });
        self.report.maximum_worklist = self.report.maximum_worklist.max(attempted);
        Ok(())
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), ClosureError<S::Error>> {
        let attempted = self
            .path
            .len()
            .checked_add(bytes.len())
            .ok_or(ClosureError::Format(CanonicalFormatError::CounterOverflow(
                "state difference path bytes",
            )))?;
        let attempted =
            checked_len(attempted, "state difference path bytes").map_err(ClosureError::Format)?;
        check_limit("state difference path bytes", self.limits.max_path_bytes, attempted)?;
        self.path.extend_from_slice(bytes);
        self.report.maximum_path_bytes = self.report.maximum_path_bytes.max(attempted);
        Ok(())
    }
}

fn parse_node<E>(
    bytes: &[u8],
    reference: ObjectRefV1,
) -> Result<CanonicalDagNodeView<'_>, ClosureError<E>> {
    match CanonicalObjectView::parse(bytes).map_err(ClosureError::Format)? {
        CanonicalObjectView::DagNode(node) => Ok(node),
        object => Err(ClosureError::Format(CanonicalFormatError::UnexpectedObjectKind {
            digest: reference.digest(),
            expected: CanonicalObjectKind::DagNode.name(),
            actual: object.kind().name(),
        })),
    }
}

fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left_byte, right_byte)| left_byte == right_byte)
        .count()
}

fn capacity<E>(limit: u64, field: &'static str) -> Result<usize, ClosureError<E>> {
    usize::try_from(limit).map_err(|_| {
        ClosureError::Format(CanonicalFormatError::LengthOutOfRange {
            field,
            value: limit,
        })
    })
}

fn check_limit<E>(
    resource: &'static str,
    limit: u64,
    attempted: u64,
) -> Result<(), ClosureError<E>> {
    if attempted > limit {
        return Err(ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        }));
    }
    Ok(())
}
