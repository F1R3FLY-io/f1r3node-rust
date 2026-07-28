//! Integration layer between Rholang Par types and the PathMap crate.
//!
//! EPathMap wire W2b-1 (the global trie re-key): trie keys are now the
//! CANONICAL PATH CODEC bytes (`canonical_path::encode_trie_path`), not the
//! former `ParToSExpr` + `SExpr::encode` segments joined with a `0xFF`
//! separator. The codec is capless, injective, and prefix-free, with the
//! `0x0F` escape arm for non-`eval_stable` trie entries (R3F-2) and a depth
//! limit of 32 collection levels (R3F-7). Consequences of the flip
//! (design §3.1-§3.2): trie keys re-key globally, formerly-colliding paths
//! (`"(expr)"`/`"Nil"` degenerate keys) SEPARATE (a D-2 disclosed fix), and
//! the `0xFF`-separator machinery (this file's flatten, the
//! `pathmap_native_query` separator special-case, `SExpr::decode`
//! reconstruction) retires.
//!
//! `par_to_path` now returns the PER-ELEMENT codec segments (each element's
//! `encode_trie_segment`); full trie keys are built with [`segments_to_key`]
//! (concatenation + the split-list `0x00` terminator) or, for a whole entry
//! Par, directly with `canonical_path::encode_trie_path`. Callers that store
//! `current_path` segments are unaffected by the return type; only the
//! key-BUILD sites (the former `0xFF` flatten) change to the codec
//! concatenation.

use pathmap::PathMap;

use crate::rhoapi::{Par, Var};
use crate::rust::canonical_path::{
    decode_trie_path, encode_trie_path, encode_trie_segment, split_carrier_list, tag,
    takes_split_arm,
};

/// Type alias for our standard use case: PathMap from bytes to Rholang Par.
pub type RholangPathMap = PathMap<Par>;

/// WHICH ENTRY an `EZipper` cursor addresses — the split/bare discriminator
/// that `current_path` alone cannot carry.
///
/// The canonical path codec has TWO top-level arms (`canonical_path.rs` §1.3):
/// a ground list splits into per-element segments plus a `0x00` terminator,
/// and anything else is one bare segment with NO terminator. `current_path`
/// stores the per-element segments of either arm, so the bare element `1` and
/// the singleton list `[1]` give the SAME segment vector — while being
/// DIFFERENT entries, under keys `03 02` and `03 02 00`, which one map may
/// hold at the same time. The cursor is `(segments, kind)`, and
/// [`cursor_entry_key`] is the key it names:
///
/// ```text
/// key(cursor) = concat(segments) ++ (0x00 iff the cursor is a split frame)
/// ```
///
/// This mirrors `EZipper.cursor_kind` (`RhoTypes.proto`), whose wire values are
/// pinned by [`CursorKind::from_wire`] / [`CursorKind::to_wire`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum CursorKind {
    /// A SPLIT path frame of `segments.len()` elements: the key is
    /// `concat(segments) ++ 0x00`.
    ///
    /// The root cursor is this with zero segments — key `0x00`, the empty
    /// list — and so is the cursor of `readZipperAt`/`descendTo` given a
    /// ground-LIST argument. It is the DEFAULT because it is proto value 0,
    /// so an `EZipper` serialized before `cursor_kind` existed decodes to
    /// exactly the behaviour that preceded it.
    #[default]
    Split,
    /// A BARE element: the key is `concat(segments)`, with no terminator.
    /// From `readZipperAt`/`descendTo` given a non-list argument, and from an
    /// enumeration step that landed on a bare entry.
    Bare,
    /// An element-PREFIX at which the cursor has NOT chosen between the bare
    /// entry (`concat(segments)`) and the split entry
    /// (`concat(segments) ++ 0x00`).
    ///
    /// Produced by every move that advances by one CHILD SEGMENT
    /// (`descendFirst`, `descendIndexedBranch`, `toNextSibling`,
    /// `toPrevSibling`, `ascendOne`, `ascend`): a segment move lands on an
    /// element boundary and carries no information about which arm the entry
    /// there took. [`cursor_entry_key`] resolves it to the SHORTEST key
    /// PRESENT in the map — bare first, split otherwise — which is
    /// indistinguishable from [`CursorKind::Split`] on any map holding no bare
    /// entries, and is why navigation does not move for the ground-list corpus.
    Prefix,
}

