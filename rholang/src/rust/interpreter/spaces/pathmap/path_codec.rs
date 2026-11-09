//! The path codec of a PathMap space (P5e: plan "PathMap delivery", decision 9.3, F9).
//!
//! A channel of a PathMap space names a path, which is a list of path elements. The path of a
//! channel `c` holds the elements of `c` when `c` is a plain list. For every other channel, it
//! is `[c]` ([`channel_path`]). So `@c` and `@[c]` name one path, and a fresh name roots a
//! private subtree. The plan calls this function `elems`.
//!
//! The keys of a path `p = [e_1, …, e_n]` and of a prefix `q = [e_1, …, e_k]` are these:
//!
//! ```text
//! path_key(p)   = seg(e_1) ‖ … ‖ seg(e_n) ‖ 0x00
//! prefix_key(q) = seg(e_1) ‖ … ‖ seg(e_k)
//! ```
//!
//! The data of the path `p` live on `D(sid, path_key(p))` (`namespace::path_data_channel`). The
//! markers and the waiters of the prefix `q` live on `I(sid, prefix_key(q))`
//! (`namespace::path_index_channel`).
//!
//! # The segment codec
//!
//! The segment `seg(e)` of an element is byte-identical to the EPM1 segment codec of
//! `feature/mettail` (`models/src/rust/canonical_path.rs` at `11c92e7cc`) for every element that
//! this codec accepts. `uv` is minimal unsigned LEB128, and `zigzag` maps a signed integer to an
//! unsigned one.
//!
//! | Tag    | Element                                       | Bytes after the tag                       |
//! |--------|-----------------------------------------------|-------------------------------------------|
//! | `0x01` | `GBool(false)`                                | none                                      |
//! | `0x02` | `GBool(true)`                                 | none                                      |
//! | `0x03` | `GInt(i)`                                     | `uv(zigzag(i))`                           |
//! | `0x04` | `GString(s)`                                  | `uv(\|s\|) ‖ UTF-8 bytes`                 |
//! | `0x05` | `GUri(u)`                                     | `uv(\|u\|) ‖ UTF-8 bytes`                 |
//! | `0x06` | `GByteArray(b)`                               | `uv(\|b\|) ‖ b`                           |
//! | `0x07` | `GDouble(bits)`                               | the 8 big-endian bytes of `bits`          |
//! | `0x08` | `GBigInt(b)`                                  | `uv(\|b\|) ‖ b`                           |
//! | `0x09` | `GBigRat(n, d)`                               | `uv(\|n\|) ‖ n ‖ uv(\|d\|) ‖ d`           |
//! | `0x0A` | `GFixedPoint(u, scale)`                       | `uv(\|u\|) ‖ u ‖ uv(scale)`               |
//! | `0x0B` | a ground `EList` of `k` elements              | `uv(k) ‖ seg(x_1) ‖ … ‖ seg(x_k)`         |
//! | `0x0C` | a ground `ETuple` of `k` elements             | `uv(k) ‖ seg(x_1) ‖ … ‖ seg(x_k)`         |
//! | `0x0E` | a `GPrivate` leaf                             | `uv(\|id\|) ‖ id`                         |
//! | `0x0F` | every other element (the escape arm)          | `uv(\|b\|) ‖ b`, `b` = prost of the sorted `Par` |
//!
//! A `Par` takes a structural arm when it is exactly one expression of the table, or exactly one
//! `GPrivate`. Its bit sets must be empty, and it must have no other part. A list or a tuple takes its structural arm
//! only when every element takes a structural arm. Otherwise the whole path element takes the
//! escape arm, so an escape occurs only at the top level of a segment, as in EPM1.
//!
//! These choices differ from the full EPM1 grammar, and each one keeps byte identity for the
//! elements that both codecs share:
//!
//! - An element that holds an `EPathMap` or an `EZipper` at any depth fails with
//!   `UnsupportedPathElement`. So the EPM1 tags `0x0D` and `0x10` never occur, and no key
//!   depends on the representation of a PathMap before the EPM1 merge.
//! - The escape payload is the prost encoding of the `Par` that the HEAD sorter
//!   (`ParSortMatcher`) sorts. A HEAD `Par` keeps its parts in lists.
//! - `GFloat32` and the cost parts of a HEAD `Par` have no EPM1 arm, so they take the escape arm.
//!
//! # Laws
//!
//! Every segment starts with a tag in `0x01..=0x0F`, and every variable part carries its length.
//! So no segment is a proper prefix of another segment, and `seg` is injective. Thus `q ⪯ p`
//! exactly when `prefix_key(q)` is a byte prefix of `path_key(p)` (`PathMapPrefix.v`,
//! `prefix_key_iff_prefix`). The terminator `0x00` is less than every tag. So in byte order, the
//! key of a path precedes the keys of all of its extensions (`candidate_order_total`).
//!
//! A channel path or a prefix with more than [`PATH_DEPTH_CAP`] elements fails with
//! `PathTooDeep` (F9). The bound limits the index channels and the markers of one datum. An entry
//! of a PathMap payload is not a channel, so [`entry_key`] has no depth bound. A re-rooted entry
//! can be deeper than the bound.
//!
//! # The encode of an element
//!
//! The encode of an element reads the element twice. The scan classifies the element and looks for
//! a PathMap. Then the write makes its structural segment, or the escape arm sorts the element and
//! encodes it with prost.

use std::cmp::Ordering;
use std::mem::size_of;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as SignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostSignature, CostSignatureCompound, CostSignedTerm,
    CostStack, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus,
    EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus, EPlusPlus, ESet,
    ETuple, Expr, GBigRational, GFixedPoint, GPrivate, GUnforgeable, If, KeyValuePair, Match,
    MatchCase, New, Par, Receive, ReceiveBind, Send,
};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;

use crate::rust::interpreter::spaces::errors::SpaceError;

/// The most elements of a channel path or of a prefix of a PathMap space (F9, vetoable). A datum
/// has one marker for each prefix of its path, so the bound limits the markers of a datum to
/// `PATH_DEPTH_CAP + 1`.
pub const PATH_DEPTH_CAP: usize = 64;

/// The tag bytes of the segment codec, with the numbers of the EPM1 codec.
pub mod tag {
    /// The terminator of a path key. It never starts a segment.
    pub const TERM: u8 = 0x00;
    /// `GBool(false)`.
    pub const GBOOL_FALSE: u8 = 0x01;
    /// `GBool(true)`.
    pub const GBOOL_TRUE: u8 = 0x02;
    /// `GInt(i)`, with `uv(zigzag(i))`.
    pub const GINT: u8 = 0x03;
    /// `GString(s)`, with the length and the UTF-8 bytes of `s`.
    pub const GSTRING: u8 = 0x04;
    /// `GUri(u)`, with the length and the UTF-8 bytes of `u`.
    pub const GURI: u8 = 0x05;
    /// `GByteArray(b)`, with the length and the bytes of `b`.
    pub const GBYTE_ARRAY: u8 = 0x06;
    /// `GDouble(bits)`, with the 8 big-endian bytes of `bits`.
    pub const GDOUBLE: u8 = 0x07;
    /// `GBigInt(b)`, with the length and the bytes of `b`.
    pub const GBIG_INT: u8 = 0x08;
    /// `GBigRat(n, d)`, with the length and the bytes of `n`, then of `d`.
    pub const GBIG_RAT: u8 = 0x09;
    /// `GFixedPoint(u, scale)`, with the length and the bytes of `u`, then `uv(scale)`.
    pub const GFIXED_POINT: u8 = 0x0A;
    /// A ground `EList`, with its element count and the segments of its elements.
    pub const ELIST: u8 = 0x0B;
    /// A ground `ETuple`, with its element count and the segments of its elements.
    pub const ETUPLE: u8 = 0x0C;
    /// A `GPrivate` leaf, with the length and the bytes of its id.
    pub const GPRIVATE: u8 = 0x0E;
    /// The escape arm: the length and the prost bytes of the sorted `Par` of the element.
    pub const ESCAPE: u8 = 0x0F;
}

