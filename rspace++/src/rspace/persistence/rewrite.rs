//! Iterative affected-frontier rewrites for canonical RSpace radix DAGs.
//!
//! The compiler consumes lexically ordered mutation cursors, retains the
//! adjacent keys' longest common prefix, opens only affected DAG nodes, and
//! finalizes the edited frontier bottom-up.  Untouched child references are
//! never opened merely to reconstruct their parent.

use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;
use std::sync::Arc;

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len,
};
use super::manifest::{SEMANTIC_MANIFEST_MAX_CANONICAL_LEN, SemanticNamespace, SemanticRoots};
use super::mutation::MutationBatchViewV1;
use super::object::{
    CanonicalDagNode, CanonicalObjectKind, CanonicalObjectView, DagChild, EndpointValue,
};
use super::store::{ImmutableObjectStore, ObjectAdmission, StagingObjectStore, StagingStoreError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewriteError<E> {
    Store(Box<E>),
    Format(CanonicalFormatError),
    CompareAndSwap(Box<CompareAndSwapFailure>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareAndSwapFailure {
    pub namespace: SemanticNamespace,
    pub key_digest: ObjectDigest,
    pub key_len: u64,
    pub expected: EndpointValue,
    pub actual: EndpointValue,
}

impl<E: Display> Display for RewriteError<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "persistent object-store failure: {error}"),
            Self::Format(error) => Display::fmt(error, formatter),
            Self::CompareAndSwap(failure) => {
                let CompareAndSwapFailure {
                    namespace,
                    key_digest,
                    key_len,
                    expected,
                    actual,
                } = failure.as_ref();
                write!(
                    formatter,
                    "{} mutation compare-and-swap failed at {}-byte key digest {}: expected {}, \
                     found {}",
                    namespace.name(),
                    key_len,
                    key_digest,
                    expected.name(),
                    actual.name()
                )
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RewriteError<E> {}

impl<E> From<CanonicalFormatError> for RewriteError<E> {
    fn from(error: CanonicalFormatError) -> Self { Self::Format(error) }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DagRewriteReport {
    pub mutations: u64,
    pub loaded_nodes: u64,
    pub loaded_bytes: u64,
    pub edited_nodes: u64,
    pub retained_prefix_bytes: u64,
    pub emitted_nodes: u64,
    pub emitted_bytes: u64,
    pub inserted_objects: u64,
    pub reused_child_references: u64,
    pub maximum_cursor_nodes: u64,
    pub maximum_finalize_worklist: u64,
}

impl DagRewriteReport {
    fn absorb(&mut self, other: Self) -> Result<(), CanonicalFormatError> {
        self.mutations = checked_add(self.mutations, other.mutations, "rewrite mutations")?;
        self.loaded_nodes = checked_add(self.loaded_nodes, other.loaded_nodes, "loaded nodes")?;
        self.loaded_bytes = checked_add(self.loaded_bytes, other.loaded_bytes, "loaded bytes")?;
        self.edited_nodes = checked_add(self.edited_nodes, other.edited_nodes, "edited nodes")?;
        self.retained_prefix_bytes = checked_add(
            self.retained_prefix_bytes,
            other.retained_prefix_bytes,
            "retained prefix bytes",
        )?;
        self.emitted_nodes = checked_add(self.emitted_nodes, other.emitted_nodes, "emitted nodes")?;
        self.emitted_bytes = checked_add(self.emitted_bytes, other.emitted_bytes, "emitted bytes")?;
        self.inserted_objects =
            checked_add(self.inserted_objects, other.inserted_objects, "inserted objects")?;
        self.reused_child_references = checked_add(
            self.reused_child_references,
            other.reused_child_references,
            "reused child references",
        )?;
        self.maximum_cursor_nodes = self.maximum_cursor_nodes.max(other.maximum_cursor_nodes);
        self.maximum_finalize_worklist = self
            .maximum_finalize_worklist
            .max(other.maximum_finalize_worklist);
        Ok(())
    }
}

/// A successfully compiled, still-private state transition.  Dropping this
/// value discards every staged object; later WAL code must validate the target
/// manifest before it can consume the staged set for durable publication.
pub struct PreparedDagRewrite<'a, S: ImmutableObjectStore> {
    base: super::mutation::StateVersionV1,
    target_generation: u64,
    roots: SemanticRoots,
    report: DagRewriteReport,
    staging: StagingObjectStore<'a, S>,
}

impl<'a, S: ImmutableObjectStore> PreparedDagRewrite<'a, S> {
    pub const fn base(&self) -> super::mutation::StateVersionV1 { self.base }

    pub const fn target_generation(&self) -> u64 { self.target_generation }

    pub const fn roots(&self) -> SemanticRoots { self.roots }

    pub const fn report(&self) -> DagRewriteReport { self.report }

    pub const fn staging(&self) -> &StagingObjectStore<'a, S> { &self.staging }
}

/// Explicit work/retention budgets for untrusted recovery input.  These are
/// resource limits, never semantic path-depth limits; trusted callers use the
/// unbounded default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagRewriteLimits {
    pub max_mutations: u64,
    pub max_loaded_nodes: u64,
    pub max_loaded_bytes: u64,
    pub max_edited_nodes: u64,
    pub max_retained_prefix_bytes: u64,
    pub max_emitted_nodes: u64,
    pub max_emitted_bytes: u64,
    pub max_cursor_nodes: u64,
    pub max_finalize_worklist: u64,
}

impl DagRewriteLimits {
    pub const UNBOUNDED: Self = Self {
        max_mutations: u64::MAX,
        max_loaded_nodes: u64::MAX,
        max_loaded_bytes: u64::MAX,
        max_edited_nodes: u64::MAX,
        max_retained_prefix_bytes: u64::MAX,
        max_emitted_nodes: u64::MAX,
        max_emitted_bytes: u64::MAX,
        max_cursor_nodes: u64::MAX,
        max_finalize_worklist: u64::MAX,
    };
}

impl Default for DagRewriteLimits {
    fn default() -> Self { Self::UNBOUNDED }
}

impl DagRewriteLimits {
    fn after(self, used: DagRewriteReport) -> Result<Self, CanonicalFormatError> {
        Ok(Self {
            max_mutations: remaining(self.max_mutations, used.mutations, "rewrite mutations")?,
            max_loaded_nodes: remaining(self.max_loaded_nodes, used.loaded_nodes, "loaded nodes")?,
            max_loaded_bytes: remaining(self.max_loaded_bytes, used.loaded_bytes, "loaded bytes")?,
            max_edited_nodes: remaining(self.max_edited_nodes, used.edited_nodes, "edited nodes")?,
            max_retained_prefix_bytes: remaining(
                self.max_retained_prefix_bytes,
                used.retained_prefix_bytes,
                "retained prefix bytes",
            )?,
            max_emitted_nodes: remaining(
                self.max_emitted_nodes,
                used.emitted_nodes,
                "emitted nodes",
            )?,
            max_emitted_bytes: remaining(
                self.max_emitted_bytes,
                used.emitted_bytes,
                "emitted bytes",
            )?,
            // Namespace editors run sequentially, so these are peak retained
            // sizes rather than cumulative work counters.
            max_cursor_nodes: self.max_cursor_nodes,
            max_finalize_worklist: self.max_finalize_worklist,
        })
    }
}

/// Rewrite the three roots from one already validated borrowed mutation view.
/// The compiler creates and owns a private staging overlay.  Only a successful
/// return exposes that overlay through `PreparedDagRewrite` for later target
/// validation and WAL publication.
pub fn rewrite_semantic_roots<'a, S: ImmutableObjectStore>(
    base: &'a S,
    mutation: MutationBatchViewV1<'_>,
) -> Result<PreparedDagRewrite<'a, S>, RewriteError<StagingStoreError<S::Error>>> {
    rewrite_semantic_roots_with_limits(base, mutation, DagRewriteLimits::UNBOUNDED)
}