impl CursorKind {
    /// Decode the `EZipper.cursor_kind` wire value.
    ///
    /// Returns `None` for an unrecognized value rather than coercing it: a
    /// cursor whose kind cannot be read names no entry, and silently treating
    /// it as `Split` is precisely the guess this type exists to remove.
    pub fn from_wire(value: u32) -> Option<CursorKind> {
        match value {
            0 => Some(CursorKind::Split),
            1 => Some(CursorKind::Bare),
            2 => Some(CursorKind::Prefix),
            _ => None,
        }
    }

    /// The `EZipper.cursor_kind` wire value.
    pub fn to_wire(self) -> u32 {
        match self {
            CursorKind::Split => 0,
            CursorKind::Bare => 1,
            CursorKind::Prefix => 2,
        }
    }

    /// The kind of a cursor built from the WHOLE path `par` — the arm the
    /// codec itself takes for it, so that
    /// `cursor_entry_key(par_to_path(par), CursorKind::of(par), map)` is
    /// `encode_trie_path(par)` for EVERY Par (the cursor round-trip law).
    pub fn of(par: &Par) -> CursorKind {
        match takes_split_arm(par) {
            true => CursorKind::Split,
            false => CursorKind::Bare,
        }
    }
}

/// The per-element codec SEGMENTS of a path Par (W2b-1).
///
/// - A split-arm entry (a ground `EList` carrier, [`takes_split_arm`]) yields
///   one segment per element (`encode_trie_segment` of each), so a
///   `current_path` of these segments keeps the segment-count ==
///   element-count invariant the zipper relies on
///   (`existing_list.ps[..current_path.len()]` indexing, `starts_with`
///   prefix matching).
/// - Any other Par yields a single segment (`encode_trie_segment(par)`).
///
/// The FULL trie key of the entry is [`cursor_entry_key`] over these segments
/// at [`CursorKind::of`] the Par — equivalently `encode_trie_path` of it.
///
/// ⚠ The split test is [`takes_split_arm`], i.e. the codec's OWN
/// `split_carrier_list`, and not a local re-derivation. An earlier local test
/// inspected only `exprs` and the `EList`'s own metadata, so a list carrier
/// that ALSO carried (say) a send split here into per-element segments while
/// the codec gave it the `0x0F` escape arm — under which the cursor's segments
/// do not concatenate to the key under ANY kind, and the round-trip law cannot
/// hold. Pinned by
/// `canonical_path::tests::par_to_path_agrees_with_the_codec_about_which_pars_split`.
pub fn par_to_path(par: &Par) -> Vec<Vec<u8>> {
    path_elements(par).iter().map(encode_trie_segment).collect()
}

/// The ELEMENTS of the path `par` names, as the codec splits it — the element
/// view of which [`par_to_path`] is the segment view. By construction
///
/// ```text
/// par_to_path(p) == path_elements(p).map(encode_trie_segment)
/// path_elements(p).len() == par_to_path(p).len()   ==  the path's LENGTH
/// ```
///
/// so anything that needs to know *how many elements a path has*, or *what its
/// tail is*, asks here instead of re-deriving the split. A split-arm carrier
/// ([`takes_split_arm`]) contributes its own list elements; **every other Par
/// contributes ITSELF, as a path of length one** — the bare arm and the `0x0F`
/// escape arm are both one segment.
///
/// # ⚠ Why re-deriving the split is a defect and not a style question
///
/// The tempting local test is `par.exprs.first()` matching an `EListBody`. That
/// is STRICTLY MORE PERMISSIVE than the codec's `split_carrier_list`, which
/// also requires the carrier to hold no sends/receives/news/matches/bundles/
/// connectives/conditionals/unforgeables, no par-level `locally_free`, and a
/// list whose own metadata is at ground defaults. A Par carrying BOTH a list
/// and a send — `[1,2] | @"d"!(3)`, which a program can put in a map by sending
/// it over a channel — passes the local test and fails the codec's, so the two
/// disagree about how long its path is: the local test says 2, the trie says 1
/// (it is one escaped segment). Every such disagreement is a wrong answer about
/// a path. This is the same divergence that cost `setSubtrie` its bare source
/// entries and `dropHead` its non-list entries.
pub fn path_elements(par: &Par) -> &[Par] {
    match split_carrier_list(par) {
        Some(list) => &list.ps,
        None => std::slice::from_ref(par),
    }
}

