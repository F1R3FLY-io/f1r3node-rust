use std::cmp::Ordering;

use super::canonical::{CanonicalFormatError, ObjectRefV1, checked_len};
use super::closure::ClosureError;
use super::manifest::{SemanticNamespace, SemanticRoots};
use super::object::{
    CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView, DagChild, DagChildPosition,
    DagChildrenLayout, DagChildrenView, EndpointValue,
};
use super::semantic::load_reference;
use super::store::ImmutableObjectStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StateDifferenceLimits {
    pub(crate) max_object_reads: u64,
    pub(crate) max_object_read_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StateDifferenceReport {
    pub(crate) object_reads: u64,
    pub(crate) object_read_bytes: u64,
    pub(crate) reserved_frames: u64,
    pub(crate) reserved_path_bytes: u64,
    pub(crate) maximum_frames: u64,
    pub(crate) maximum_path_bytes: u64,
    pub(crate) differences: u64,
}

pub(crate) fn visit_state_difference<S, F>(
    store: &S,
    from: SemanticRoots,
    to: SemanticRoots,
    longest_current_key_bytes: u64,
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
    let mut walk = DifferenceWalk::reserve(store, longest_current_key_bytes, limits, visit)?;
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

#[derive(Clone, Copy)]
enum ChildStep {
    Pair {
        from: ObjectRefV1,
        to: ObjectRefV1,
    },
    Whole {
        side: StateSide,
        reference: ObjectRefV1,
    },
}

enum Step<O> {
    Child(ChildStep),
    FromRemainder {
        node: O,
        layout: DagChildrenLayout,
        offset: usize,
    },
}

#[derive(Clone, Copy)]
struct Opening {
    layout: DagChildrenLayout,
    next: Option<DagChildPosition>,
}

impl Opening {
    fn of(node: CanonicalDagNodeView<'_>) -> Self {
        Self {
            layout: node.children_layout(),
            next: node.children().highest(),
        }
    }

    fn over<O>(self, node: O) -> Descent<O> {
        Descent {
            node,
            layout: self.layout,
            next: self.next,
        }
    }
}

struct Descent<O> {
    node: O,
    layout: DagChildrenLayout,
    next: Option<DagChildPosition>,
}

enum Frame<O> {
    Pair {
        from: Descent<O>,
        to: Descent<O>,
        path_len: usize,
    },
    OneSided {
        side: StateSide,
        children: Descent<O>,
        skipped: Option<u8>,
        path_len: usize,
    },
    FromRemainder {
        node: O,
        layout: DagChildrenLayout,
        offset: usize,
        path_len: usize,
    },
}

struct DifferenceWalk<'store, S, F>
where S: ImmutableObjectStore
{
    store: &'store S,
    limits: StateDifferenceLimits,
    visit: F,
    frames: Vec<Frame<S::Object>>,
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
        longest_current_key_bytes: u64,
        limits: StateDifferenceLimits,
        visit: F,
    ) -> Result<Self, ClosureError<S::Error>> {
        let reserved_frames = longest_current_key_bytes.saturating_add(1);
        let frames = reserved(reserved_frames)?;
        let path = reserved(longest_current_key_bytes)?;
        Ok(Self {
            store,
            limits,
            visit,
            frames,
            path,
            report: StateDifferenceReport {
                reserved_frames,
                reserved_path_bytes: longest_current_key_bytes,
                ..StateDifferenceReport::default()
            },
        })
    }

    fn compare_namespace(
        &mut self,
        namespace: SemanticNamespace,
        from_root: ObjectRefV1,
        to_root: ObjectRefV1,
    ) -> Result<(), ClosureError<S::Error>> {
        self.path.clear();
        let mut step = Step::Child(ChildStep::Pair {
            from: from_root,
            to: to_root,
        });
        loop {
            self.enter(namespace, step)?;
            match self.resume()? {
                Some(next) => step = next,
                None => return Ok(()),
            }
        }
    }

    fn enter(
        &mut self,
        namespace: SemanticNamespace,
        step: Step<S::Object>,
    ) -> Result<(), ClosureError<S::Error>> {
        match step {
            Step::Child(ChildStep::Pair { from, to }) => self.align(namespace, from, to),
            Step::Child(ChildStep::Whole { side, reference }) => {
                let object = self.read(reference)?;
                let node = parse_node(object.as_ref(), reference)?;
                let opening = self.open_one_sided(namespace, side, node, 0)?;
                self.push_one_sided(side, object, opening)
            }
            Step::FromRemainder {
                node: object,
                layout,
                offset,
            } => {
                let node = layout.node(object.as_ref()).map_err(ClosureError::Format)?;
                let opening = self.open_one_sided(namespace, StateSide::From, node, offset)?;
                self.push_one_sided(StateSide::From, object, opening)
            }
        }
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
                    let (from_opening, to_opening) = (Opening::of(from_node), Opening::of(to_node));
                    let path_len = self.path.len();
                    return self.push_frame(Frame::Pair {
                        from: from_opening.over(from_object),
                        to: to_opening.over(to_object),
                        path_len,
                    });
                }
                (None, Some(next)) => {
                    self.compare(namespace, from_node.endpoint(), EndpointValue::Absent)?;
                    let continued = from_node.children().get(next);
                    let opening = Opening::of(from_node);
                    let path_len = self.path.len();
                    self.push_frame(Frame::OneSided {
                        side: StateSide::From,
                        children: opening.over(from_object),
                        skipped: Some(next),
                        path_len,
                    })?;
                    let Some(continued) = continued else {
                        let opening = self.open_one_sided(
                            namespace,
                            StateSide::To,
                            to_node,
                            to_offset + common,
                        )?;
                        return self.push_one_sided(StateSide::To, to_object, opening);
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
                    let continued = to_node.children().get(next);
                    let opening = Opening::of(to_node);
                    let path_len = self.path.len();
                    self.push_frame(Frame::OneSided {
                        side: StateSide::To,
                        children: opening.over(to_object),
                        skipped: Some(next),
                        path_len,
                    })?;
                    let Some(continued) = continued else {
                        let opening = self.open_one_sided(
                            namespace,
                            StateSide::From,
                            from_node,
                            from_offset + common,
                        )?;
                        return self.push_one_sided(StateSide::From, from_object, opening);
                    };
                    self.append(&[next])?;
                    from_offset += common + 1;
                    to_reference = continued;
                    to_object = self.read(to_reference)?;
                    to_node = parse_node(to_object.as_ref(), to_reference)?;
                    to_offset = 0;
                }
                (Some(_), Some(_)) => {
                    let layout = from_node.children_layout();
                    let path_len = self.path.len();
                    self.push_frame(Frame::FromRemainder {
                        node: from_object,
                        layout,
                        offset: from_offset + common,
                        path_len,
                    })?;
                    let opening =
                        self.open_one_sided(namespace, StateSide::To, to_node, to_offset + common)?;
                    return self.push_one_sided(StateSide::To, to_object, opening);
                }
            }
        }
    }

    fn resume(&mut self) -> Result<Option<Step<S::Object>>, ClosureError<S::Error>> {
        loop {
            let taken = match self.frames.last_mut() {
                None => return Ok(None),
                Some(Frame::FromRemainder { .. }) => None,
                Some(Frame::Pair { from, to, path_len }) => take_differing_child(from, to)
                    .map_err(ClosureError::Format)?
                    .map(|(edge, step)| (*path_len, edge, step)),
                Some(Frame::OneSided {
                    side,
                    children,
                    skipped,
                    path_len,
                }) => take_child(children, *skipped)
                    .map_err(ClosureError::Format)?
                    .map(|child| {
                        (*path_len, child.edge, ChildStep::Whole {
                            side: *side,
                            reference: child.target,
                        })
                    }),
            };
            match taken {
                Some((path_len, edge, step)) => {
                    self.path.truncate(path_len);
                    self.append(&[edge])?;
                    return Ok(Some(Step::Child(step)));
                }
                None => {
                    if let Some(Frame::FromRemainder {
                        node,
                        layout,
                        offset,
                        path_len,
                    }) = self.frames.pop()
                    {
                        self.path.truncate(path_len);
                        return Ok(Some(Step::FromRemainder {
                            node,
                            layout,
                            offset,
                        }));
                    }
                }
            }
        }
    }

    fn open_one_sided(
        &mut self,
        namespace: SemanticNamespace,
        side: StateSide,
        node: CanonicalDagNodeView<'_>,
        offset: usize,
    ) -> Result<Opening, ClosureError<S::Error>> {
        self.append(&node.prefix()[offset..])?;
        match side {
            StateSide::From => self.compare(namespace, node.endpoint(), EndpointValue::Absent)?,
            StateSide::To => self.compare(namespace, EndpointValue::Absent, node.endpoint())?,
        }
        Ok(Opening::of(node))
    }

    fn push_one_sided(
        &mut self,
        side: StateSide,
        node: S::Object,
        opening: Opening,
    ) -> Result<(), ClosureError<S::Error>> {
        let path_len = self.path.len();
        self.push_frame(Frame::OneSided {
            side,
            children: opening.over(node),
            skipped: None,
            path_len,
        })
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

    fn push_frame(&mut self, frame: Frame<S::Object>) -> Result<(), ClosureError<S::Error>> {
        let attempted = checked_len(self.frames.len(), "state difference frames")
            .map_err(ClosureError::Format)?
            .checked_add(1)
            .ok_or(ClosureError::Format(CanonicalFormatError::CounterOverflow(
                "state difference frames",
            )))?;
        self.frames.try_reserve(1).map_err(|_| {
            ClosureError::Format(CanonicalFormatError::AllocationFailure {
                requested: attempted,
            })
        })?;
        self.frames.push(frame);
        self.report.maximum_frames = self.report.maximum_frames.max(attempted);
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
        self.path.try_reserve(bytes.len()).map_err(|_| {
            ClosureError::Format(CanonicalFormatError::AllocationFailure {
                requested: attempted,
            })
        })?;
        self.path.extend_from_slice(bytes);
        self.report.maximum_path_bytes = self.report.maximum_path_bytes.max(attempted);
        Ok(())
    }
}