pub fn rewrite_semantic_roots_with_limits<'a, S: ImmutableObjectStore>(
    base: &'a S,
    mutation: MutationBatchViewV1<'_>,
    limits: DagRewriteLimits,
) -> Result<PreparedDagRewrite<'a, S>, RewriteError<StagingStoreError<S::Error>>> {
    let store = StagingObjectStore::new(base);
    let base_version = mutation.base();
    let target_generation = mutation.target_generation();
    let roots = load_manifest_roots(&store, base_version.manifest())?;
    let (data_bag, data_report) = rewrite_root(
        &store,
        roots.data_bag,
        SemanticNamespace::DataBag,
        mutation.data().map(|entry| {
            (
                entry.key,
                count_endpoint(entry.transition.before()),
                count_endpoint(entry.transition.after()),
            )
        }),
        limits,
    )?;
    let continuation_limits = limits.after(data_report)?;
    let (continuation_map, continuation_report) = rewrite_root(
        &store,
        roots.continuation_map,
        SemanticNamespace::ContinuationMap,
        mutation.continuations().map(|entry| {
            (
                entry.key,
                content_endpoint(entry.transition.before()),
                content_endpoint(entry.transition.after()),
            )
        }),
        continuation_limits,
    )?;
    let mut report = data_report;
    report.absorb(continuation_report)?;
    let join_limits = limits.after(report)?;
    let (join_bag, join_report) = rewrite_root(
        &store,
        roots.join_bag,
        SemanticNamespace::JoinBag,
        mutation.joins().map(|entry| {
            (
                entry.key,
                count_endpoint(entry.transition.before()),
                count_endpoint(entry.transition.after()),
            )
        }),
        join_limits,
    )?;

    report.absorb(join_report)?;
    Ok(PreparedDagRewrite {
        base: base_version,
        target_generation,
        roots: SemanticRoots {
            data_bag,
            continuation_map,
            join_bag,
        },
        report,
        staging: store,
    })
}

fn count_endpoint(count: u64) -> EndpointValue {
    NonZeroU64::new(count)
        .map(EndpointValue::Multiplicity)
        .unwrap_or(EndpointValue::Absent)
}