/// The ENTRY key a cursor `(segments, kind)` addresses in `map`.
///
/// This is THE cursor→key rule, and the only place the split/bare
/// discriminator is spent. `map` is consulted for [`CursorKind::Prefix`] only.
///
/// ```text
/// Split   concat(segments) ++ 0x00
/// Bare    concat(segments)
/// Prefix  concat(segments)          if the map holds a value there
///         concat(segments) ++ 0x00  otherwise
/// ```
///
/// PREFIX queries — `leafCount`, `childCount`, `getSubtrie`, `pathExists`,
/// `descendFirst`, `restriction`, `prunePath`, `removeBranches` — do NOT use
/// this. They ask about the BRANCH at the cursor, which is
/// `segments_to_key(segments, false)` under every kind, so they are unaffected
/// by the discriminator and their bytes never move. See `pathmap_native_query`
/// for the branch-vs-entry semantics that follow from this split.
///
/// # ★ The read-side image invariant, checked here
///
/// This is the single point every ENTRY key passes through, so it is where the
/// dual of the write-side entry invariant ([`trie_entry_divergences`]) is
/// enforced: the key it returns is in the image of `encode_trie_path`
/// ([`entry_key_is_in_codec_image`]). See that function for why a key outside
/// the image is a defect and not merely a miss.
pub fn cursor_entry_key(
    segments: &[Vec<u8>],
    kind: CursorKind,
    map: &RholangPathMap,
) -> Vec<u8> {
    let bare = segments_to_key(segments, false);
    let key = match kind {
        CursorKind::Bare => bare,
        CursorKind::Split => {
            let mut split = bare;
            split.push(tag::TERM);
            split
        }
        // Shortest key PRESENT. A proper prefix sorts before what it prefixes,
        // so "shortest present" is also "first in trie order" — the same tie
        // -break the enumeration walk would make.
        CursorKind::Prefix => match map.get(&bare) {
            Some(_) => bare,
            None => {
                let mut split = bare;
                split.push(tag::TERM);
                split
            }
        },
    };
    debug_assert!(
        entry_key_is_in_codec_image(&key),
        "cursor_entry_key built an entry key that is in the image of no Par, \
         so it can never hit on any map: kind = {kind:?}, {} segment(s), \
         key = {key:02x?}, decode = {:?}",
        segments.len(),
        decode_trie_path(&key).err()
    );
    key
}

/// ★ **THE READ-SIDE IMAGE INVARIANT**, stated executably:
///
/// ```math
/// \forall\ \text{entry keys } k \text{ a reader constructs} .\quad
///   \mathrm{decode\_trie\_path}(k) \in \mathrm{Ok}
/// ```
///
/// # Why a key outside the image is a defect and not a miss
///
/// Every value in a `RholangPathMap` is filed under `encode_trie_path` of
/// itself ([`create_pathmap_from_elements`]), and `decode_trie_path` accepts
/// EXACTLY the encoder's image (`canonical_path.rs`: "decode-accepts ≡
/// encoder-image"). So a key the decoder rejects is a key no entry can ever be
/// stored under, and `map.get` with it **misses on every map, whatever the map
/// holds** — the lookup is not answering "absent", it is asking an
/// unanswerable question. Such a key is therefore never a fact about the data;
/// it is always a defect in the reader.
///
/// The two shapes that are rejected, and what produces them:
///
/// | rejected key | `CodecError` | what builds it |
/// |---|---|---|
/// | `concat(segments)`, `segments.len() ≥ 2` | `UnterminatedMultiSegment` | a `Bare` cursor below depth 1 — **defect #108** |
/// | `[]` (the empty key) | `EmptyPath` | a `Bare` cursor with no segments |
///
/// A bare entry's key is exactly ONE segment (`encode_trie_path`'s bare arm
/// emits one segment and no terminator), so [`CursorKind::Bare`] is inhabited
/// at exactly one cursor depth — 1 — and is nonsense at every other. That is
/// the whole content of the invariant, and it is what
/// [`composed_cursor_kind`] exists to respect.
///
/// # Relationship to the write-side invariant
///
/// [`trie_entry_divergences`] checks PRODUCERS (`∀ (k,v) ∈ m . enc(v) = k`);
/// this checks CONSUMERS. Neither implies the other, and #108 is the proof: the
/// trie was perfect and every write-side check passed, while the reader asked
/// for a key in the image of no Par.
///
/// # Cost
///
/// One `decode_trie_path` per entry-key construction. It is called from
/// production code only inside a `debug_assert!` in [`cursor_entry_key`], so
/// release builds (consensus nodes) do not pay for it, while the whole test
/// corpus runs with it live.
pub fn entry_key_is_in_codec_image(key: &[u8]) -> bool { decode_trie_path(key).is_ok() }