fn children_of<O: AsRef<[u8]>>(
    node: &O,
    layout: DagChildrenLayout,
) -> Result<DagChildrenView<'_>, CanonicalFormatError> {
    layout.children(node.as_ref())
}

fn child_at(
    children: DagChildrenView<'_>,
    next: Option<DagChildPosition>,
) -> Option<(DagChildPosition, DagChild)> {
    let position = next?;
    Some((position, children.at(position)?))
}

fn one_sided_step(side: StateSide, child: DagChild) -> (u8, ChildStep) {
    (child.edge, ChildStep::Whole {
        side,
        reference: child.target,
    })
}

fn take_child<O: AsRef<[u8]>>(
    descent: &mut Descent<O>,
    skipped: Option<u8>,
) -> Result<Option<DagChild>, CanonicalFormatError> {
    let Some(mut position) = descent.next else {
        return Ok(None);
    };
    let children = children_of(&descent.node, descent.layout)?;
    loop {
        descent.next = children.below(position);
        match children.at(position) {
            Some(child) if Some(child.edge) != skipped => return Ok(Some(child)),
            _ => match descent.next {
                Some(lower) => position = lower,
                None => return Ok(None),
            },
        }
    }
}

fn take_differing_child<O: AsRef<[u8]>>(
    from: &mut Descent<O>,
    to: &mut Descent<O>,
) -> Result<Option<(u8, ChildStep)>, CanonicalFormatError> {
    if let (None, None) = (from.next, to.next) {
        return Ok(None);
    }
    let from_children = children_of(&from.node, from.layout)?;
    let to_children = children_of(&to.node, to.layout)?;
    loop {
        match (child_at(from_children, from.next), child_at(to_children, to.next)) {
            (None, None) => return Ok(None),
            (Some((position, child)), None) => {
                from.next = from_children.below(position);
                return Ok(Some(one_sided_step(StateSide::From, child)));
            }
            (None, Some((position, child))) => {
                to.next = to_children.below(position);
                return Ok(Some(one_sided_step(StateSide::To, child)));
            }
            (Some((from_position, from_child)), Some((to_position, to_child))) => {
                match from_child.edge.cmp(&to_child.edge) {
                    Ordering::Greater => {
                        from.next = from_children.below(from_position);
                        return Ok(Some(one_sided_step(StateSide::From, from_child)));
                    }
                    Ordering::Less => {
                        to.next = to_children.below(to_position);
                        return Ok(Some(one_sided_step(StateSide::To, to_child)));
                    }
                    Ordering::Equal => {
                        from.next = from_children.below(from_position);
                        to.next = to_children.below(to_position);
                        if from_child.target != to_child.target {
                            return Ok(Some((from_child.edge, ChildStep::Pair {
                                from: from_child.target,
                                to: to_child.target,
                            })));
                        }
                    }
                }
            }
        }
    }
}

fn reserved<T, E>(requested: u64) -> Result<Vec<T>, ClosureError<E>> {
    let refusal = || ClosureError::Format(CanonicalFormatError::AllocationFailure { requested });
    let capacity = usize::try_from(requested).map_err(|_| refusal())?;
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(capacity).map_err(|_| refusal())?;
    Ok(buffer)
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