/// The first capacity of the worklist of a walk.
const WORKLIST_CAPACITY: usize = 8;

/// The elements of `par` when `par` is a plain list, else `None`. A plain list is one `EList`
/// with no remainder, with empty bit sets and no `connective_used` flag, and with no other part.
/// This is the split carrier of the EPM1 codec (`split_carrier_list`).
pub fn plain_list_elements(par: &Par) -> Option<&[Par]> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free,
        connective_used,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    let bare = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty()
        && locally_free.is_empty()
        && use_blocks.is_empty()
        && !*connective_used;
    match (bare, exprs.as_slice(), unforgeables.as_slice()) {
        (
            true,
            [Expr {
                expr_instance:
                    Some(ExprInstance::EListBody(EList {
                        ps,
                        locally_free,
                        connective_used: false,
                        remainder: None,
                    })),
            }],
            [],
        ) if locally_free.is_empty() => Some(ps.as_slice()),
        _ => None,
    }
}

/// The path of an element or of a channel `par`: the elements of `par` when it is a plain list,
/// and `[par]` otherwise. It is `elems` of the plan and of `PathMapPrefix.v`.
pub fn path_elements(par: &Par) -> &[Par] {
    match plain_list_elements(par) {
        Some(elements) => elements,
        None => std::slice::from_ref(par),
    }
}

/// [`path_elements`] of an owned `par`. It moves the elements out of a plain list, and it makes a
/// list of one element from every other `par`.
pub fn into_path_elements(par: Par) -> Result<Vec<Par>, SpaceError> {
    match plain_list_elements(&par).is_some() {
        true => {
            let Par { mut exprs, .. } = par;
            match exprs.pop() {
                Some(Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
                }) => Ok(ps),
                _ => Err(SpaceError::Inconsistent("a plain list has no list")),
            }
        }
        false => {
            let mut elements = Vec::<Par>::with_capacity(1);
            elements.push(par);
            Ok(elements)
        }
    }
}

/// The union of the `locally_free` bit sets and the disjunction of the `connective_used` flags of
/// `parts`, as the normalizer computes them for a collection.
pub fn bit_sets(parts: &[Par]) -> (Vec<u8>, bool) {
    let mut widest = 0usize;
    for part in parts {
        widest = widest.max(part.locally_free.len());
    }
    let mut union = vec![0u8; widest];
    let mut connective_used = false;
    for part in parts {
        for (bits, other) in union.iter_mut().zip(&part.locally_free) {
            *bits |= *other;
        }
        connective_used = connective_used || part.connective_used;
    }
    (union, connective_used)
}

/// The plain list of `elements`, with the bit sets of the normalizer. Its `locally_free` set is
/// the union of the sets of the elements, and its flag is the disjunction of their flags. It
/// restates `list_par` of `record.rs`. The list of closed elements is a plain list, so
/// [`path_elements`] gives `elements` back. The elements move into the list.
pub fn plain_list(elements: Vec<Par>) -> Par {
    let (locally_free, connective_used) = bit_sets(&elements);
    let list_bits = locally_free.clone();
    let mut exprs = Vec::<Expr>::with_capacity(1);
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: elements,
            locally_free: list_bits,
            connective_used,
            remainder: None,
        })),
    });
    Par {
        exprs,
        locally_free,
        connective_used,
        ..Par::default()
    }
}

/// The path of the channel `channel` of a PathMap space ([`path_elements`]). A path of more than
/// [`PATH_DEPTH_CAP`] elements fails with `PathTooDeep`.
pub fn channel_path(channel: &Par) -> Result<&[Par], SpaceError> {
    let path = path_elements(channel);
    check_depth(path.len())?;
    Ok(path)
}

/// Fails with `PathTooDeep` when a channel path or a prefix has more than [`PATH_DEPTH_CAP`]
/// elements.
fn check_depth(depth: usize) -> Result<(), SpaceError> {
    match depth > PATH_DEPTH_CAP {
        true => Err(SpaceError::PathTooDeep),
        false => Ok(()),
    }
}

/// The keys of a path: its path key, and the end of the prefix key of each prefix in the path
/// key. The prefix key of a prefix is a byte prefix of the path key, so one buffer holds all of
/// the keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPath {
    key: Vec<u8>,
    ends: Vec<usize>,
}

impl EncodedPath {
    /// The number of elements of the path. An encoded path holds the end of the prefix `ε` too,
    /// so it has one end more than elements.
    pub fn depth(&self) -> usize {
        match self.ends.len() {
            0 => 0,
            ends => ends - 1,
        }
    }

    /// `path_key(p)`: the segments of the elements and the terminator.
    pub fn path_key(&self) -> &[u8] { &self.key }

    /// `prefix_key` of the prefix with `depth` elements, or `None` when the path is shorter.
    pub fn prefix_key(&self, depth: usize) -> Option<&[u8]> {
        self.ends.get(depth).and_then(|end| self.key.get(..*end))
    }

    /// The prefix keys of every prefix of the path in depth order, from `ε` to the whole path.
    /// A path of `n` elements has `n + 1` prefixes. The encoder puts each end inside the path
    /// key, and an end outside it fails closed with `Inconsistent`.
    pub fn prefix_keys(&self) -> impl Iterator<Item = Result<&[u8], SpaceError>> + '_ {
        self.ends.iter().map(|end| {
            self.key.get(..*end).ok_or(SpaceError::Inconsistent(
                "the end of a prefix key lies outside the path key",
            ))
        })
    }

    /// The path key, which the encoder owns.
    pub fn into_path_key(self) -> Vec<u8> { self.key }
}

/// The keys of the channel path or the prefix `path` ([`EncodedPath`]). A path of more than
/// [`PATH_DEPTH_CAP`] elements fails with `PathTooDeep`, and an element that holds a PathMap or a
/// zipper fails with `UnsupportedPathElement`.
pub fn encode_path(path: &[Par]) -> Result<EncodedPath, SpaceError> {
    check_depth(path.len())?;
    let (key, ends) = encode_elements(path, true, true)?;
    Ok(EncodedPath { key, ends })
}

/// `path_key(path)` of a channel path. The rules of [`encode_path`] apply.
pub fn path_key(path: &[Par]) -> Result<Vec<u8>, SpaceError> {
    check_depth(path.len())?;
    Ok(encode_elements(path, true, false)?.0)
}

/// `prefix_key(prefix)` of the prefix of a receive. The rules of [`encode_path`] apply.
pub fn prefix_key(prefix: &[Par]) -> Result<Vec<u8>, SpaceError> {
    check_depth(prefix.len())?;
    Ok(encode_elements(prefix, false, false)?.0)
}

/// The key of the path of an entry of a PathMap payload. It has the bytes of `path_key`, but no
/// depth bound, because an entry is not a channel. An element that holds a PathMap or a zipper
/// fails with `UnsupportedPathElement`.
pub fn entry_key(path: &[Par]) -> Result<Vec<u8>, SpaceError> {
    Ok(encode_elements(path, true, false)?.0)
}

