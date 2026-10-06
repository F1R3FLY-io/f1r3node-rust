//! The hash-consed trie of the causal paths in native evidence (C7b, DR-86).
//!
//! Each node stands for one causal path. The parent of a node is its path
//! without the last segment, so the trie holds every prefix of each path
//! that it holds. `children` maps (parent, segment) to the node, so equal
//! paths get the same `PathId`. A node stores the chained digest of its path
//! (C7a), so a live `CausalPath` and the node of the same path have equal
//! digests. A jump table of `levels` ancestors for each node gives ancestors
//! and longest common prefixes in O(`levels`) steps.

use std::collections::BTreeMap;
use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::operation_context::{
    child_path_digest, root_path_digest, CausalPath, PathDigest, PathSegment,
};
use shared::rust::collection_backing::{tree_growth, tree_search_bound};

use super::index::reserve_vector;
use super::recording::work;
use super::{HostWorkBudget, InterpreterError};

/// The bytes hashed for the root digest: the domain tag and the node-kind
/// byte.
const ROOT_DIGEST_INPUT_BYTES: usize = 22 + 1;

/// The bytes hashed for one child digest: the domain tag, the node-kind byte,
/// the parent digest and the two segment words.
const CHILD_DIGEST_INPUT_BYTES: usize = 22 + 1 + 32 + 16;

type ChildKey = (u32, PathSegment);

/// A node of a [`NativePathTrie`]: its index and the depth of its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathId {
    index: u32,
    depth: u32,
}

impl PathId {
    pub const ROOT: Self = Self { index: 0, depth: 0 };

    pub fn depth(self) -> usize { self.depth as usize }

    pub(super) fn index(self) -> usize { self.index as usize }
}

/// The digest key of a path: equal keys mean equal paths, unless Blake2b-256
/// collides (`NativePathTrie.occurrence_key_correct`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PathKey {
    digest: PathDigest,
    depth: usize,
    last: Option<PathSegment>,
}

impl PathKey {
    pub(super) const BYTES: usize = 32 + size_of::<usize>() + 17;

