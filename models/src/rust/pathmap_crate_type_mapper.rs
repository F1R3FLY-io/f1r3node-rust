//! PathMap-native EPathMap conversion and stability classification.
//!
//! Set-mode conversion shares the stored `PathMap<()>` root. The reverse
//! conversion adopts the supplied trie without flattening it. The stability
//! classifier proves when reducer re-evaluation is byte-identical and may be
//! fused without changing semantics or accounting.

use super::pathmap_integration::{RholangSetPathMap, SetPathMapCreationResult};
use super::rhoapi_ext::EntryTrie;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{EPathMap, Par, Var};

// ─────────────────────────────────────────────────────────────────────────────
// The `eval_stable` classifier
// ─────────────────────────────────────────────────────────────────────────────

pub fn eval_stable_epathmap(e_pathmap: &EPathMap) -> bool {
    e_pathmap.remainder.is_none()
        && e_pathmap.locally_free.is_empty()
        && !e_pathmap.connective_used
        && e_pathmap.entry_trie().entries_stable()
}

/// Classify a `Par` as ground normal form: either a single-expr carrier over
/// the stable expression alphabet, or the reflect `GPrivate` leaf. Every other
/// `Par` field must be empty and `locally_free`/`connective_used` must have their
/// ground defaults.
///
/// `pub(crate)` because the canonical path codec uses this exact predicate as
/// its ground-domain gate. The codec grammar and this classifier must remain
/// one grammar, pinned by the codec's agreement property test.
///
/// # The defect this replaces
///
/// `eval_stable_par` and `eval_stable_expr` were mutually recursive,
/// descending through `EList.ps` and `ETuple.ps`. That put a Θ(depth)
/// native-stack traversal on `canonical_path.rs::encode_trie_path`, which is
/// **required total** (R3F-2: *"trie keys must build for every legal runtime
/// value"*) and whose module header promises *"unlimited depth; iterative; no
/// panics."* It could not honour either: a sufficiently nested entry aborted the
/// process inside a function documented not to.
///
/// ★ It was found by *bisection*, not by reading: a 256 KiB probe thread that was
/// built to show the escape arm's protobuf encode had become stack-safe overflowed
/// anyway, and the encoder was not the frame that ran out.
///
/// ⚠ A crash is strictly worse than a rejection. A clean `Err` is a decision every
/// node reaches identically; a `SIGSEGV` is a liveness failure of whichever node
/// was asked first, on input a peer controls.
///
/// # PDA and allocation shape
///
/// `current` is the PDA's state register and `deferred` is its continuation
/// stack. A unary collection chain allocates nothing: it advances through
/// `current` and pushes only siblings. Wider collections pay exactly for the
/// outstanding siblings, with no native recursion and no artificial depth
/// threshold. The predicate is a side-effect-free conjunction, so this
/// depth-first evaluation returns the same boolean as the recursive definition.
pub(crate) fn eval_stable_par(root: &Par) -> bool {
    let mut current = Some(root);
    let mut deferred: Vec<&Par> = Vec::new();

    loop {
        let Some(par) = current.take().or_else(|| deferred.pop()) else {
            return true;
        };

        if !par.sends.is_empty()
            || !par.receives.is_empty()
            || !par.news.is_empty()
            || !par.matches.is_empty()
            || !par.bundles.is_empty()
            || !par.connectives.is_empty()
            || !par.conditionals.is_empty()
            || !par.locally_free.is_empty()
            || par.connective_used
        {
            return false;
        }

        let expr = match (par.exprs.as_slice(), par.unforgeables.as_slice()) {
            // The reflect GPrivate leaf is an unforgeable instead of an expr.
            ([], [unforgeable])
                if matches!(unforgeable.unf_instance, Some(UnfInstance::GPrivateBody(_))) =>
            {
                continue;
            }
            // A single expression is the other valid carrier shape.
            ([expr], []) => expr,
            _ => return false,
        };

        let children = match &expr.expr_instance {
            Some(
                ExprInstance::GBool(_)
                | ExprInstance::GInt(_)
                | ExprInstance::GString(_)
                | ExprInstance::GUri(_)
                | ExprInstance::GByteArray(_)
                | ExprInstance::GDouble(_)
                | ExprInstance::GBigInt(_)
                | ExprInstance::GBigRat(_)
                | ExprInstance::GFixedPoint(_),
            ) => continue,
            Some(ExprInstance::EListBody(list))
                if list.remainder.is_none()
                    && list.locally_free.is_empty()
                    && !list.connective_used =>
            {
                list.ps.as_slice()
            }
            Some(ExprInstance::ETupleBody(tuple))
                if tuple.locally_free.is_empty() && !tuple.connective_used =>
            {
                tuple.ps.as_slice()
            }
            // O(1): `EntryTrie` maintains the entry fold as entries change.
            Some(ExprInstance::EPathmapBody(inner)) if eval_stable_epathmap(inner) => continue,
            _ => return false,
        };

        if let Some((first, siblings)) = children.split_first() {
            // Reverse-push preserves the recursive definition's left-to-right
            // visitation while storing only pending siblings.
            deferred.extend(siblings.iter().rev());
            current = Some(first);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathFrameError {
    /// Between 1 and 3 bytes remained — not enough for a `u32-LE` length
    /// header. `at` is the offset of the first orphaned byte.
    TruncatedLength { at: usize },
    /// A length header promised `needed` key bytes and only `available`
    /// remained. `at` is the offset the key would have started at.
    TruncatedKey {
        at: usize,
        needed: usize,
        available: usize,
    },
    /// `at + needed` overflowed `usize`. Unreachable on a 64-bit target (a
    /// `u32` length cannot carry an in-memory offset past `isize::MAX`), and
    /// enumerated anyway because "cannot happen" is a claim about the host.
    LengthOverflow { at: usize, needed: usize },
}

/// Reader for the retired `serialized_paths` compatibility framing.
///
/// Yields each frame's KEY as a borrowed slice, in stream order, performing
/// **no decode at all**: the framing is `repeat( u32-LE keylen ‖ key )`, so
/// splitting it is pure byte slicing. That is what makes this iterator TOTAL on
/// every byte string — `decode_trie_path`, which the key would have to go
/// through to become a `Par`, is not.
///
/// ⚠ **FUSED at the first error.** A framing fault leaves no defensible cursor
/// to resume from (the next four bytes are a length header only if the previous
/// frame was well-formed), so the iterator yields the error once and then ends.
/// Without the fuse a caller that ignored the error would loop forever on the
/// same offset.
///
/// # Why the two readers share it rather than each splitting inline
///
/// `U(m)` is written in exactly one place. Read in two, its framing would be
/// stated twice — and the two statements would be free to disagree about a
/// trailing partial header, which is precisely the class of divergence that
/// forks a network. The *dispositions* still differ (reject vs. re-file) and
/// belong to the callers; the *framing* does not.
pub(crate) struct PathFrames<'a> {
    stream: &'a [u8],
    cursor: usize,
    done: bool,
}

impl<'a> PathFrames<'a> {
    pub(crate) fn new(stream: &'a [u8]) -> Self {
        PathFrames {
            stream,
            cursor: 0,
            done: false,
        }
    }
}

impl<'a> Iterator for PathFrames<'a> {
    type Item = Result<&'a [u8], PathFrameError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let remaining = self.stream.len() - self.cursor;
        match remaining {
            0 => {
                self.done = true;
                None
            }
            1..=3 => {
                self.done = true;
                Some(Err(PathFrameError::TruncatedLength { at: self.cursor }))
            }
            _ => {
                let header = &self.stream[self.cursor..self.cursor + 4];
                let needed = u32::from_le_bytes(
                    header
                        .try_into()
                        .expect("a 4-byte slice converts to a 4-byte array"),
                ) as usize;
                let start = self.cursor + 4;
                let Some(end) = start.checked_add(needed) else {
                    self.done = true;
                    return Some(Err(PathFrameError::LengthOverflow { at: start, needed }));
                };
                if end > self.stream.len() {
                    self.done = true;
                    return Some(Err(PathFrameError::TruncatedKey {
                        at: start,
                        needed,
                        available: self.stream.len() - start,
                    }));
                }
                self.cursor = end;
                Some(Ok(&self.stream[start..end]))
            }
        }
    }
}

/// Test seam for the exact stability grammar used by canonical paths.
#[doc(hidden)]
pub fn eval_stable_par_for_test(par: &Par) -> bool { eval_stable_par(par) }

pub struct PathMapCrateTypeMapper;

impl PathMapCrateTypeMapper {
    pub fn set_epathmap_to_rholang_set_pathmap(e_pathmap: &EPathMap) -> SetPathMapCreationResult {
        let entries = e_pathmap.entry_trie();
        SetPathMapCreationResult {
            map: entries.set_trie().clone(),
            connective_used: entries.any_connective_used() || e_pathmap.remainder.is_some(),
            locally_free: entries.union_locally_free().to_vec(),
        }
    }

    pub fn rholang_set_pathmap_to_set_epathmap(
        map: &RholangSetPathMap,
        connective_used: bool,
        locally_free: &[u8],
        remainder: Option<Var>,
    ) -> EPathMap {
        EPathMap::new(
            EntryTrie::adopt_trie(map),
            locally_free.to_vec(),
            connective_used,
            remainder,
        )
    }
}