/// The segment `seg(element)` of one path element.
pub fn segment(element: &Par) -> Result<Vec<u8>, SpaceError> {
    Ok(encode_elements(std::slice::from_ref(element), false, false)?.0)
}

/// True when `prefix` is a byte prefix of `key`. With a prefix key and a path key, this holds
/// exactly when the prefix is a prefix of the path (`prefix_key_iff_prefix`).
pub fn is_prefix_key(prefix: &[u8], key: &[u8]) -> bool { key.starts_with(prefix) }

/// The byte order of two keys. Across paths, the candidates of a receive follow the byte order
/// of their path keys, so the exact path comes first (`candidate_order_total`).
pub fn compare_keys(left: &[u8], right: &[u8]) -> Ordering { left.cmp(right) }

/// The plan of one segment: the length of a structural segment, or the payload of an escape.
enum SegmentPlan {
    /// The element takes structural arms, and its segment has this many bytes.
    Structural(usize),
    /// The element takes the escape arm, with these prost bytes.
    Escape(Vec<u8>),
}

impl SegmentPlan {
    /// The bytes of the segment.
    fn length(&self) -> usize {
        match self {
            SegmentPlan::Structural(length) => *length,
            SegmentPlan::Escape(payload) => 1usize + uv_len(payload.len() as u64) + payload.len(),
        }
    }
}

/// The segments of `path` in one buffer, with the terminator when `terminated`. With `ends`, it
/// also returns the end of each prefix key, from `ε` to the whole path. Every segment is planned
/// first, so the buffer is allocated once with its exact length.
fn encode_elements(
    path: &[Par],
    terminated: bool,
    ends: bool,
) -> Result<(Vec<u8>, Vec<usize>), SpaceError> {
    let mut plans = Vec::<SegmentPlan>::with_capacity(path.len());
    let mut total = usize::from(terminated);
    for element in path {
        let plan = plan_segment(element)?;
        total += plan.length();
        plans.push(plan);
    }
    let mut key = Vec::<u8>::with_capacity(total);
    let mut boundaries = match ends {
        true => {
            let count = path.len() + 1;
            let mut boundaries = Vec::<usize>::with_capacity(count);
            boundaries.push(0);
            boundaries
        }
        false => Vec::new(),
    };
    for (element, plan) in path.iter().zip(plans) {
        write_segment(element, plan, &mut key)?;
        if ends {
            boundaries.push(key.len());
        }
    }
    if terminated {
        key.push(tag::TERM);
    }
    match key.len() == total {
        true => Ok((key, boundaries)),
        false => Err(SpaceError::Inconsistent(
            "a path key differs from its planned length",
        )),
    }
}

/// Plans the segment of `element`: the scan, and the escape payload when the element is outside
/// the structural grammar.
fn plan_segment(element: &Par) -> Result<SegmentPlan, SpaceError> {
    match scan_element(element)? {
        Some(length) => Ok(SegmentPlan::Structural(length)),
        None => Ok(SegmentPlan::Escape(escape_payload(element)?)),
    }
}

/// Appends the planned segment of `element` to `out`, whose room the caller reserved.
fn write_segment(element: &Par, plan: SegmentPlan, out: &mut Vec<u8>) -> Result<(), SpaceError> {
    match plan {
        SegmentPlan::Structural(length) => {
            let start = out.len();
            write_structural(element, out)?;
            match out.len().checked_sub(start) == Some(length) {
                true => Ok(()),
                false => Err(SpaceError::Inconsistent(
                    "a structural segment differs from its planned length",
                )),
            }
        }
        SegmentPlan::Escape(payload) => {
            out.push(tag::ESCAPE);
            write_uv(out, payload.len() as u64);
            out.extend_from_slice(&payload);
            Ok(())
        }
    }
}

/// A node of the scan of an element: a `Par`, or a cost signature, which can hold `Par`s.
#[derive(Clone, Copy)]
enum Node<'p> {
    Par(&'p Par),
    Signature(&'p CostSignature),
}

/// A node of the scan and its place. A node is on the spine when every node above it takes a
/// structural arm.
#[derive(Clone, Copy)]
struct Visit<'p> {
    node: Node<'p>,
    spine: bool,
}

/// Scans every part of `element` once. It fails with `UnsupportedPathElement` when a part is an
/// `EPathMap` or an `EZipper`. It returns the length of the structural segment of `element`. It
/// returns `None` when a node of the spine takes no structural arm, and the element then takes the
/// escape arm.
fn scan_element(element: &Par) -> Result<Option<usize>, SpaceError> {
    let mut work = Vec::<Visit<'_>>::with_capacity(WORKLIST_CAPACITY);
    work.push(Visit {
        node: Node::Par(element),
        spine: true,
    });
    let mut length = Some(0usize);
    while let Some(Visit { node, spine }) = work.pop() {
        match node {
            Node::Par(par) => {
                let arm = match spine {
                    true => structural_arm(par),
                    false => None,
                };
                length = match (length, &arm, spine) {
                    (Some(total), Some(arm), _) => Some(total + arm.header_length()),
                    (Some(_), None, true) => None,
                    (other, _, _) => other,
                };
                push_par_children(par, arm.is_some(), &mut work)?;
            }
            Node::Signature(signature) => push_signature_children(signature, &mut work),
        }
    }
    Ok(length)
}

/// Pushes the `Par` `child` on the worklist, on the spine when `spine` holds.
fn push_par<'p>(child: &'p Par, spine: bool, work: &mut Vec<Visit<'p>>) {
    work.push(Visit {
        node: Node::Par(child),
        spine,
    });
}

/// Pushes the `Par` of `child`, when it holds one.
fn push_option<'p>(child: &'p Option<Par>, spine: bool, work: &mut Vec<Visit<'p>>) {
    if let Some(par) = child {
        push_par(par, spine, work);
    }
}

/// Pushes every `Par` of `children`.
fn push_all<'p>(children: &'p [Par], spine: bool, work: &mut Vec<Visit<'p>>) {
    for child in children {
        push_par(child, spine, work);
    }
}

/// Pushes the cost signature of `child`, when it holds one. A signature is never on the spine.
fn push_signature<'p>(child: &'p Option<CostSignature>, work: &mut Vec<Visit<'p>>) {
    if let Some(signature) = child {
        work.push(Visit {
            node: Node::Signature(signature),
            spine: false,
        });
    }
}