    pub(super) fn of(path: &CausalPath) -> Self {
        Self {
            digest: path.digest(),
            depth: path.len(),
            last: path.last_segment(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrieNode {
    parent: u32,
    depth: u32,
    segment: PathSegment,
    digest: PathDigest,
}

#[derive(Clone, Debug)]
pub struct NativePathTrie {
    nodes: Vec<TrieNode>,
    node_capacity: usize,
    jumps: Vec<u32>,
    jump_capacity: usize,
    levels: usize,
    max_depth: usize,
    children: BTreeMap<ChildKey, u32>,
    suffix_segments: usize,
}

/// Two tries are equal when they hold the same nodes in the same order. The
/// capacities and the admitted suffix count are bookkeeping.
impl PartialEq for NativePathTrie {
    fn eq(&self, other: &Self) -> bool { self.nodes == other.nodes }
}

impl Eq for NativePathTrie {}

fn trie_error(message: &str) -> InterpreterError {
    InterpreterError::ReduceError(format!("native path trie: {message}"))
}

fn bit_length(value: usize) -> usize { (usize::BITS - value.leading_zeros()) as usize }

fn checked(value: Option<usize>) -> Result<usize, InterpreterError> {
    value.ok_or(InterpreterError::HostWorkRejected)
}

impl NativePathTrie {
    /// A trie that holds only the root, for paths of at most `max_depth`
    /// segments.
    pub fn new(max_depth: usize, host: &HostWorkBudget) -> Result<Self, InterpreterError> {
        let levels = bit_length(max_depth).max(1);
        let mut trie = Self {
            nodes: Vec::new(),
            node_capacity: 0,
            jumps: Vec::new(),
            jump_capacity: 0,
            levels,
            max_depth,
            children: BTreeMap::new(),
            suffix_segments: 0,
        };
        reserve_vector(&mut trie.nodes, &mut trie.node_capacity, 1, host)?;
        reserve_vector(&mut trie.jumps, &mut trie.jump_capacity, levels, host)?;
        work(host, HostWorkDimension::VerificationOperations, 1)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            ROOT_DIGEST_INPUT_BYTES,
        )?;
        trie.nodes.push(TrieNode {
            parent: 0,
            depth: 0,
            segment: (0, 0),
            digest: root_path_digest(),
        });
        trie.jumps.resize(levels, 0);
        Ok(trie)
    }

    /// The number of nodes, without the root.
    pub fn node_count(&self) -> usize { self.nodes.len() - 1 }

    pub fn max_depth(&self) -> usize { self.max_depth }

    /// Whether `id` names a node of this trie.
    pub fn contains(&self, id: PathId) -> bool {
        self.nodes
            .get(id.index())
            .is_some_and(|node| node.depth == id.depth)
    }

    fn node(&self, id: PathId) -> &TrieNode { &self.nodes[id.index()] }

    pub fn digest(&self, id: PathId) -> PathDigest { self.node(id).digest }

    pub fn last_segment(&self, id: PathId) -> Option<PathSegment> {
        (id.depth != 0).then(|| self.node(id).segment)
    }

    pub fn parent(&self, id: PathId) -> PathId {
        PathId {
            index: self.node(id).parent,
            depth: id.depth.saturating_sub(1),
        }
    }

    pub(super) fn key(&self, id: PathId) -> PathKey {
        PathKey {
            digest: self.digest(id),
            depth: id.depth(),
            last: self.last_segment(id),
        }
    }

    /// Admits `count` more suffix segments. Their total over the evidence,
    /// Σ s_i, may not exceed `limit`. The decoder calls this before it
    /// allocates any node of the suffix.
    pub fn admit(&mut self, count: usize, limit: usize) -> bool {
        match self
            .suffix_segments
            .checked_add(count)
            .filter(|total| *total <= limit)
        {
            Some(total) => {
                self.suffix_segments = total;
                true
            }
            None => false,
        }
    }

    fn jump(&self, index: u32, level: usize) -> u32 {
        self.jumps[index as usize * self.levels + level]
    }

    /// The ancestor of `id` at `depth`.
    pub fn ancestor(
        &self,
        id: PathId,
        depth: usize,
        host: &HostWorkBudget,
    ) -> Result<PathId, InterpreterError> {
        let distance = id
            .depth()
            .checked_sub(depth)
            .ok_or_else(|| trie_error("ancestor below its node"))?;
        work(host, HostWorkDimension::VerificationOperations, self.levels)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            checked(self.levels.checked_mul(size_of::<u32>()))?,
        )?;
        let mut index = id.index;
        for level in 0..self.levels {
            if ((distance >> level) & 1) == 1 {
                index = self.jump(index, level);
            }
        }
        Ok(PathId {
            index,
            depth: u32::try_from(depth).map_err(|_| trie_error("depth exceeds u32"))?,
        })
    }

    /// The depth of the longest common prefix of `left` and `right`.
    pub fn lcp_depth(
        &self,
        left: PathId,
        right: PathId,
        host: &HostWorkBudget,
    ) -> Result<usize, InterpreterError> {
        let depth = left.depth().min(right.depth());
        let left = self.ancestor(left, depth, host)?;
        let right = self.ancestor(right, depth, host)?;
        if left.index == right.index {
            return Ok(depth);
        }
        work(host, HostWorkDimension::VerificationOperations, self.levels)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            checked(self.levels.checked_mul(2 * size_of::<u32>()))?,
        )?;
        let (mut left_index, mut right_index) = (left.index, right.index);
        for level in (0..self.levels).rev() {
            let (left_jump, right_jump) =
                (self.jump(left_index, level), self.jump(right_index, level));
            if left_jump != right_jump {
                left_index = left_jump;
                right_index = right_jump;
            }
        }
        Ok(self.nodes[left_index as usize].depth as usize - 1)
    }

    /// The child of `parent` with `segment`: the existing node, or a new node
    /// when the trie does not hold that path yet.
    pub fn child(
        &mut self,
        parent: PathId,
        segment: PathSegment,
        host: &HostWorkBudget,
    ) -> Result<PathId, InterpreterError> {
        if !self.contains(parent) {
            return Err(trie_error("unknown parent"));
        }
        let depth = parent
            .depth
            .checked_add(1)
            .filter(|depth| *depth as usize <= self.max_depth)
            .ok_or_else(|| trie_error("path exceeds its depth limit"))?;
        let comparisons = tree_search_bound(self.children.len());
        work(host, HostWorkDimension::VerificationOperations, comparisons)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            checked(comparisons.checked_mul(2 * size_of::<ChildKey>()))?,
        )?;
        if let Some(&index) = self.children.get(&(parent.index, segment)) {
            return Ok(PathId { index, depth });
        }
        let index =
            u32::try_from(self.nodes.len()).map_err(|_| trie_error("node index exceeds u32"))?;
        reserve_vector(&mut self.nodes, &mut self.node_capacity, 1, host)?;
        reserve_vector(&mut self.jumps, &mut self.jump_capacity, self.levels, host)?;
        let (operations, bytes) =
            checked_pair(tree_growth::<ChildKey, u32>(self.children.len(), 1))?;
        work(host, HostWorkDimension::VerificationOperations, operations)?;
        work(host, HostWorkDimension::SearchStateBytes, bytes)?;
        work(
            host,
            HostWorkDimension::VerificationOperations,
            checked(self.levels.checked_add(1))?,
        )?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            CHILD_DIGEST_INPUT_BYTES,
        )?;
        let digest = child_path_digest(&self.nodes[parent.index()].digest, segment);
        self.nodes.push(TrieNode {
            parent: parent.index,
            depth,
            segment,
            digest,
        });
        let mut ancestor = parent.index;
        self.jumps.push(ancestor);
        for level in 1..self.levels {
            ancestor = self.jump(ancestor, level - 1);
            self.jumps.push(ancestor);
        }
        self.children.insert((parent.index, segment), index);
        Ok(PathId { index, depth })
    }

    /// Interns `path` as the delta decoder does: the longest common prefix
    /// with the previous path stays, and the rest of `path` is interned below
    /// the ancestor of `previous` at that depth. The prefix scan reads 16
    /// segments at a time, as the delta encoder does.
    pub fn intern_delta(
        &mut self,
        path: &[PathSegment],
        previous_path: &[PathSegment],
        previous: PathId,
        host: &HostWorkBudget,
    ) -> Result<PathId, InterpreterError> {
        let shared = path.len().min(previous_path.len());
        let mut prefix = 0;
        while prefix < shared {
            let end = prefix.saturating_add(16).min(shared);
            let count = end - prefix;
            work(
                host,
                HostWorkDimension::VerificationOperations,
                checked(count.checked_mul(2))?,
            )?;
            work(
                host,
                HostWorkDimension::VerificationBytes,
                checked(count.checked_mul(16))?,
            )?;
            while prefix < end && path[prefix] == previous_path[prefix] {
                prefix += 1;
            }
            if prefix != end {
                break;
            }
        }
        let base = self.ancestor(previous, prefix, host)?;
        self.intern(base, &path[prefix..], host)
    }

    /// Interns `segments` below `base` and returns the node of the whole path.
    pub fn intern(
        &mut self,
        base: PathId,
        segments: &[PathSegment],
        host: &HostWorkBudget,
    ) -> Result<PathId, InterpreterError> {
        let mut node = base;
        for segment in segments {
            node = self.child(node, *segment, host)?;
        }
        Ok(node)
    }

    /// The segments of `id` below depth `from_depth`, in path order.
    pub fn suffix(
        &self,
        id: PathId,
        from_depth: usize,
        host: &HostWorkBudget,
    ) -> Result<Vec<PathSegment>, InterpreterError> {
        let count = id
            .depth()
            .checked_sub(from_depth)
            .ok_or_else(|| trie_error("suffix below its node"))?;
        work(host, HostWorkDimension::VerificationOperations, count)?;
        work(
            host,
            HostWorkDimension::SearchStateBytes,
            checked(count.checked_mul(size_of::<PathSegment>()))?,
        )?;
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(count)
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        let mut index = id.index;
        for _ in 0..count {
            let node = &self.nodes[index as usize];
            segments.push(node.segment);
            index = node.parent;
        }
        segments.reverse();
        Ok(segments)
    }

    /// The nodes without the root, in the lexicographic order of their paths:
    /// a depth-first walk that visits the children of each node by increasing
    /// segment (`NativePathTrie.trie_dfs_is_lex_sort`). `children` iterates by
    /// (parent, segment), so the children of each node are contiguous and
    /// sorted.
    pub fn preorder(&self, host: &HostWorkBudget) -> Result<Vec<PathId>, InterpreterError> {
        let count = self.nodes.len();
        let edges = self.children.len();
        work(
            host,
            HostWorkDimension::VerificationOperations,
            checked(
                edges
                    .checked_mul(2)
                    .and_then(|operations| operations.checked_add(count.checked_mul(3)?)),
            )?,
        )?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            checked(edges.checked_mul(size_of::<(ChildKey, u32)>()))?,
        )?;
        work(
            host,
            HostWorkDimension::SearchStateBytes,
            checked(
                count
                    .checked_add(1)
                    .and_then(|offsets| offsets.checked_mul(size_of::<usize>()))
                    .and_then(|bytes| bytes.checked_add(edges.checked_mul(size_of::<u32>())?))
                    .and_then(|bytes| bytes.checked_add(count.checked_mul(size_of::<u32>())?))
                    .and_then(|bytes| bytes.checked_add(count.checked_mul(size_of::<PathId>())?)),
            )?,
        )?;
        let mut offsets = Vec::new();
        offsets
            .try_reserve_exact(count + 1)
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        offsets.resize(count + 1, 0_usize);
        let mut list = Vec::new();
        list.try_reserve_exact(edges)
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        for (&(parent, _), &child) in &self.children {
            offsets[parent as usize + 1] += 1;
            list.push(child);
        }
        for index in 0..count {
            offsets[index + 1] += offsets[index];
        }
        let mut stack = Vec::new();
        stack
            .try_reserve_exact(count)
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        let mut order = Vec::new();
        order
            .try_reserve_exact(count - 1)
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        stack.push(0_u32);
        while let Some(index) = stack.pop() {
            let node = index as usize;
            if node != 0 {
                order.push(PathId {
                    index,
                    depth: self.nodes[node].depth,
                });
            }
            for child in list[offsets[node]..offsets[node + 1]].iter().rev() {
                stack.push(*child);
            }
        }
        Ok(order)
    }

    /// The segments of `id`, without host-work charges.
    #[cfg(test)]
    pub fn segments(&self, id: PathId) -> Vec<PathSegment> {
        let mut segments = Vec::with_capacity(id.depth());
        let mut index = id.index;
        for _ in 0..id.depth() {
            let node = &self.nodes[index as usize];
            segments.push(node.segment);
            index = node.parent;
        }
        segments.reverse();
        segments
    }

    /// The node of `path`, if the trie holds it.
    #[cfg(test)]
    pub fn find(&self, path: &[PathSegment]) -> Option<PathId> {
        path.iter().try_fold(PathId::ROOT, |node, segment| {
            self.children
                .get(&(node.index, *segment))
                .map(|&index| PathId {
                    index,
                    depth: node.depth + 1,
                })
        })
    }
}

fn checked_pair(value: Option<(usize, usize)>) -> Result<(usize, usize), InterpreterError> {
    value.ok_or(InterpreterError::HostWorkRejected)
}

#[cfg(test)]
#[path = "tests/path_trie.rs"]
mod tests;