fn content_endpoint(content: Option<ObjectRefV1>) -> EndpointValue {
    content
        .map(EndpointValue::Content)
        .unwrap_or(EndpointValue::Absent)
}

fn load_manifest_roots<S: ImmutableObjectStore>(
    store: &S,
    reference: ObjectRefV1,
) -> Result<SemanticRoots, RewriteError<S::Error>> {
    let object = load_exact_object(store, reference, SEMANTIC_MANIFEST_MAX_CANONICAL_LEN)?;
    let view = CanonicalObjectView::parse(object.as_ref())?;
    let CanonicalObjectView::SemanticManifest(manifest) = view else {
        return Err(CanonicalFormatError::UnexpectedObjectKind {
            digest: reference.digest(),
            expected: CanonicalObjectKind::SemanticManifest.name(),
            actual: view.kind().name(),
        }
        .into());
    };
    Ok(manifest.roots())
}

fn rewrite_root<'a, S, I>(
    store: &S,
    root: ObjectRefV1,
    namespace: SemanticNamespace,
    mutations: I,
    limits: DagRewriteLimits,
) -> Result<(ObjectRefV1, DagRewriteReport), RewriteError<S::Error>>
where
    S: ImmutableObjectStore,
    I: IntoIterator<Item = (&'a [u8], EndpointValue, EndpointValue)>,
{
    let mut mutations = mutations.into_iter();
    let Some((first_key, first_before, first_after)) = mutations.next() else {
        return Ok((root, DagRewriteReport::default()));
    };
    require_limit(1, limits.max_loaded_nodes, "loaded nodes")?;
    require_limit(1, limits.max_edited_nodes, "edited nodes")?;
    let (root_node, loaded_bytes, prefix_bytes) =
        load_node(store, root, limits.max_loaded_bytes, limits.max_retained_prefix_bytes)?;
    let mut editor = RadixEditor::new(store, root_node, loaded_bytes, prefix_bytes, limits)?;
    editor.apply(namespace, first_key, first_before, first_after)?;
    for (key, before, after) in mutations {
        editor.apply(namespace, key, before, after)?;
    }
    editor.finish()
}

#[derive(Clone, Copy, Debug)]
enum ChildLink {
    Reference(ObjectRefV1),
    Edit(usize),
}

#[derive(Clone, Copy, Debug)]
struct EditChild {
    edge: u8,
    link: ChildLink,
}

struct EditNode {
    prefix: Vec<u8>,
    endpoint: EndpointValue,
    children: Vec<EditChild>,
}

impl EditNode {
    fn vacant() -> Self {
        Self {
            prefix: Vec::new(),
            endpoint: EndpointValue::Absent,
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CursorStep {
    node: usize,
    end: usize,
}

struct RadixEditor<'a, S: ImmutableObjectStore> {
    store: &'a S,
    nodes: Vec<EditNode>,
    root: usize,
    cursor: Vec<CursorStep>,
    previous_key: Option<&'a [u8]>,
    report: DagRewriteReport,
    limits: DagRewriteLimits,
}

impl<'a, S: ImmutableObjectStore> RadixEditor<'a, S> {
    fn new(
        store: &'a S,
        root: EditNode,
        loaded_bytes: u64,
        retained_prefix_bytes: u64,
        limits: DagRewriteLimits,
    ) -> Result<Self, RewriteError<S::Error>> {
        let mut nodes = Vec::new();
        reserve_one(&mut nodes, "rewrite nodes")?;
        nodes.push(root);
        require_limit(1, limits.max_loaded_nodes, "loaded nodes")?;
        require_limit(loaded_bytes, limits.max_loaded_bytes, "loaded bytes")?;
        require_limit(1, limits.max_edited_nodes, "edited nodes")?;
        require_limit(
            retained_prefix_bytes,
            limits.max_retained_prefix_bytes,
            "retained prefix bytes",
        )?;
        Ok(Self {
            store,
            nodes,
            root: 0,
            cursor: Vec::new(),
            previous_key: None,
            report: DagRewriteReport {
                loaded_nodes: 1,
                loaded_bytes,
                edited_nodes: 1,
                retained_prefix_bytes,
                ..DagRewriteReport::default()
            },
            limits,
        })
    }

    fn apply(
        &mut self,
        namespace: SemanticNamespace,
        key: &'a [u8],
        before: EndpointValue,
        after: EndpointValue,
    ) -> Result<(), RewriteError<S::Error>> {
        let next_mutation = checked_add(self.report.mutations, 1, "rewrite mutations")?;
        require_limit(next_mutation, self.limits.max_mutations, "rewrite mutations")?;
        let common = self
            .previous_key
            .map(|previous| lcp(previous, key))
            .unwrap_or(0);
        let keep = self
            .cursor
            .iter()
            .take_while(|step| step.end <= common)
            .count();
        self.cursor.truncate(keep);

        let (mut node, mut position) = if let Some(step) = self.cursor.last().copied() {
            (step.node, step.end)
        } else {
            self.ensure_root_prefix(key)?
        };

        while position < key.len() {
            let edge = key[position];
            let child_slot = self.nodes[node]
                .children
                .binary_search_by_key(&edge, |child| child.edge);
            let slot = match child_slot {
                Ok(slot) => slot,
                Err(slot) => {
                    let suffix = &key[position + 1..];
                    self.preflight_push_node(suffix.len(), 0)?;
                    let leaf = self.push_node(EditNode {
                        prefix: copy_bytes(suffix, "inserted key suffix")?,
                        endpoint: EndpointValue::Absent,
                        children: Vec::new(),
                    })?;
                    reserve_one(&mut self.nodes[node].children, "rewrite node children")?;
                    self.nodes[node].children.insert(slot, EditChild {
                        edge,
                        link: ChildLink::Edit(leaf),
                    });
                    position = key.len();
                    node = leaf;
                    self.push_cursor(leaf, position)?;
                    break;
                }
            };

            let child = self.expand_child(node, slot)?;
            let remaining = &key[position + 1..];
            let (shared, child_prefix_len) = {
                let child_prefix = &self.nodes[child].prefix;
                (lcp(child_prefix, remaining), child_prefix.len())
            };
            if shared < child_prefix_len {
                let released = checked_len(shared + 1, "split prefix release")?;
                self.preflight_push_node(shared, released)?;
                let (old_edge, child_suffix, split_prefix) = {
                    let child_prefix = &self.nodes[child].prefix;
                    (
                        child_prefix[shared],
                        copy_bytes(&child_prefix[shared + 1..], "split child suffix")?,
                        copy_bytes(&child_prefix[..shared], "split prefix")?,
                    )
                };
                self.nodes[child].prefix = child_suffix;
                self.report.retained_prefix_bytes = self
                    .report
                    .retained_prefix_bytes
                    .checked_sub(released)
                    .ok_or(CanonicalFormatError::CounterOverflow("retained prefix bytes"))?;
                let mut split_children = Vec::new();
                reserve_one(&mut split_children, "split node children")?;
                split_children.push(EditChild {
                    edge: old_edge,
                    link: ChildLink::Edit(child),
                });
                let split = self.push_node(EditNode {
                    prefix: split_prefix,
                    endpoint: EndpointValue::Absent,
                    children: split_children,
                })?;
                self.nodes[node].children[slot].link = ChildLink::Edit(split);
                let advance = shared
                    .checked_add(1)
                    .ok_or(CanonicalFormatError::CounterOverflow("rewrite key position"))?;
                position = position
                    .checked_add(advance)
                    .ok_or(CanonicalFormatError::CounterOverflow("rewrite key position"))?;
                node = split;
                self.push_cursor(split, position)?;
                continue;
            }

            let advance = child_prefix_len
                .checked_add(1)
                .ok_or(CanonicalFormatError::CounterOverflow("rewrite key position"))?;
            position = position
                .checked_add(advance)
                .ok_or(CanonicalFormatError::CounterOverflow("rewrite key position"))?;
            node = child;
            self.push_cursor(child, position)?;
        }

        let actual = self.nodes[node].endpoint;
        if actual != before {
            return Err(RewriteError::CompareAndSwap(Box::new(CompareAndSwapFailure {
                namespace,
                key_digest: ObjectDigest::of_canonical_bytes(key),
                key_len: checked_len(key.len(), "compare-and-swap key")?,
                expected: before,
                actual,
            })));
        }
        self.nodes[node].endpoint = after;
        self.previous_key = Some(key);
        self.report.mutations = next_mutation;
        Ok(())
    }

    fn ensure_root_prefix(&mut self, key: &[u8]) -> Result<(usize, usize), RewriteError<S::Error>> {
        let (shared, prefix_len) = {
            let prefix = &self.nodes[self.root].prefix;
            (lcp(prefix, key), prefix.len())
        };
        if shared < prefix_len {
            let old_root = self.root;
            let released = checked_len(shared + 1, "split root release")?;
            self.preflight_push_node(shared, released)?;
            let (old_edge, root_suffix, root_prefix) = {
                let prefix = &self.nodes[old_root].prefix;
                (
                    prefix[shared],
                    copy_bytes(&prefix[shared + 1..], "split root suffix")?,
                    copy_bytes(&prefix[..shared], "split root prefix")?,
                )
            };
            self.nodes[old_root].prefix = root_suffix;
            self.report.retained_prefix_bytes = self
                .report
                .retained_prefix_bytes
                .checked_sub(released)
                .ok_or(CanonicalFormatError::CounterOverflow("retained prefix bytes"))?;
            let mut root_children = Vec::new();
            reserve_one(&mut root_children, "split root children")?;
            root_children.push(EditChild {
                edge: old_edge,
                link: ChildLink::Edit(old_root),
            });
            let root = self.push_node(EditNode {
                prefix: root_prefix,
                endpoint: EndpointValue::Absent,
                children: root_children,
            })?;
            self.root = root;
        }
        let end = self.nodes[self.root].prefix.len();
        self.push_cursor(self.root, end)?;
        Ok((self.root, end))
    }

    fn push_cursor(&mut self, node: usize, end: usize) -> Result<(), RewriteError<S::Error>> {
        let attempted = self
            .cursor
            .len()
            .checked_add(1)
            .ok_or(CanonicalFormatError::CounterOverflow("rewrite cursor"))?;
        let attempted = checked_len(attempted, "rewrite cursor")?;
        require_limit(attempted, self.limits.max_cursor_nodes, "rewrite cursor")?;
        reserve_one(&mut self.cursor, "rewrite cursor")?;
        self.cursor.push(CursorStep { node, end });
        self.report.maximum_cursor_nodes = self.report.maximum_cursor_nodes.max(attempted);
        Ok(())
    }

    fn expand_child(
        &mut self,
        parent: usize,
        slot: usize,
    ) -> Result<usize, RewriteError<S::Error>> {
        match self.nodes[parent].children[slot].link {
            ChildLink::Edit(child) => Ok(child),
            ChildLink::Reference(reference) => {
                // Reject an exhausted edit budget before materializing the
                // referenced child.  The exact prefix-byte preflight follows
                // from the bounded decode once its canonical prefix length is
                // known; `push_node` repeats both checks as an invariant.
                self.preflight_push_node(0, 0)?;
                let loaded_nodes = checked_add(self.report.loaded_nodes, 1, "loaded nodes")?;
                require_limit(loaded_nodes, self.limits.max_loaded_nodes, "loaded nodes")?;
                let remaining_prefix = remaining(
                    self.limits.max_retained_prefix_bytes,
                    self.report.retained_prefix_bytes,
                    "retained prefix bytes",
                )?;
                let remaining_bytes = remaining(
                    self.limits.max_loaded_bytes,
                    self.report.loaded_bytes,
                    "loaded bytes",
                )?;
                let (node, loaded_bytes, _) =
                    load_node(self.store, reference, remaining_bytes, remaining_prefix)?;
                let child = self.push_node(node)?;
                self.nodes[parent].children[slot].link = ChildLink::Edit(child);
                self.report.loaded_nodes = loaded_nodes;
                self.report.loaded_bytes =
                    checked_add(self.report.loaded_bytes, loaded_bytes, "loaded bytes")?;
                Ok(child)
            }
        }
    }

    fn preflight_push_node(
        &self,
        prefix_len: usize,
        released_prefix_bytes: u64,
    ) -> Result<(), RewriteError<S::Error>> {
        let edited_nodes = checked_add(self.report.edited_nodes, 1, "edited nodes")?;
        require_limit(edited_nodes, self.limits.max_edited_nodes, "edited nodes")?;
        let retained_after_release = self
            .report
            .retained_prefix_bytes
            .checked_sub(released_prefix_bytes)
            .ok_or(CanonicalFormatError::CounterOverflow("retained prefix bytes"))?;
        let prefix_bytes = checked_len(prefix_len, "retained prefix bytes")?;
        let retained_prefix_bytes =
            checked_add(retained_after_release, prefix_bytes, "retained prefix bytes")?;
        require_limit(
            retained_prefix_bytes,
            self.limits.max_retained_prefix_bytes,
            "retained prefix bytes",
        )?;
        Ok(())
    }

    fn push_node(&mut self, node: EditNode) -> Result<usize, RewriteError<S::Error>> {
        let prefix_bytes = checked_len(node.prefix.len(), "retained prefix bytes")?;
        let edited_nodes = checked_add(self.report.edited_nodes, 1, "edited nodes")?;
        let retained_prefix_bytes =
            checked_add(self.report.retained_prefix_bytes, prefix_bytes, "retained prefix bytes")?;
        require_limit(edited_nodes, self.limits.max_edited_nodes, "edited nodes")?;
        require_limit(
            retained_prefix_bytes,
            self.limits.max_retained_prefix_bytes,
            "retained prefix bytes",
        )?;
        reserve_one(&mut self.nodes, "rewrite nodes")?;
        let id = self.nodes.len();
        self.nodes.push(node);
        self.report.edited_nodes = edited_nodes;
        self.report.retained_prefix_bytes = retained_prefix_bytes;
        Ok(id)
    }

    fn finish(mut self) -> Result<(ObjectRefV1, DagRewriteReport), RewriteError<S::Error>> {
        enum Work {
            Enter(usize),
            Exit(usize),
        }
        let mut work = Vec::new();
        require_limit(1, self.limits.max_finalize_worklist, "rewrite finalization worklist")?;
        reserve_one(&mut work, "rewrite finalization worklist")?;
        work.push(Work::Enter(self.root));
        let mut results: Vec<Option<FinalResult>> = Vec::new();
        let result_count = checked_len(self.nodes.len(), "rewrite results")?;
        results.try_reserve_exact(self.nodes.len()).map_err(|_| {
            CanonicalFormatError::AllocationFailure {
                requested: result_count,
            }
        })?;
        results.resize_with(self.nodes.len(), || None);

        while let Some(item) = work.pop() {
            match item {
                Work::Enter(node) => {
                    let edit_children = self.nodes[node]
                        .children
                        .iter()
                        .filter_map(|child| match child.link {
                            ChildLink::Edit(child) => Some(child),
                            ChildLink::Reference(_) => None,
                        })
                        .count();
                    let additions = edit_children.checked_add(1).ok_or(
                        CanonicalFormatError::CounterOverflow("rewrite finalization worklist"),
                    )?;
                    let attempted = work.len().checked_add(additions).ok_or(
                        CanonicalFormatError::CounterOverflow("rewrite finalization worklist"),
                    )?;
                    let attempted = checked_len(attempted, "rewrite finalization worklist")?;
                    require_limit(
                        attempted,
                        self.limits.max_finalize_worklist,
                        "rewrite finalization worklist",
                    )?;
                    work.try_reserve(additions).map_err(|_| {
                        CanonicalFormatError::AllocationFailure {
                            requested: checked_len(additions, "rewrite finalization worklist")
                                .unwrap_or(u64::MAX),
                        }
                    })?;
                    work.push(Work::Exit(node));
                    for child in self.nodes[node].children.iter().rev() {
                        if let ChildLink::Edit(child) = child.link {
                            work.push(Work::Enter(child));
                        }
                    }
                    self.report.maximum_finalize_worklist =
                        self.report.maximum_finalize_worklist.max(attempted);
                }
                Work::Exit(node) => {
                    let is_root = node == self.root;
                    let result = self.finalize_node(node, is_root, &mut results)?;
                    results[node] = Some(result);
                }
            }
        }

        let result = results[self.root]
            .take()
            .expect("the iterative finalizer completes the root");
        let FinalResult::Present(root) = result else {
            unreachable!("the canonical root is represented by an empty node")
        };
        self.stage_final(&root)?;
        Ok((root.reference, self.report))
    }

    fn finalize_node(
        &mut self,
        node_id: usize,
        is_root: bool,
        results: &mut [Option<FinalResult>],
    ) -> Result<FinalResult, RewriteError<S::Error>> {
        let node = std::mem::replace(&mut self.nodes[node_id], EditNode::vacant());
        let mut children = Vec::new();
        children.try_reserve(node.children.len()).map_err(|_| {
            CanonicalFormatError::AllocationFailure {
                requested: node.children.len() as u64,
            }
        })?;
        for child in node.children {
            match child.link {
                ChildLink::Reference(reference) => {
                    self.report.reused_child_references = checked_add(
                        self.report.reused_child_references,
                        1,
                        "reused child references",
                    )?;
                    children.push(ResolvedChild {
                        edge: child.edge,
                        reference,
                        edited: None,
                    });
                }
                ChildLink::Edit(child_id) => {
                    let result = results[child_id]
                        .take()
                        .expect("children finalize before their parent");
                    if let FinalResult::Present(child_result) = result {
                        children.push(ResolvedChild {
                            edge: child.edge,
                            reference: child_result.reference,
                            edited: Some(child_result),
                        });
                    }
                }
            }
        }

        if matches!(node.endpoint, EndpointValue::Absent) && children.is_empty() {
            if !is_root {
                return Ok(FinalResult::Empty);
            }
            return self
                .build_final(CanonicalDagNode::empty())
                .map(FinalResult::Present);
        }

        let canonical = if matches!(node.endpoint, EndpointValue::Absent) && children.len() == 1 {
            let mut child = children.pop().expect("one child is present");
            let child_node = match child.edited.take() {
                Some(edited) => edited.node,
                None => {
                    let loaded_nodes = checked_add(self.report.loaded_nodes, 1, "loaded nodes")?;
                    require_limit(loaded_nodes, self.limits.max_loaded_nodes, "loaded nodes")?;
                    let remaining_prefix = remaining(
                        self.limits.max_retained_prefix_bytes,
                        self.report.retained_prefix_bytes,
                        "retained prefix bytes",
                    )?;
                    let remaining_bytes = remaining(
                        self.limits.max_loaded_bytes,
                        self.report.loaded_bytes,
                        "loaded bytes",
                    )?;
                    let (loaded, loaded_bytes, prefix_bytes) =
                        load_node(self.store, child.reference, remaining_bytes, remaining_prefix)?;
                    self.report.loaded_nodes = loaded_nodes;
                    self.report.loaded_bytes =
                        checked_add(self.report.loaded_bytes, loaded_bytes, "loaded bytes")?;
                    self.report.retained_prefix_bytes = checked_add(
                        self.report.retained_prefix_bytes,
                        prefix_bytes,
                        "retained prefix bytes",
                    )?;
                    edit_into_canonical(loaded)?
                }
            };
            let mut prefix = node.prefix;
            let appended = child_node.prefix().len().checked_add(1).ok_or(
                CanonicalFormatError::LengthOverflow {
                    field: "compressed node prefix",
                },
            )?;
            let requested =
                prefix
                    .len()
                    .checked_add(appended)
                    .ok_or(CanonicalFormatError::LengthOverflow {
                        field: "compressed node prefix",
                    })?;
            let retained_prefix_bytes =
                checked_add(self.report.retained_prefix_bytes, 1, "retained prefix bytes")?;
            require_limit(
                retained_prefix_bytes,
                self.limits.max_retained_prefix_bytes,
                "retained prefix bytes",
            )?;
            let encoded_len = CanonicalDagNode::canonical_len_for_shape(
                requested,
                child_node.endpoint(),
                child_node.children().len(),
            )?;
            self.preflight_emission(encoded_len)?;
            prefix
                .try_reserve(appended)
                .map_err(|_| CanonicalFormatError::AllocationFailure {
                    requested: checked_len(requested, "compressed node prefix").unwrap_or(u64::MAX),
                })?;
            prefix.push(child.edge);
            prefix.extend_from_slice(child_node.prefix());
            self.report.retained_prefix_bytes = retained_prefix_bytes;
            CanonicalDagNode::new(
                prefix,
                child_node.endpoint(),
                copy_children(child_node.children())?.into_boxed_slice(),
            )?
        } else {
            let mut canonical_children = Vec::new();
            canonical_children
                .try_reserve(children.len())
                .map_err(|_| CanonicalFormatError::AllocationFailure {
                    requested: children.len() as u64,
                })?;
            for child in &children {
                if let Some(edited) = &child.edited {
                    self.stage_final(edited)?;
                }
                canonical_children.push(DagChild {
                    edge: child.edge,
                    target: child.reference,
                });
            }
            CanonicalDagNode::new(
                node.prefix,
                node.endpoint,
                canonical_children.into_boxed_slice(),
            )?
        };
        self.build_final(canonical).map(FinalResult::Present)
    }

    fn build_final(&mut self, node: CanonicalDagNode) -> Result<FinalNode, RewriteError<S::Error>> {
        let encoded_len = node.canonical_len();
        self.preflight_emission(encoded_len)?;
        let emitted_nodes = checked_add(self.report.emitted_nodes, 1, "emitted nodes")?;
        let emitted_bytes = checked_add(self.report.emitted_bytes, encoded_len, "emitted bytes")?;
        let reference = node.object_ref();
        let bytes: Arc<[u8]> = node.try_canonical_bytes()?.into();
        self.report.emitted_nodes = emitted_nodes;
        self.report.emitted_bytes = emitted_bytes;
        Ok(FinalNode {
            reference,
            node,
            bytes,
        })
    }

    fn preflight_emission(&self, encoded_len: u64) -> Result<(), RewriteError<S::Error>> {
        let emitted_nodes = checked_add(self.report.emitted_nodes, 1, "emitted nodes")?;
        let emitted_bytes = checked_add(self.report.emitted_bytes, encoded_len, "emitted bytes")?;
        require_limit(emitted_nodes, self.limits.max_emitted_nodes, "emitted nodes")?;
        require_limit(emitted_bytes, self.limits.max_emitted_bytes, "emitted bytes")?;
        Ok(())
    }

    fn stage_final(&mut self, node: &FinalNode) -> Result<(), RewriteError<S::Error>> {
        let admission = self
            .store
            .put_if_absent_or_equal(node.reference, Arc::clone(&node.bytes))
            .map_err(|error| RewriteError::Store(Box::new(error)))?;
        if admission == ObjectAdmission::Inserted {
            self.report.inserted_objects =
                checked_add(self.report.inserted_objects, 1, "inserted objects")?;
        }
        Ok(())
    }
}

struct ResolvedChild {
    edge: u8,
    reference: ObjectRefV1,
    edited: Option<FinalNode>,
}

enum FinalResult {
    Empty,
    Present(FinalNode),
}

struct FinalNode {
    reference: ObjectRefV1,
    node: CanonicalDagNode,
    bytes: Arc<[u8]>,
}

fn load_node<S: ImmutableObjectStore>(
    store: &S,
    reference: ObjectRefV1,
    max_loaded_bytes: u64,
    max_prefix_bytes: u64,
) -> Result<(EditNode, u64, u64), RewriteError<S::Error>> {
    require_limit(reference.encoded_len(), max_loaded_bytes, "loaded bytes")?;
    let object = load_exact_object(store, reference, max_loaded_bytes)?;
    let bytes = object.as_ref();
    let object = CanonicalObjectView::parse(bytes)?;
    let CanonicalObjectView::DagNode(node) = object else {
        return Err(CanonicalFormatError::UnexpectedObjectKind {
            digest: reference.digest(),
            expected: CanonicalObjectKind::DagNode.name(),
            actual: object.kind().name(),
        }
        .into());
    };
    let prefix_bytes = checked_len(node.prefix().len(), "loaded node prefix")?;
    require_limit(prefix_bytes, max_prefix_bytes, "retained prefix bytes")?;
    let mut children = Vec::new();
    children.try_reserve(node.children().len()).map_err(|_| {
        CanonicalFormatError::AllocationFailure {
            requested: checked_len(node.children().len(), "loaded node children")
                .unwrap_or(u64::MAX),
        }
    })?;
    for child in node.children().iter() {
        children.push(EditChild {
            edge: child.edge,
            link: ChildLink::Reference(child.target),
        });
    }
    Ok((
        EditNode {
            prefix: copy_bytes(node.prefix(), "loaded node prefix")?,
            endpoint: node.endpoint(),
            children,
        },
        reference.encoded_len(),
        prefix_bytes,
    ))
}

fn load_exact_object<S: ImmutableObjectStore>(
    store: &S,
    reference: ObjectRefV1,
    max_encoded_len: u64,
) -> Result<S::Object, RewriteError<S::Error>> {
    require_limit(reference.encoded_len(), max_encoded_len, "loaded bytes")?;
    let object = store
        .get_bounded(&reference.digest(), max_encoded_len.min(reference.encoded_len()))
        .map_err(|error| RewriteError::Store(Box::new(error)))?
        .ok_or(CanonicalFormatError::MissingObject(reference.digest()))?;
    let bytes = object.as_ref();
    let actual_len = checked_len(bytes.len(), "canonical object")?;
    if actual_len != reference.encoded_len() {
        return Err(CanonicalFormatError::ObjectLengthMismatch {
            expected: reference.encoded_len(),
            actual: actual_len,
        }
        .into());
    }
    let actual = ObjectDigest::of_canonical_bytes(bytes);
    if actual != reference.digest() {
        return Err(CanonicalFormatError::DigestMismatch {
            expected: reference.digest(),
            actual,
        }
        .into());
    }
    Ok(object)
}

fn edit_into_canonical(node: EditNode) -> Result<CanonicalDagNode, CanonicalFormatError> {
    let mut children = Vec::new();
    children
        .try_reserve_exact(node.children.len())
        .map_err(|_| CanonicalFormatError::AllocationFailure {
            requested: checked_len(node.children.len(), "canonical edit children")
                .unwrap_or(u64::MAX),
        })?;
    for child in node.children {
        let ChildLink::Reference(target) = child.link else {
            return Err(CanonicalFormatError::CounterOverflow("unfinalized edit child"));
        };
        children.push(DagChild {
            edge: child.edge,
            target,
        });
    }
    CanonicalDagNode::new(node.prefix, node.endpoint, children.into_boxed_slice())
}

fn copy_children(children: &[DagChild]) -> Result<Vec<DagChild>, CanonicalFormatError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(children.len()).map_err(|_| {
        CanonicalFormatError::AllocationFailure {
            requested: checked_len(children.len(), "canonical node children").unwrap_or(u64::MAX),
        }
    })?;
    copy.extend_from_slice(children);
    Ok(copy)
}

fn lcp(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

#[cfg(test)]
std::thread_local! {
    static PREFLIGHT_SENSITIVE_COPY_BYTES: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
}

#[cfg(test)]
pub(crate) fn reset_preflight_sensitive_copy_bytes() { PREFLIGHT_SENSITIVE_COPY_BYTES.set(0); }

#[cfg(test)]
pub(crate) fn preflight_sensitive_copy_bytes() -> u64 { PREFLIGHT_SENSITIVE_COPY_BYTES.get() }

#[cfg(test)]
fn observe_preflight_sensitive_copy(field: &'static str, requested: u64) {
    if matches!(
        field,
        "inserted key suffix" |
            "split child suffix" |
            "split prefix" |
            "split root suffix" |
            "split root prefix"
    ) {
        PREFLIGHT_SENSITIVE_COPY_BYTES.set(
            PREFLIGHT_SENSITIVE_COPY_BYTES
                .get()
                .saturating_add(requested),
        );
    }
}

fn copy_bytes(bytes: &[u8], field: &'static str) -> Result<Vec<u8>, CanonicalFormatError> {
    let requested = checked_len(bytes.len(), field)?;
    #[cfg(test)]
    observe_preflight_sensitive_copy(field, requested);
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| CanonicalFormatError::AllocationFailure { requested })?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

fn reserve_one<T>(values: &mut Vec<T>, field: &'static str) -> Result<(), CanonicalFormatError> {
    values
        .try_reserve(1)
        .map_err(|_| CanonicalFormatError::AllocationFailure {
            requested: checked_len(values.len().saturating_add(1), field).unwrap_or(u64::MAX),
        })
}

fn checked_add(left: u64, right: u64, counter: &'static str) -> Result<u64, CanonicalFormatError> {
    left.checked_add(right)
        .ok_or(CanonicalFormatError::CounterOverflow(counter))
}

fn require_limit(
    attempted: u64,
    limit: u64,
    resource: &'static str,
) -> Result<(), CanonicalFormatError> {
    if attempted > limit {
        return Err(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        });
    }
    Ok(())
}

fn remaining(limit: u64, used: u64, resource: &'static str) -> Result<u64, CanonicalFormatError> {
    limit
        .checked_sub(used)
        .ok_or(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted: used,
        })
}