/// Pushes every part of `par` that can hold a `Par`. The children are on the spine when `spine`
/// holds, which is when `par` itself takes a structural arm on the spine. An `EPathMap` or an
/// `EZipper` among the expressions of `par` fails with `UnsupportedPathElement`.
fn push_par_children<'p>(
    par: &'p Par,
    spine: bool,
    work: &mut Vec<Visit<'p>>,
) -> Result<(), SpaceError> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables: _,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    for expr in exprs {
        push_expr_children(expr, spine, work)?;
    }
    for send in sends {
        let Send {
            chan,
            data,
            persistent: _,
            locally_free: _,
            connective_used: _,
            params,
        } = send;
        push_option(chan, false, work);
        push_all(data, false, work);
        if let Some(params) = params {
            for value in params.positional.iter().chain(params.named.values()) {
                push_par(value, false, work);
            }
        }
    }
    for receive in receives {
        let Receive {
            binds,
            body,
            persistent: _,
            peek: _,
            bind_count: _,
            locally_free: _,
            connective_used: _,
            condition,
        } = receive;
        for bind in binds {
            let ReceiveBind {
                patterns,
                source,
                remainder: _,
                free_count: _,
                cost_signature,
                query_spec,
            } = bind;
            push_all(patterns, false, work);
            push_option(source, false, work);
            push_signature(cost_signature, work);
            if let Some(query) = query_spec {
                for value in query.payload.iter().chain(query.named.values()) {
                    push_par(value, false, work);
                }
            }
        }
        push_option(body, false, work);
        push_option(condition, false, work);
    }
    for new in news {
        let New {
            bind_count: _,
            p,
            uri: _,
            injections,
            locally_free: _,
            space_types,
        } = new;
        push_option(p, false, work);
        for injection in injections.values() {
            push_par(injection, false, work);
        }
        for annotation in space_types {
            push_option(&annotation.space, false, work);
        }
    }
    for matched in matches {
        let Match {
            target,
            cases,
            locally_free: _,
            connective_used: _,
        } = matched;
        push_option(target, false, work);
        for case in cases {
            let MatchCase {
                pattern,
                source,
                free_count: _,
                guard,
            } = case;
            push_option(pattern, false, work);
            push_option(source, false, work);
            push_option(guard, false, work);
        }
    }
    for bundle in bundles {
        let Bundle {
            body,
            write_flag: _,
            read_flag: _,
        } = bundle;
        push_option(body, false, work);
    }
    for connective in connectives {
        let Connective {
            connective_instance,
        } = connective;
        match connective_instance {
            Some(ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }))
            | Some(ConnectiveInstance::ConnOrBody(ConnectiveBody { ps })) => {
                push_all(ps, false, work);
            }
            Some(ConnectiveInstance::ConnNotBody(body)) => push_par(body, false, work),
            Some(ConnectiveInstance::VarRefBody(_))
            | Some(ConnectiveInstance::ConnBool(_))
            | Some(ConnectiveInstance::ConnInt(_))
            | Some(ConnectiveInstance::ConnString(_))
            | Some(ConnectiveInstance::ConnUri(_))
            | Some(ConnectiveInstance::ConnByteArray(_))
            | None => {}
        }
    }
    for conditional in conditionals {
        let If {
            condition,
            if_true,
            if_false,
            locally_free: _,
            connective_used: _,
        } = conditional;
        push_option(condition, false, work);
        push_option(if_true, false, work);
        push_option(if_false, false, work);
    }
    for signed in cost_signed_terms {
        // Changed by decision 90 (DR-135): a signed term also holds its held price, a plain
        // integer with no `Par` in it.
        // let CostSignedTerm { body, signature } = signed;
        let CostSignedTerm {
            body,
            signature,
            held_price: _,
        } = signed;
        push_option(body, false, work);
        push_signature(signature, work);
    }
    for stack in cost_stacks {
        let CostStack { cells } = stack;
        for cell in cells {
            work.push(Visit {
                node: Node::Signature(cell),
                spine: false,
            });
        }
    }
    for block in use_blocks {
        push_option(&block.space, false, work);
        push_option(&block.body, false, work);
    }
    Ok(())
}

/// Pushes every `Par` of the expression `expr`. Only the elements of a list or a tuple can be on
/// the spine. An `EPathMap` or an `EZipper` fails with `UnsupportedPathElement`.
fn push_expr_children<'p>(
    expr: &'p Expr,
    spine: bool,
    work: &mut Vec<Visit<'p>>,
) -> Result<(), SpaceError> {
    let Some(instance) = &expr.expr_instance else {
        return Ok(());
    };
    match instance {
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::GFloat32(_)
        | ExprInstance::GUint64(_)
        | ExprInstance::GInt32(_)
        | ExprInstance::GUint32(_)
        | ExprInstance::GUint16(_)
        | ExprInstance::GUint8(_)
        | ExprInstance::EVarBody(_) => Ok(()),
        ExprInstance::ENotBody(ENot { p }) | ExprInstance::ENegBody(ENeg { p }) => {
            push_option(p, false, work);
            Ok(())
        }
        ExprInstance::EMultBody(EMult { p1, p2 })
        | ExprInstance::EDivBody(EDiv { p1, p2 })
        | ExprInstance::EModBody(EMod { p1, p2 })
        | ExprInstance::EPlusBody(EPlus { p1, p2 })
        | ExprInstance::EMinusBody(EMinus { p1, p2 })
        | ExprInstance::ELtBody(ELt { p1, p2 })
        | ExprInstance::ELteBody(ELte { p1, p2 })
        | ExprInstance::EGtBody(EGt { p1, p2 })
        | ExprInstance::EGteBody(EGte { p1, p2 })
        | ExprInstance::EEqBody(EEq { p1, p2 })
        | ExprInstance::ENeqBody(ENeq { p1, p2 })
        | ExprInstance::EAndBody(EAnd { p1, p2 })
        | ExprInstance::EOrBody(EOr { p1, p2 })
        | ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 })
        | ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 })
        | ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }) => {
            push_option(p1, false, work);
            push_option(p2, false, work);
            Ok(())
        }
        ExprInstance::EListBody(EList { ps, .. }) | ExprInstance::ETupleBody(ETuple { ps, .. }) => {
            push_all(ps, spine, work);
            Ok(())
        }
        ExprInstance::ESetBody(ESet { ps, .. }) => {
            push_all(ps, false, work);
            Ok(())
        }
        ExprInstance::EMapBody(EMap { kvs, .. }) => {
            for KeyValuePair { key, value } in kvs {
                push_option(key, false, work);
                push_option(value, false, work);
            }
            Ok(())
        }
        ExprInstance::EMethodBody(EMethod {
            target, arguments, ..
        }) => {
            push_option(target, false, work);
            push_all(arguments, false, work);
            Ok(())
        }
        ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
            push_option(target, false, work);
            push_option(pattern, false, work);
            Ok(())
        }
        ExprInstance::EFreeBody(free) => {
            push_all(&free.args, false, work);
            Ok(())
        }
        ExprInstance::EParamsBody(params) => {
            for value in params.positional.iter().chain(params.named.values()) {
                push_par(value, false, work);
            }
            Ok(())
        }
        ExprInstance::EPathmapBody(_) | ExprInstance::EZipperBody(_) => {
            Err(SpaceError::UnsupportedPathElement)
        }
    }
}

/// Pushes every `Par` of the cost signature `signature`.
fn push_signature_children<'p>(signature: &'p CostSignature, work: &mut Vec<Visit<'p>>) {
    match &signature.value {
        Some(SignatureValue::Quote(par)) | Some(SignatureValue::Name(par)) => {
            push_par(par, false, work);
        }
        Some(SignatureValue::Compound(CostSignatureCompound { elements })) => {
            for element in elements {
                work.push(Visit {
                    node: Node::Signature(element),
                    spine: false,
                });
            }
        }
        Some(SignatureValue::Ground(_))
        | Some(SignatureValue::BoundLevel(_))
        | Some(SignatureValue::Unit(_))
        | None => (),
    }
}