/// ★ **THE COMPOSITION LAW** — the arm the cursor `cursor ⌢ path_par` takes,
/// in the ONE place every composer spends it.
///
/// ```math
/// \mathrm{kind}(\mathrm{cursor} \frown p) \;=\;
///   \begin{cases}
///     \mathrm{CursorKind::of}(p) & \text{if } \mathrm{cursor} = \varepsilon,\\
///     \mathrm{Split}             & \text{otherwise.}
///   \end{cases}
/// ```
///
/// # Why the second arm is forced
///
/// `encode_trie_path` emits exactly two shapes (`canonical_path.rs`
/// `push_path_ops`): the split arm is `concat(per-element segments) ++ 0x00`,
/// and the bare arm is ONE segment with no terminator. **A bare entry's key is
/// therefore exactly one segment long**, so a cursor of `n` segments can name a
/// bare entry only when `n = 1`; at `n ≥ 2` the unterminated concatenation is
/// in the image of no Par ([`entry_key_is_in_codec_image`]) and names nothing
/// at all.
///
/// Below the root the cursor contributes `n ≥ 1` segments. If the relative path
/// contributes at least one more, the composed path has `n ≥ 2` and the
/// terminated key is the only key it can have. If it contributes none — which
/// happens for exactly one Par, the empty list `[]`, since [`par_to_path`]
/// returns one segment for every non-split Par and `list.ps.len()` for a split
/// one — then `CursorKind::of([])` is `Split` already, and the two arms agree.
/// So below the root the composed arm is `Split` unconditionally.
///
/// # Why the FIRST arm is not the same rule
///
/// At the root the cursor contributes nothing, so the composed path IS the
/// argument and takes the argument's own arm — which is how
/// `map.atPath(5)` names the bare entry `5` while `map.atPath([5])` names the
/// singleton list, two entries one map may hold at once.
///
/// # ⚠ The defect this replaced (#108)
///
/// All three composers — `entry_key_at` (`atPath`), `descendTo` in `reduce.rs`,
/// and `descendTo` in `fused_pathmap_chain.rs` — each carried their own copy of
/// the law, and all three copies said *"the composed cursor takes the
/// ARGUMENT's arm"*. That is true only at the root. Below it,
/// `readZipperAt(["a"]).atPath(5)` built `enc("a") ‖ enc(5)` with no
/// terminator and could not hit `["a",5]` — or anything else. The law now
/// exists once; a fourth composer inherits it rather than re-deriving it.
pub fn composed_cursor_kind(cursor: &[Vec<u8>], path_par: &Par) -> CursorKind {
    match cursor.is_empty() {
        true => CursorKind::of(path_par),
        false => CursorKind::Split,
    }
}

/// Build a trie key from per-element codec segments (W2b-1): concatenation,
/// with the split-list terminator (`0x00`) appended iff `terminate`.
///
/// - A FULL entry key (getLeaf/setLeaf/removeLeaf exact addressing) passes
///   `terminate = true` — matching `encode_trie_path` of the list path.
/// - A PREFIX key (getSubtrie/childCount/descend/pathExists) passes
///   `terminate = false`; codec segments are prefix-free, so the raw
///   concatenation is a clean trie prefix boundary.
pub fn segments_to_key(segments: &[Vec<u8>], terminate: bool) -> Vec<u8> {
    let total: usize = segments.iter().map(|s| s.len()).sum::<usize>() + terminate as usize;
    let mut key = Vec::with_capacity(total);
    for segment in segments {
        key.extend_from_slice(segment);
    }
    if terminate {
        key.push(tag::TERM);
    }
    key
}