/// A structural arm of the segment codec, with the parts that its bytes hold.
enum Arm<'p> {
    Bool(bool),
    Int(i64),
    Text(u8, &'p str),
    Bytes(u8, &'p [u8]),
    Double(u64),
    BigRat(&'p [u8], &'p [u8]),
    Fixed(&'p [u8], u32),
    Collection(u8, &'p [Par]),
}

impl<'p> Arm<'p> {
    /// The bytes of this node of the segment, without the segments of its elements.
    fn header_length(&self) -> usize {
        match self {
            Arm::Bool(_) => 1,
            Arm::Int(value) => 1 + uv_len(zigzag(*value)),
            Arm::Text(_, text) => counted_length(text.len()),
            Arm::Bytes(_, bytes) => counted_length(bytes.len()),
            Arm::Double(_) => 1 + size_of::<u64>(),
            Arm::BigRat(numerator, denominator) => {
                counted_length(numerator.len())
                    + uv_len(denominator.len() as u64)
                    + denominator.len()
            }
            Arm::Fixed(unscaled, scale) => {
                counted_length(unscaled.len()) + uv_len(u64::from(*scale))
            }
            Arm::Collection(_, elements) => 1 + uv_len(elements.len() as u64),
        }
    }

    /// Writes this node of the segment, without the segments of its elements.
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Arm::Bool(false) => out.push(tag::GBOOL_FALSE),
            Arm::Bool(true) => out.push(tag::GBOOL_TRUE),
            Arm::Int(value) => {
                out.push(tag::GINT);
                write_uv(out, zigzag(*value));
            }
            Arm::Text(arm, text) => {
                out.push(*arm);
                write_uv(out, text.len() as u64);
                out.extend_from_slice(text.as_bytes());
            }
            Arm::Bytes(arm, bytes) => {
                out.push(*arm);
                write_uv(out, bytes.len() as u64);
                out.extend_from_slice(bytes);
            }
            Arm::Double(bits) => {
                out.push(tag::GDOUBLE);
                out.extend_from_slice(&bits.to_be_bytes());
            }
            Arm::BigRat(numerator, denominator) => {
                out.push(tag::GBIG_RAT);
                write_uv(out, numerator.len() as u64);
                out.extend_from_slice(numerator);
                write_uv(out, denominator.len() as u64);
                out.extend_from_slice(denominator);
            }
            Arm::Fixed(unscaled, scale) => {
                out.push(tag::GFIXED_POINT);
                write_uv(out, unscaled.len() as u64);
                out.extend_from_slice(unscaled);
                write_uv(out, u64::from(*scale));
            }
            Arm::Collection(arm, elements) => {
                out.push(*arm);
                write_uv(out, elements.len() as u64);
            }
        }
    }

    /// The elements of a list or a tuple, and no element for every other arm.
    fn elements(&self) -> &'p [Par] {
        match self {
            Arm::Collection(_, elements) => elements,
            Arm::Bool(_)
            | Arm::Int(_)
            | Arm::Text(..)
            | Arm::Bytes(..)
            | Arm::Double(_)
            | Arm::BigRat(..)
            | Arm::Fixed(..) => &[],
        }
    }
}

/// The bytes of a tag, a length and a payload of `length` bytes.
fn counted_length(length: usize) -> usize { 1 + uv_len(length as u64) + length }

/// The structural arm of `par`, or `None` when `par` is outside the structural grammar of the
/// EPM1 codec (`eval_stable_par` of `feature/mettail`). A list or a tuple returns its arm here,
/// and the scan checks its elements.
fn structural_arm(par: &Par) -> Option<Arm<'_>> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free,
        connective_used,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    let bare = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && cost_stacks.is_empty()
        && locally_free.is_empty()
        && use_blocks.is_empty()
        && !*connective_used;
    match (bare, exprs.as_slice(), unforgeables.as_slice()) {
        (
            true,
            [],
            [GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
            }],
        ) => Some(Arm::Bytes(tag::GPRIVATE, id)),
        (
            true,
            [Expr {
                expr_instance: Some(instance),
            }],
            [],
        ) => expression_arm(instance),
        _ => None,
    }
}

/// The structural arm of one expression, or `None` outside the structural grammar.
fn expression_arm(instance: &ExprInstance) -> Option<Arm<'_>> {
    match instance {
        ExprInstance::GBool(value) => Some(Arm::Bool(*value)),
        ExprInstance::GInt(value) => Some(Arm::Int(*value)),
        ExprInstance::GString(text) => Some(Arm::Text(tag::GSTRING, text)),
        ExprInstance::GUri(uri) => Some(Arm::Text(tag::GURI, uri)),
        ExprInstance::GByteArray(bytes) => Some(Arm::Bytes(tag::GBYTE_ARRAY, bytes)),
        ExprInstance::GDouble(bits) => Some(Arm::Double(*bits)),
        ExprInstance::GBigInt(bytes) => Some(Arm::Bytes(tag::GBIG_INT, bytes)),
        ExprInstance::GBigRat(GBigRational {
            numerator,
            denominator,
        }) => Some(Arm::BigRat(numerator, denominator)),
        ExprInstance::GFixedPoint(GFixedPoint { unscaled, scale }) => {
            Some(Arm::Fixed(unscaled, *scale))
        }
        ExprInstance::EListBody(EList {
            ps,
            locally_free,
            connective_used: false,
            remainder: None,
        }) if locally_free.is_empty() => Some(Arm::Collection(tag::ELIST, ps)),
        ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free,
            connective_used: false,
        }) if locally_free.is_empty() => Some(Arm::Collection(tag::ETUPLE, ps)),
        ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::GFloat32(_)
        | ExprInstance::GUint64(_)
        | ExprInstance::GInt32(_)
        | ExprInstance::GUint32(_)
        | ExprInstance::GUint16(_)
        | ExprInstance::GUint8(_)
        | ExprInstance::ENotBody(_)
        | ExprInstance::ENegBody(_)
        | ExprInstance::EMultBody(_)
        | ExprInstance::EDivBody(_)
        | ExprInstance::EModBody(_)
        | ExprInstance::EPlusBody(_)
        | ExprInstance::EMinusBody(_)
        | ExprInstance::ELtBody(_)
        | ExprInstance::ELteBody(_)
        | ExprInstance::EGtBody(_)
        | ExprInstance::EGteBody(_)
        | ExprInstance::EEqBody(_)
        | ExprInstance::ENeqBody(_)
        | ExprInstance::EAndBody(_)
        | ExprInstance::EOrBody(_)
        | ExprInstance::EVarBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_)
        | ExprInstance::EMethodBody(_)
        | ExprInstance::EPathmapBody(_)
        | ExprInstance::EZipperBody(_)
        | ExprInstance::EMatchesBody(_)
        | ExprInstance::EPercentPercentBody(_)
        | ExprInstance::EPlusPlusBody(_)
        | ExprInstance::EFreeBody(_)
        | ExprInstance::EParamsBody(_)
        | ExprInstance::EMinusMinusBody(_) => None,
    }
}

/// Writes the structural segment of `element` into `out`, whose room the caller reserved. The
/// write reads the element a second time.
fn write_structural(element: &Par, out: &mut Vec<u8>) -> Result<(), SpaceError> {
    let mut work = Vec::<&Par>::with_capacity(WORKLIST_CAPACITY);
    work.push(element);
    while let Some(par) = work.pop() {
        let arm = structural_arm(par).ok_or(SpaceError::Inconsistent(
            "a structural element left the structural grammar",
        ))?;
        arm.write(out);
        for child in arm.elements().iter().rev() {
            work.push(child);
        }
    }
    Ok(())
}

/// The prost bytes of the sorted `Par` of `element`, the payload of its escape arm.
fn escape_payload(element: &Par) -> Result<Vec<u8>, SpaceError> {
    let sorted = sorted_par(element);
    let length = sorted.encoded_len();
    let mut bytes = Vec::<u8>::with_capacity(length);
    sorted
        .encode(&mut bytes)
        .map_err(|_| SpaceError::Inconsistent("a path element does not encode"))?;
    Ok(bytes)
}