/// The trie key of the ENTRY named by `path_par`, as reached from a cursor
/// whose per-element segments are `cursor`.
///
/// # Why this exists (the bare-element key defect, read side)
///
/// `segments_to_key(par_to_path(p), true)` appends the split-list terminator
/// UNCONDITIONALLY, so for a bare (non-list) `p` it produces
/// `encode_trie_path([p])` — the key of the SINGLETON LIST, a valid canonical
/// key naming a DIFFERENT element. The read is not a miss; it is a wrong
/// answer. (Pinned by `canonical_path::tests::bare_and_singleton_list_are_distinct_entries`.)
///
/// The information needed to avoid the guess is available whenever the caller
/// holds the whole path Par, and this function is the ONE place that spends it.
///
/// * **At the root** (`cursor` empty) the argument IS the whole path, so the
///   key is [`crate::rust::canonical_path::encode_trie_path`] of it — bit for
///   bit the key `create_pathmap_from_elements` inserted the entry under,
///   split arm or bare arm or `0x0F` escape arm, with NO reconstruction. For a
///   split-form Par this is byte-identical to the old expression (the two
///   agree exactly on the split arm), so the ground-LIST corpus does not move.
///
/// * **Below the root** the argument is a RELATIVE descent: its elements extend
///   the cursor's, and the composed cursor's arm is [`composed_cursor_kind`] —
///   `Split`, because a composed path of two or more elements is a LIST and has
///   no bare form. `readZipperAt(["a"]).atPath(5)` therefore names `["a",5]`,
///   the same entry `readZipper().atPath(["a",5])` names from the root, and
///   that agreement is what makes a cursor a position rather than a mode.
///
/// # ⚠ The bare-element key defect, read side, BELOW the root (#108)
///
/// This arm used to pass the ARGUMENT's own arm as the composed cursor's arm.
/// For a bare argument below the root that produced an UNTERMINATED
/// multi-segment key — `enc("a") ‖ enc(5)` — which `decode_trie_path` rejects
/// as `UnterminatedMultiSegment` and which is therefore in the image of no Par:
/// the lookup could not hit on ANY map. The three rows that separate the arms
/// are pinned together in
/// `rholang/tests/relative_path_composition_spec.rs`, and the key's membership
/// of the codec image is checked inside [`cursor_entry_key`].
pub fn entry_key_at(cursor: &[Vec<u8>], path_par: &Par, map: &RholangPathMap) -> Vec<u8> {
    match cursor.is_empty() {
        // ZERO AMBIGUITY: the reader has the Par, so it can ask the codec.
        true => encode_trie_path(path_par),
        false => {
            let relative = par_to_path(path_par);
            let mut segments = Vec::with_capacity(cursor.len() + relative.len());
            segments.extend_from_slice(cursor);
            segments.extend(relative);
            cursor_entry_key(&segments, composed_cursor_kind(cursor, path_par), map)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE TRIE ENTRY INVARIANT
// ─────────────────────────────────────────────────────────────────────────────

/// One trie entry whose VALUE does not encode to the KEY it is stored under —
/// a violation of the entry invariant. See [`trie_entry_divergences`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrieEntryDivergence {
    /// The key the entry is stored under — what every KEY-side reader sees.
    pub key: Vec<u8>,
    /// `encode_trie_path(value)` — the key the entry's VALUE claims, i.e. the
    /// key a VALUE-side reader's answer would be re-inserted under.
    pub value_key: Vec<u8>,
    /// The stored value, for diagnosis.
    pub value: Par,
}

/// ★ **THE TRIE ENTRY INVARIANT**, stated executably:
///
/// ```math
/// \forall (k, v) \in m .\quad \mathrm{encode\_trie\_path}(v) = k
/// ```
///
/// The returned vector is EMPTY iff the invariant holds. Each element names one
/// violating entry.
///
/// # Why this invariant is load-bearing
///
/// A `RholangPathMap` is not a general map: it is a *set of Par entries indexed
/// by their own codec path*. [`create_pathmap_from_elements`] — the sole
/// construction site for a map that came from a program — writes
/// `map.insert(encode_trie_path(par), par.clone())`, so **the value is a
/// redundant mirror of the key**. Everything downstream is built on that
/// redundancy, and the tree contains TWO INDEPENDENT READERS that exploit it in
/// OPPOSITE directions:
///
/// | reader | reads | used by |
/// |---|---|---|
/// | [`crate::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap`] | the **values** (`iter()`) | every `EPathMap`-returning method in the reducer |
/// | `pathmap_crate_type_mapper::canonical_ps_from_trie` | the **keys**, via `decode_trie_path` (`to_next_val()`) | the serde / event-hash preimage, `canonicalize_ground_epathmap` |
///
/// The two agree on every map **exactly when this invariant holds**, and
/// nothing else makes them agree. So a producer that writes a value which does
/// not encode to its key does not merely store an odd pair — it makes the
/// reducer's answer and the consensus event hash disagree about what the map
/// contains.
///
/// The failure is worse than a mismatch, because the value-side reader is
/// *lossy under re-insertion*. Distinct keys `k₁ ≠ k₂` carrying the SAME value
/// `v` survive `rholang_pathmap_to_e_pathmap` as two identical `ps` entries;
/// the next `e_pathmap_to_rholang_pathmap` re-keys both to
/// `encode_trie_path(v)` and the trie collapses them into one. **Entries are
/// silently lost.** (Witnessed by `zz`-free fixtures in
/// `models/tests/pathmap_integration_tests.rs`.)
///
/// # The root-key corollary (why an empty key is always a divergence)
///
/// `encode_trie_path` emits at least one byte for every Par — the bare arm
/// emits a tag, the split arm emits the `0x00` terminator — so `[]` is in the
/// image of no Par and a value stored at the EMPTY (root) key is *necessarily*
/// a divergence. That is not a technicality: `PathMap::iter()` YIELDS the root
/// value while `ZipperIteration::to_next_val()` SKIPS it, so a root value is
/// kept by the value-side reader and dropped by the key-side reader. Today
/// nothing can create one (every producer keys through the codec); this
/// function is what makes that a checked fact rather than an assumption.
///
/// # Cost
///
/// One `encode_trie_path` per entry — the same order as the `value.clone()` the
/// converter already performs per entry. It is called from production code only
/// under `#[cfg(debug_assertions)]`; release builds (consensus nodes) do not
/// pay for it.
pub fn trie_entry_divergences(map: &RholangPathMap) -> Vec<TrieEntryDivergence> {
    let mut divergences = Vec::new();
    for (key, value) in map.iter() {
        let value_key = encode_trie_path(value);
        if value_key != key {
            divergences.push(TrieEntryDivergence {
                key: key.to_vec(),
                value_key,
                value: value.clone(),
            });
        }
    }
    divergences
}

/// A one-line-per-entry rendering of [`trie_entry_divergences`], for assertion
/// messages: the key the entry is filed under, and the key its value claims.
pub fn render_trie_entry_divergences(divergences: &[TrieEntryDivergence]) -> String {
    let mut out = String::new();
    for divergence in divergences {
        out.push_str(&format!(
            "\n  stored under {:02x?}\n  value encodes to {:02x?}\n  value = {:?}\n",
            divergence.key, divergence.value_key, divergence.value
        ));
    }
    out
}

/// Convenience return type—including the constructed map and related Rholang metadata.
pub struct PathMapCreationResult {
    pub map: RholangPathMap,
    pub connective_used: bool,
    pub locally_free: Vec<u8>,
}

/// Construct a RholangPathMap from a list of Par elements and an optional remainder.
/// This mirrors what the normalizer does when producing EPathMap from parsed elements.
///
/// W2b-1: each entry's key is `canonical_path::encode_trie_path(par)` — the
/// capless, injective, prefix-free codec path (split form for ground lists,
/// bare form otherwise, `0x0F` escape for non-`eval_stable` entries). This
/// replaces the former `par_to_path` + `0xFF` flatten; formerly-colliding
/// degenerate keys now separate.
pub fn create_pathmap_from_elements(
    elements: &[Par],
    remainder: Option<Var>,
) -> PathMapCreationResult {
    let mut map = RholangPathMap::new();
    let mut connective_used = false;
    let mut locally_free = Vec::new();

    for par in elements {
        // Update connective metadata
        if par.connective_used {
            connective_used = true;
        }
        locally_free = crate::rust::utils::union(locally_free.clone(), par.locally_free.clone());

        // The codec trie key (capless, injective, prefix-free). No separator
        // — the path is self-delimiting.
        let key = encode_trie_path(par);
        map.insert(key, par.clone());
    }

    if remainder.is_some() {
        connective_used = true;
    }

    PathMapCreationResult {
        map,
        connective_used,
        locally_free,
    }
}