/// The sorted `Par` of `par`, sorted as the host sorts.
fn sorted_par(par: &Par) -> Par { ParSortMatcher::sort_match(par).term }

/// `zigzag64` of the EPM1 codec: a signed integer as an unsigned one, with small magnitudes small.
fn zigzag(value: i64) -> u64 { ((value << 1) ^ (value >> 63)) as u64 }

/// The bytes of the minimal unsigned LEB128 form of `value`.
fn uv_len(value: u64) -> usize {
    let mut length = 1;
    let mut rest = value >> 7;
    while rest != 0 {
        length += 1;
        rest >>= 7;
    }
    length
}

/// Writes the minimal unsigned LEB128 form of `value` (`write_uv` of the EPM1 codec).
fn write_uv(out: &mut Vec<u8>, value: u64) {
    let mut rest = value;
    loop {
        let group = (rest & 0x7F) as u8;
        rest >>= 7;
        match rest {
            0 => {
                out.push(group);
                return;
            }
            _ => out.push(group | 0x80),
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{
        Bundle, EList, EMap, EVar, If, KeyValuePair, New, Par, Receive, ReceiveBind, Var,
    };
    use models::rust::utils::union;
    use proptest::prelude::*;
    use prost::Message;

    use super::*;
    use crate::rust::interpreter::spaces::pathmap::test_support::*;

    fn seg(par: &Par) -> Vec<u8> { segment(par).expect("the element encodes") }

    fn key(path: &[Par]) -> Vec<u8> { path_key(path).expect("the path encodes") }

    fn pkey(prefix: &[Par]) -> Vec<u8> { prefix_key(prefix).expect("the prefix encodes") }

    fn uv(value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write_uv(&mut out, value);
        out
    }

    fn escape(payload: &[u8]) -> Vec<u8> {
        [
            vec![tag::ESCAPE],
            uv(payload.len() as u64),
            payload.to_vec(),
        ]
        .concat()
    }

    fn ints(values: &[i64]) -> Vec<Par> { values.iter().map(|value| gint(*value)).collect() }

    fn list_with_remainder() -> Par {
        expr(ExprInstance::EListBody(EList {
            ps: vec![gint(1)],
            locally_free: Vec::new(),
            connective_used: true,
            remainder: Some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            }),
        }))
    }

    /// Two elements are one path element when their canonical forms are equal.
    fn same_elements(left: &[Par], right: &[Par]) -> bool {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(left, right)| sorted(left) == sorted(right))
    }

    #[test]
    fn varint_and_zigzag_follow_the_epm1_primitives() {
        for (value, bytes) in [
            (0u64, vec![0x00]),
            (1, vec![0x01]),
            (127, vec![0x7F]),
            (128, vec![0x80, 0x01]),
            (300, vec![0xAC, 0x02]),
            (u64::MAX, [vec![0xFF; 9], vec![0x01]].concat()),
        ] {
            assert_eq!(uv(value), bytes, "{value}");
            assert_eq!(uv_len(value), bytes.len(), "{value}");
        }
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
        assert_eq!(zigzag(i64::MIN), u64::MAX);
        assert_eq!(zigzag(i64::MAX), u64::MAX - 1);
    }

    // The pins of `per_arm_byte_pins`, `top_level_split_vs_bare_vs_reject` and
    // `escape_arm_roundtrip_ordering_and_rejections` of `canonical_path.rs` at `11c92e7cc`. A
    // bare EPM1 path is one segment, and a split EPM1 path is the key of the list elements.
    #[test]
    fn segments_and_keys_match_the_epm1_pins() {
        assert_eq!(seg(&gbool(false)), vec![0x01]);
        assert_eq!(seg(&gbool(true)), vec![0x02]);
        assert_eq!(seg(&gint(0)), vec![0x03, 0x00]);
        assert_eq!(seg(&gint(-1)), vec![0x03, 0x01]);
        assert_eq!(seg(&gint(1)), vec![0x03, 0x02]);
        assert_eq!(seg(&gint(5)), vec![0x03, 0x0A]);
        assert_eq!(seg(&gstring("hi")), vec![0x04, 0x02, b'h', b'i']);
        assert_eq!(seg(&gstring("")), vec![0x04, 0x00]);
        assert_eq!(seg(&guri("")), vec![0x05, 0x00]);
        assert_eq!(seg(&gbytes(Vec::new())), vec![0x06, 0x00]);
        let double = seg(&gdouble(1.5f64.to_bits()));
        assert_eq!(double[0], 0x07);
        assert_eq!(&double[1..], 1.5f64.to_bits().to_be_bytes());
        assert_ne!(
            seg(&gdouble((-0.0f64).to_bits())),
            seg(&gdouble(0.0f64.to_bits()))
        );
        assert_eq!(seg(&gtuple(Vec::new())), vec![0x0C, 0x00]);
        assert_eq!(seg(&gprivate(vec![0xAA])), vec![0x0E, 0x01, 0xAA]);
        assert_eq!(key(path_elements(&glist(Vec::new()))), vec![0x00]);
        assert_eq!(
            key(path_elements(&glist(vec![gstring("a"), gstring("b")]))),
            vec![0x04, 0x01, b'a', 0x04, 0x01, b'b', 0x00]
        );
        assert_eq!(key(path_elements(&glist(vec![glist(Vec::new())]))), vec![
            0x0B, 0x00, 0x00
        ]);
        assert_eq!(key(path_elements(&glist(vec![gint(5)]))), vec![
            0x03, 0x0A, 0x00
        ]);
        assert_eq!(seg(&Par::default()), vec![tag::ESCAPE, 0x00]);
        let set = eset(vec![gint(1)]);
        assert_eq!(seg(&set), escape(&set.encode_to_vec()));
        assert!(seg(&gprivate(b"zzz".to_vec())) < seg(&set));
        let located = key(path_elements(&glist(vec![gstring("loc"), set.clone()])));
        assert_eq!(&located[..5], &[0x04, 0x03, b'l', b'o', b'c']);
        assert_eq!(located[5], tag::ESCAPE);
        assert_eq!(located.last(), Some(&tag::TERM));
    }

    // The arms that the EPM1 pins do not cover, from the grammar of `canonical_path.rs`.
    #[test]
    fn the_other_arms_follow_the_epm1_grammar() {
        assert_eq!(seg(&gbigint(vec![0x01, 0x00])), vec![
            0x08, 0x02, 0x01, 0x00
        ]);
        assert_eq!(seg(&gbigrat(vec![0x02], vec![0x03])), vec![
            0x09, 0x01, 0x02, 0x01, 0x03
        ]);
        assert_eq!(seg(&gbigrat(Vec::new(), Vec::new())), vec![
            0x09, 0x00, 0x00
        ]);
        assert_eq!(seg(&gfixed(vec![0x2A], 2)), vec![0x0A, 0x01, 0x2A, 0x02]);
        assert_eq!(seg(&gfixed(Vec::new(), u32::MAX)), vec![
            0x0A, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F
        ]);
        assert_eq!(
            seg(&gint(i64::MIN)),
            [vec![0x03], vec![0xFF; 9], vec![0x01]].concat()
        );
        assert_eq!(
            seg(&gint(i64::MAX)),
            [vec![0x03, 0xFE], vec![0xFF; 8], vec![0x01]].concat()
        );
        assert_eq!(
            seg(&guri("rho:io")),
            [vec![0x05, 0x06], b"rho:io".to_vec()].concat()
        );
        assert_eq!(seg(&gbytes(vec![0x00, 0xFF])), vec![0x06, 0x02, 0x00, 0xFF]);
        assert_eq!(seg(&gtuple(vec![gint(1), gstring("x")])), vec![
            0x0C, 0x02, 0x03, 0x02, 0x04, 0x01, b'x'
        ]);
        assert_eq!(seg(&glist(vec![gbool(true), glist(Vec::new())])), vec![
            0x0B, 0x02, 0x02, 0x0B, 0x00
        ]);
    }

    #[test]
    fn elements_outside_the_structural_grammar_take_the_escape_arm() {
        let evar = expr(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(0)),
            }),
        }));
        let two_exprs = Par {
            exprs: vec![gint(2).exprs[0].clone(), gint(1).exprs[0].clone()],
            ..Par::default()
        };
        let name_and_expr = Par {
            exprs: gint(3).exprs,
            ..gprivate(b"mix".to_vec())
        };
        let flagged = Par {
            connective_used: true,
            ..gint(9)
        };
        let bits = Par {
            locally_free: vec![1],
            ..gint(9)
        };
        for par in [
            Par::default(),
            evar,
            eset(vec![gint(1)]),
            two_exprs,
            send(gint(0), vec![gint(1)]),
            name_and_expr,
            flagged,
            bits,
            gfloat32(7),
            list_with_remainder(),
            cost_stacked(gint(4)),
            glist(vec![gint(1), Par::default()]),
            gtuple(vec![eset(Vec::new())]),
        ] {
            assert_eq!(seg(&par), escape(&sorted(&par).encode_to_vec()), "{par:?}");
        }
        // The escape payload is the prost encoding of the sorted `Par`, so the order of the
        // parts of a `Par` does not change the key.
        let forward = Par {
            exprs: vec![gint(1).exprs[0].clone(), gint(2).exprs[0].clone()],
            ..Par::default()
        };
        let backward = Par {
            exprs: vec![gint(2).exprs[0].clone(), gint(1).exprs[0].clone()],
            ..Par::default()
        };
        assert_ne!(forward.encode_to_vec(), backward.encode_to_vec());
        assert_eq!(seg(&forward), seg(&backward));
    }

    #[test]
    fn pathmaps_and_zippers_are_unsupported_at_any_depth() {
        let map = pathmap(vec![gint(1)]);
        let in_receive = Par {
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: Vec::new(),
                    source: Some(gint(0)),
                    remainder: None,
                    free_count: 0,
                    cost_signature: None,
                    query_spec: None,
                }],
                body: Some(map.clone()),
                persistent: false,
                peek: false,
                bind_count: 0,
                locally_free: Vec::new(),
                connective_used: false,
                condition: None,
            }],
            ..Par::default()
        };
        let in_new = Par {
            news: vec![New {
                bind_count: 1,
                p: Some(send(gint(0), vec![map.clone()])),
                uri: Vec::new(),
                injections: Default::default(),
                locally_free: Vec::new(),
                space_types: Vec::new(),
            }],
            ..Par::default()
        };
        let in_if = Par {
            conditionals: vec![If {
                condition: Some(gbool(true)),
                if_true: Some(map.clone()),
                if_false: Some(Par::default()),
                locally_free: Vec::new(),
                connective_used: false,
            }],
            ..Par::default()
        };
        let in_bundle = Par {
            bundles: vec![Bundle {
                body: Some(zipper(Vec::new())),
                write_flag: true,
                read_flag: true,
            }],
            ..Par::default()
        };
        let in_map = expr(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(gint(1)),
                value: Some(map.clone()),
            }],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }));
        for element in [
            map.clone(),
            pathmap(Vec::new()),
            zipper(vec![gint(1)]),
            glist(vec![gint(0), map.clone()]),
            gtuple(vec![zipper(Vec::new())]),
            send(gint(0), vec![map.clone()]),
            send(map.clone(), Vec::new()),
            eset(vec![map.clone()]),
            in_map,
            cost_stacked(map.clone()),
            in_receive,
            in_new,
            in_if,
            in_bundle,
        ] {
            assert_eq!(
                segment(&element),
                Err(SpaceError::UnsupportedPathElement),
                "{element:?}"
            );
            let path = [gint(0), element.clone()];
            assert_eq!(path_key(&path), Err(SpaceError::UnsupportedPathElement));
            assert_eq!(prefix_key(&path), Err(SpaceError::UnsupportedPathElement));
            assert_eq!(entry_key(&path), Err(SpaceError::UnsupportedPathElement));
            assert_eq!(encode_path(&path), Err(SpaceError::UnsupportedPathElement));
        }
        // A PathMap channel is a path of one element, which fails at its encode.
        let channel = channel_path(&map).expect("a channel has a path");
        assert_eq!(channel, std::slice::from_ref(&map));
        assert_eq!(path_key(channel), Err(SpaceError::UnsupportedPathElement));
    }

    #[test]
    fn the_depth_cap_bounds_channel_paths_and_prefixes() {
        let deepest: Vec<Par> = (0..PATH_DEPTH_CAP as i64).map(gint).collect();
        let too_deep: Vec<Par> = (0..=PATH_DEPTH_CAP as i64).map(gint).collect();
        assert_eq!(PATH_DEPTH_CAP, 64);
        assert!(path_key(&deepest).is_ok());
        assert!(prefix_key(&deepest).is_ok());
        assert_eq!(encode_path(&deepest).expect("64 elements fit").depth(), 64);
        let channel = glist(deepest.clone());
        assert_eq!(channel_path(&channel).expect("64 elements fit").len(), 64);
        assert_eq!(path_key(&too_deep), Err(SpaceError::PathTooDeep));
        assert_eq!(prefix_key(&too_deep), Err(SpaceError::PathTooDeep));
        assert_eq!(encode_path(&too_deep), Err(SpaceError::PathTooDeep));
        assert_eq!(
            channel_path(&glist(too_deep.clone())),
            Err(SpaceError::PathTooDeep)
        );
        // An entry of a payload is not a channel, so its key has no depth bound.
        let entry = entry_key(&too_deep).expect("an entry has no depth bound");
        assert_eq!(
            entry,
            [
                key(&deepest)[..key(&deepest).len() - 1].to_vec(),
                seg(&gint(64)),
                vec![0]
            ]
            .concat()
        );
    }

    #[test]
    fn channel_paths_split_plain_lists_only() {
        let five = gint(5);
        let singleton = glist(vec![five.clone()]);
        assert_eq!(channel_path(&five), Ok(std::slice::from_ref(&five)));
        assert_eq!(channel_path(&singleton), Ok(std::slice::from_ref(&five)));
        assert_eq!(
            key(channel_path(&five).expect("a path")),
            key(channel_path(&singleton).expect("a path")),
            "@c and @[c] name one path"
        );
        let nested = glist(vec![singleton.clone()]);
        assert_eq!(channel_path(&nested), Ok(std::slice::from_ref(&singleton)));
        assert_eq!(channel_path(&glist(Vec::new())), Ok(&[][..]));
        let remainder = list_with_remainder();
        let with_send = Par {
            sends: send(gint(0), Vec::new()).sends,
            ..glist(vec![gint(1)])
        };
        let with_bits = Par {
            locally_free: vec![1],
            ..glist(vec![gint(1)])
        };
        let two_exprs = Par {
            exprs: [glist(vec![gint(1)]).exprs, gint(2).exprs].concat(),
            ..Par::default()
        };
        for unsplit in [remainder, with_send, with_bits, two_exprs] {
            assert_eq!(
                channel_path(&unsplit),
                Ok(std::slice::from_ref(&unsplit)),
                "{unsplit:?}"
            );
            assert_eq!(plain_list_elements(&unsplit), None);
        }
    }

    #[test]
    fn encoded_paths_hold_every_prefix_key() {
        let path = ints(&[0, 1, 2]);
        let encoded = encode_path(&path).expect("the path encodes");
        assert_eq!(encoded.depth(), 3);
        assert_eq!(encoded.path_key(), key(&path).as_slice());
        assert_eq!(encoded.path_key(), &[3, 0, 3, 2, 3, 4, 0]);
        let prefixes: Vec<&[u8]> = encoded
            .prefix_keys()
            .collect::<Result<_, _>>()
            .expect("the prefix keys");
        assert_eq!(prefixes, vec![
            &[][..],
            &[3, 0][..],
            &[3, 0, 3, 2][..],
            &[3, 0, 3, 2, 3, 4][..]
        ]);
        for depth in 0..=3 {
            assert_eq!(
                encoded.prefix_key(depth),
                Some(pkey(&path[..depth]).as_slice())
            );
        }
        assert_eq!(encoded.prefix_key(4), None);
        assert_eq!(encoded.clone().into_path_key(), key(&path));
        let root = encode_path(&[]).expect("the root encodes");
        assert_eq!(root.depth(), 0);
        assert_eq!(root.path_key(), &[tag::TERM]);
        let no_bytes: &[u8] = &[];
        assert_eq!(
            root.prefix_keys()
                .collect::<Result<Vec<&[u8]>, _>>()
                .expect("the prefix keys"),
            vec![no_bytes]
        );
    }

    #[test]
    fn the_fips_paths_contain_and_order() {
        let at_01 = pkey(&ints(&[0, 1]));
        assert!(is_prefix_key(&at_01, &key(&ints(&[0, 1, 2]))));
        assert!(is_prefix_key(&at_01, &key(&ints(&[0, 1, 3]))));
        assert!(is_prefix_key(&at_01, &key(&ints(&[0, 1]))));
        assert!(!is_prefix_key(&at_01, &key(&ints(&[0]))));
        assert!(!is_prefix_key(&at_01, &key(&ints(&[0, 2, 1]))));
        assert!(is_prefix_key(&[], &key(&ints(&[7]))));
        assert_eq!(
            compare_keys(&key(&ints(&[0, 1, 2])), &key(&ints(&[0, 1, 3]))),
            Ordering::Less
        );
        assert_eq!(
            compare_keys(&key(&ints(&[0, 1])), &key(&ints(&[0, 1, 2]))),
            Ordering::Less,
            "the exact path comes first"
        );
    }

    #[test]
    fn plain_lists_and_path_elements_invert_each_other() {
        let elements = vec![gint(2), gstring("hi"), glist(vec![gint(3)])];
        let list = plain_list(elements.clone());
        assert_eq!(list, glist(elements.clone()));
        assert_eq!(path_elements(&list), elements.as_slice());
        assert_eq!(into_path_elements(list).expect("the list moves"), elements);
        assert_eq!(into_path_elements(gint(5)).expect("one element"), vec![
            gint(5)
        ]);
        let bits = Par {
            locally_free: vec![0b001],
            ..gint(1)
        };
        let more_bits = Par {
            locally_free: vec![0b100, 0b010],
            connective_used: true,
            ..gint(2)
        };
        let parts = vec![bits.clone(), more_bits.clone(), gint(3)];
        assert_eq!(
            bit_sets(&parts),
            (
                union(bits.locally_free.clone(), more_bits.locally_free.clone()),
                true
            )
        );
        let flagged = plain_list(parts);
        assert_eq!(flagged.locally_free, vec![0b101, 0b010]);
        assert!(flagged.connective_used);
        assert_eq!(plain_list_elements(&flagged), None);
    }

    fn check_prefix_law(prefix: &[Par], path: &[Par]) -> Result<(), TestCaseError> {
        let expected = path.len() >= prefix.len() && same_elements(&path[..prefix.len()], prefix);
        let actual = is_prefix_key(&pkey(prefix), &key(path));
        ensure(
            actual == expected,
            "a prefix key is a prefix of a path key exactly when the prefix is a prefix",
        )
    }

    // `PathMapPrefix.v`, `prefix_key_iff_prefix`.
    #[test]
    fn prop_prefix_key_iff_prefix() {
        check(
            pure_config(),
            (path(), path(), path()),
            |(prefix, suffix, other)| {
                let extension = [prefix.clone(), suffix].concat();
                check_prefix_law(&prefix, &extension)?;
                ensure(
                    is_prefix_key(&pkey(&prefix), &key(&extension)),
                    "every extension of a prefix holds its key",
                )?;
                check_prefix_law(&prefix, &other)?;
                check_prefix_law(&other, &prefix)
            },
        );
    }

    // `PathMapPrefix.v`, `seg_unique` and `seg_tag`: the segments are a prefix code.
    #[test]
    fn prop_segments_are_a_prefix_code() {
        check(pure_config(), (element(), element()), |(left, right)| {
            let (left_bytes, right_bytes) = (seg(&left), seg(&right));
            ensure(
                (1..=tag::ESCAPE).contains(&left_bytes[0]),
                "every segment starts with a tag",
            )?;
            let equal = sorted(&left) == sorted(&right);
            ensure(
                right_bytes.starts_with(&left_bytes) == equal,
                "a segment is a prefix of another segment only when the elements are equal",
            )?;
            ensure(
                (left_bytes == right_bytes) == equal,
                "the segment codec is injective",
            )
        });
    }

    // `PathMapPrefix.v`, `path_key_injective`.
    #[test]
    fn prop_path_keys_are_injective() {
        check(pure_config(), (path(), path()), |(left, right)| {
            ensure(
                (key(&left) == key(&right)) == same_elements(&left, &right),
                "two paths have one key exactly when they are equal",
            )
        });
    }

    // `PathMapPrefix.v`, `candidate_order_total`: the candidates of a receive order by their path
    // keys, then by their order within the path. The order is strict and total, and the exact
    // path comes first.
    #[test]
    fn prop_candidate_order_total() {
        let candidate = || (path(), 0u8..3);
        check(
            pure_config(),
            (candidate(), candidate(), candidate(), path(), path()),
            |(first, second, third, prefix, suffix)| {
                let order = |left: &(Vec<Par>, u8), right: &(Vec<Par>, u8)| {
                    compare_keys(&key(&left.0), &key(&right.0)).then(left.1.cmp(&right.1))
                };
                for (left, right) in [(&first, &second), (&second, &third), (&first, &third)] {
                    ensure(
                        order(left, left) == Ordering::Equal,
                        "the order is reflexive",
                    )?;
                    ensure(
                        order(left, right) == order(right, left).reverse(),
                        "the order is antisymmetric",
                    )?;
                    ensure(
                        (order(left, right) == Ordering::Equal)
                            == (same_elements(&left.0, &right.0) && left.1 == right.1),
                        "two candidates are equal in the order only when they are equal",
                    )?;
                }
                let mut ranked = [&first, &second, &third];
                ranked.sort_by(|left, right| order(left, right));
                ensure(
                    order(ranked[0], ranked[2]) != Ordering::Greater,
                    "the order is transitive",
                )?;
                match suffix.is_empty() {
                    true => Ok(()),
                    false => {
                        let extension = [prefix.clone(), suffix].concat();
                        ensure(
                            compare_keys(&key(&prefix), &key(&extension)) == Ordering::Less,
                            "the exact path comes before its extensions",
                        )
                    }
                }
            },
        );
    }
}
