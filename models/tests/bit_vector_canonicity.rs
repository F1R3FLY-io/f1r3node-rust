//! # The `locally_free` bitset has ONE spelling of `∅`, and this file holds it
//!
//! ## The representation, before the law that depends on it
//!
//! A `locally_free` bitset in this port is **one byte per de Bruijn index** —
//! not a packed bitmap and not a list of indices. [`models::create_bit_vector`]
//! is `vec![0; max_index + 1]` followed by `bit_vector[index] = 1`, and
//! [`models::rust::utils::union`] is an element-wise `OR` over the two operands
//! padded to the longer length. **A member's identity IS its position.**
//!
//! ## ★★ The defect: `∅` had TWO spellings, and `Vec::is_empty()` split them
//!
//! ```text
//!   create_bit_vector(&[])  =  vec![0; 0 + 1]  =  [0]        length 1, all bits CLEAR
//!   Vec::new()              =  []                            length 0, all bits CLEAR
//! ```
//!
//! Both denote `∅`. `Vec::is_empty()` answers **length**, so it separates them:
//! `[].is_empty()` is `true` and `[0].is_empty()` is `false`. That is not a
//! cosmetic difference, because `locally_free.is_empty()` is read on production
//! paths — `rholang/src/rust/interpreter/matcher/fold_match.rs:103` and three
//! siblings in `fold_match` / `list_match` — where it is *intended* as a
//! set-emptiness query. Under a second spelling it stops being one.
//!
//! Verbatim from the gate that found it
//! (`rholang/src/rust/interpreter/util/mod.rs`, `binder_shift_law`):
//!
//! > `create_bit_vector(&[])` is `vec![0; 0 + 1]` = `[0]` — a length-one vector
//! > whose only byte is zero. That is a SECOND spelling of the empty set […] So
//! > the oracle renders `∅` as `[]`.
//!
//! ⚠ That gate worked *around* the defect with a private `bits()` helper. A law
//! spelled at each reader drifts; this file moves the law into the **producer**
//! so the wrong form cannot be spelled at all.
//!
//! ## The law
//!
//! ```math
//! \forall b \in \mathrm{BitSet}_{\text{canonical}} : \quad
//!     b = [\,] \;\lor\; b_{|b|-1} \neq 0
//! ```
//!
//! — *no canonical bitset ends in a clear byte*. Equivalently: the canonical
//! representative of a member set is the **shortest** byte string that spells
//! it, and the map from member sets to canonical bitsets is a **bijection**.
//! [`the_canonical_form_is_a_bijection_on_member_sets`] asserts exactly that,
//! over the whole lattice of member sets on `0..=5`, rather than on an example.
//!
//! ## Why this is a *producer* law and not a *reader* law
//!
//! There are two ways to make `is_empty()` sound. Adding
//! `while b.last() == Some(&0) { b.pop(); }` at each of the four readers is one;
//! it is four copies of one truth, and this repository has measured what happens
//! to a truth stored in four places (`docs/design/audits/
//! theta-depth-traversals-2026-07-26.md` §11.4: four copies of the converted-set
//! list, every copy drifted, two of them stale *within the hour* of being
//! reconciled, twice). The other way is to make the non-canonical form
//! unconstructible. This file takes the second, and [`the_bitset_construction_
//! sites_are_enumerated_from_source`] keeps the set of producers **derived from
//! the source text** rather than remembered.
//!
//! ## ⚠ What this does NOT cover, stated rather than left to be discovered
//!
//! 1. **Wire ingress.** `Par.locally_free` is proto tag 9, `bytes`. A peer may
//!    send `[0]` and both the prost and the bincode decoder will accept it
//!    verbatim — as they must, since narrowing a decoder's accept set is a fork.
//!    This is *agreement-preserving*: every node decoding those bytes gets the
//!    same non-canonical value and computes the same `is_empty()`, so it is not
//!    a divergence. It is a residual, and [`the_wire_ingress_residual_is_real`]
//!    exhibits it so that it is a measured fact rather than an assumption.
//!
//! 2. **`rholang`'s own producers.** Two exist and this crate cannot reach them.
//!    [`the_rholang_producers_are_named_with_their_verdicts`] records both, with
//!    the verdict *derived by executing the same arithmetic* rather than
//!    asserted:
//!    * `interpreter::util::filter_and_adjust_bitset` — a **suffix**; cannot
//!      create a trailing clear byte from a canonical input.
//!    * `interpreter::substitute_combine::set_bits_until` — a **prefix**, and
//!      ★ it **can**: `set_bits_until([0, 1], 1) = [0]`. That is this defect's
//!      one production-reachable sibling, and it is owned elsewhere.

use models::rust::utils::union;
use models::{canonical_bit_vector, create_bit_vector};

// ---------------------------------------------------------------------------
// helpers — the ORACLE works in member-set space, so it cannot share the defect
// ---------------------------------------------------------------------------

/// The member set a bitset denotes: the positions holding a non-zero byte.
///
/// ★ This is the semantics, and it is deliberately independent of length —
/// which is exactly why it can tell two spellings of one set apart from a
/// genuine difference.
fn members(bits: &[u8]) -> Vec<usize> {
    bits.iter()
        .enumerate()
        .filter(|(_, &b)| b != 0)
        .map(|(i, _)| i)
        .collect()
}

/// Every member set over `0..width`, as a `Vec` of index slices. `2^width` rows.
fn member_lattice(width: usize) -> Vec<Vec<usize>> {
    let mut rows = Vec::with_capacity(1usize << width);
    for mask in 0u32..(1u32 << width) {
        rows.push((0..width).filter(|i| mask & (1 << i) != 0).collect());
    }
    rows
}

/// The property the whole file is about.
fn is_canonical(bits: &[u8]) -> bool {
    bits.last() != Some(&0)
}

// ---------------------------------------------------------------------------
// §1 the law
// ---------------------------------------------------------------------------

/// ★★ THE DEFECT, as an assertion: `∅` has exactly one spelling.
#[test]
fn the_empty_set_has_exactly_one_spelling() {
    assert_eq!(
        create_bit_vector(&[]),
        Vec::<u8>::new(),
        "create_bit_vector(&[]) must be the EMPTY vector. `vec![0; max_index + 1]` with \
         `max_index = *indices.iter().max().unwrap_or(&0)` gives `[0]` for the empty slice — a \
         length-one vector whose only byte is clear, which denotes the same set as `[]` and \
         compares differently under `Vec::is_empty()`. `locally_free.is_empty()` is read as a \
         SET-EMPTINESS query on production paths (rholang matcher/fold_match.rs:103 and three \
         siblings), so two spellings make those four readers unsound."
    );

    // and the two spellings, if both existed, would disagree under the reader
    // that production actually uses.
    let second_spelling = vec![0u8];
    assert!(
        members(&second_spelling).is_empty(),
        "control: [0] denotes the empty member set"
    );
    assert!(
        !second_spelling.is_empty(),
        "control: [0] is NOT Vec-empty — this is precisely the disagreement, and it is why the \
         producer and not the reader is the place to fix it"
    );
}

/// The canonical form is a **bijection** from member sets to byte strings.
///
/// Two directions, both needed: distinct member sets get distinct bitsets
/// (injectivity — otherwise information is lost), and a bitset recovers its
/// member set (soundness — otherwise the canonicalisation is not
/// semantics-preserving).
#[test]
fn the_canonical_form_is_a_bijection_on_member_sets() {
    let lattice = member_lattice(6);
    assert_eq!(lattice.len(), 64, "control: the lattice on 0..6 has 2^6 rows");

    let mut seen: Vec<(Vec<usize>, Vec<u8>)> = Vec::with_capacity(lattice.len());
    for indices in &lattice {
        let bits = create_bit_vector(indices);

        assert!(
            is_canonical(&bits),
            "create_bit_vector({indices:?}) = {bits:?} ends in a clear byte"
        );
        assert_eq!(
            members(&bits),
            *indices,
            "create_bit_vector({indices:?}) = {bits:?} does not denote its own input"
        );
        assert_eq!(
            bits.is_empty(),
            indices.is_empty(),
            "Vec::is_empty() must answer SET-emptiness for {indices:?}"
        );

        for (other_indices, other_bits) in &seen {
            assert_ne!(
                bits, *other_bits,
                "canonicalisation is not injective: {indices:?} and {other_indices:?} both spell \
                 {bits:?}"
            );
        }
        seen.push((indices.clone(), bits));
    }
}

/// `canonical_bit_vector` is idempotent and semantics-preserving, including on
/// inputs that are already wrong — which is what makes it usable as a *repair*
/// at an ingress and not only as a discipline at a constructor.
#[test]
fn canonicalisation_is_idempotent_and_preserves_the_member_set() {
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        vec![0, 0],
        vec![0, 0, 0, 0, 0, 0, 0, 0],
        vec![1],
        vec![1, 0],
        vec![0, 1],
        vec![0, 1, 0, 0],
        vec![1, 1, 1],
        // ⚠ a non-zero byte that is not 1: the representation stores a BYTE per
        // index, and "set" means "non-zero", so 7 is a member just as 1 is.
        vec![7, 0],
        vec![0, 7, 0, 0, 0],
    ];

    for raw in cases {
        let once = canonical_bit_vector(raw.clone());
        let twice = canonical_bit_vector(once.clone());

        assert!(
            is_canonical(&once),
            "canonical_bit_vector({raw:?}) = {once:?} still ends in a clear byte"
        );
        assert_eq!(
            once, twice,
            "canonical_bit_vector is not idempotent on {raw:?}"
        );
        assert_eq!(
            members(&once),
            members(&raw),
            "canonical_bit_vector({raw:?}) changed the member set"
        );
        assert_eq!(
            once.is_empty(),
            members(&raw).is_empty(),
            "after canonicalisation, Vec::is_empty() must answer SET-emptiness for {raw:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// §2 the propagator
// ---------------------------------------------------------------------------

/// `union` over the FULL cross product of the lattice: canonical in, canonical
/// out, and the member set is the set union.
///
/// ⚠ `union` could not *create* a trailing clear byte from two canonical
/// operands even before this change — the longer operand's last byte is
/// non-zero and `x | 0 = x`. It could *propagate* one, and this test pins the
/// stronger property (canonical **whatever** it is given), which is what makes
/// the ingress repair above composable.
#[test]
fn union_is_canonical_on_the_full_cross_product() {
    let lattice = member_lattice(5);
    for a_idx in &lattice {
        for b_idx in &lattice {
            let a = create_bit_vector(a_idx);
            let b = create_bit_vector(b_idx);
            let u = union(a.clone(), b.clone());

            assert!(
                is_canonical(&u),
                "union({a:?}, {b:?}) = {u:?} ends in a clear byte"
            );

            let mut expected: Vec<usize> = a_idx.iter().chain(b_idx.iter()).copied().collect();
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(members(&u), expected, "union({a:?}, {b:?}) = {u:?}");
            assert_eq!(
                u.is_empty(),
                expected.is_empty(),
                "union({a:?}, {b:?}) = {u:?}: Vec::is_empty() must answer SET-emptiness"
            );
        }
    }
}

/// ★ `union` given a NON-canonical operand still returns canonical bytes. This
/// is the case the old implementation got wrong, and it is reachable from wire
/// ingress (see §4) rather than only from a constructor.
#[test]
fn union_canonicalises_a_non_canonical_operand() {
    assert_eq!(
        union(vec![0], vec![]),
        Vec::<u8>::new(),
        "union([0], []) must be [] — [0] denotes ∅ and so does [], and the OR of two empty sets \
         is the empty set"
    );
    assert_eq!(union(vec![0, 1, 0, 0], vec![]), vec![0u8, 1]);
    assert_eq!(union(vec![], vec![0, 0, 0]), Vec::<u8>::new());
    assert_eq!(union(vec![1, 0], vec![0, 0]), vec![1u8]);
}

// ---------------------------------------------------------------------------
// §3 the producer set, DERIVED from the source text
// ---------------------------------------------------------------------------

/// ★★ The enumeration is a **scan**, not a memory.
///
/// Every byte-per-index bitset in this crate is materialised by a `vec![0; n]`
/// of a computed length — that is the *only* way to make a vector with a
/// possibly-clear last byte, because every other construction here either
/// starts from `Vec::new()` or copies an existing bitset. So the set of
/// producers is exactly the set of `vec![0; …]` sites in `models/src`, and this
/// test reads them out of the source.
///
/// ⚠ A third site fails this test **naming itself**, which is the property a
/// hand-maintained list does not have. `docs/design/audits/
/// theta-depth-traversals-2026-07-26.md` §11.4 is the measured precedent: one
/// list, four copies, every copy drifted.
#[test]
fn the_bitset_construction_sites_are_enumerated_from_source() {
    /// (file, the function that owns the site, whether it can emit a trailing
    /// clear byte AFTER this change)
    const ATTRIBUTED: &[(&str, &str)] = &[
        ("src/lib.rs", "create_bit_vector"),
        ("src/rust/utils.rs", "union"),
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<(String, usize, String)> = Vec::new();

    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            for (lineno, line) in text.lines().enumerate() {
                let trimmed = line.trim_start();
                // ⚠ A COMMENT that quotes the construction is not a
                // construction. This exclusion is load-bearing and it is the
                // reason the scan was shown red first: the very documentation
                // that explains the defect quotes `vec![0; max_index + 1]` four
                // times, and without this line the scan reported 6 sites for 2
                // producers — a false positive that would have been "fixed" by
                // padding the attribution table, i.e. by making the gate lie.
                if trimmed.starts_with("//") {
                    continue;
                }
                // `vec![0; …]` / `vec![0u8; …]` — a zero-filled vector of
                // computed length. A literal length (`vec![0u8; 32]`, a hash
                // buffer) is not a bitset, but including it costs only an
                // attribution row and excluding it would need a heuristic.
                if line.contains("vec![0;") || line.contains("vec![0u8;") {
                    let rel = path
                        .strip_prefix(root)
                        .expect("under manifest dir")
                        .to_string_lossy()
                        .replace('\\', "/");
                    found.push((rel, lineno + 1, line.trim().to_string()));
                }
            }
        }
    }

    // NON-VACUITY: the scan must actually find the sites we know exist. A scan
    // that matches nothing passes every "each hit is attributed" check.
    assert_eq!(
        found.len(),
        ATTRIBUTED.len(),
        "the zero-filled-vector scan of models/src found {} site(s), but {} are attributed.\n\
         found: {found:#?}\n\
         attributed: {ATTRIBUTED:#?}\n\
         A NEW site means a new bitset producer: give it a row here and prove it cannot emit a \
         trailing clear byte. A MISSING site means the scan broke — which would make every other \
         assertion in this test vacuous.",
        found.len(),
        ATTRIBUTED.len()
    );

    for (file, owner) in ATTRIBUTED {
        assert!(
            found.iter().any(|(f, _, _)| f == file),
            "attributed producer {owner} in {file} has no `vec![0; …]` site — the attribution is \
             stale"
        );
    }
}

// ---------------------------------------------------------------------------
// §4 what is NOT covered — exhibited, not assumed
// ---------------------------------------------------------------------------

/// ⚠ The wire accepts a non-canonical bitset, and it must.
///
/// This is not a defect to fix here: narrowing what a decoder accepts is a
/// consensus-visible change of the accept set, and every node decoding the same
/// bytes reaches the same non-canonical value, so there is no disagreement to
/// have. It is exhibited so that the residual is a **measured fact** rather than
/// an assumption a later reader has to re-derive.
#[test]
fn the_wire_ingress_residual_is_real() {
    use models::rhoapi::Par;
    use prost::Message;

    let hostile = Par {
        locally_free: vec![0],
        ..Default::default()
    };
    let bytes = hostile.encode_to_vec();
    let back = Par::decode(&bytes[..]).expect("a Par carrying locally_free = [0] decodes");

    assert_eq!(
        back.locally_free,
        vec![0u8],
        "the decoder must reproduce the bytes it was given"
    );
    assert!(
        !back.locally_free.is_empty(),
        "★ THE RESIDUAL: a peer-supplied [0] survives ingress, so `is_empty()` answers `false` \
         for a term whose free-variable set is empty. Agreement-preserving (every node decoding \
         these bytes agrees), and therefore not a fork — but it is the reason the producer law \
         is a floor and not a totality claim."
    );
    assert!(
        members(&back.locally_free).is_empty(),
        "control: the decoded value still DENOTES the empty set"
    );
}

/// The two `rholang` producers, named with their verdicts **computed here**
/// rather than transcribed.
///
/// This crate cannot call them (`models` is below `rholang` in the dependency
/// order — a dev-dependency here would be a cycle), so the arithmetic each one
/// performs is re-executed locally against the same inputs. A local model that
/// disagreed with the real function would be worthless; what makes this
/// worthwhile is that the two functions are each *three lines*, quoted in full
/// beside their model, and the verdicts they produce are opposite — which is
/// the finding.
#[test]
fn the_rholang_producers_are_named_with_their_verdicts() {
    // rholang/src/rust/interpreter/util/mod.rs — `filter_and_adjust_bitset`:
    //     match bound_count >= bitset.len() {
    //         true  => Vec::new(),
    //         false => { bitset.drain(..bound_count); bitset }
    //     }
    fn filter_and_adjust_bitset(mut bitset: Vec<u8>, bound_count: usize) -> Vec<u8> {
        match bound_count >= bitset.len() {
            true => Vec::new(),
            false => {
                bitset.drain(..bound_count);
                bitset
            }
        }
    }

    // rholang/src/rust/interpreter/substitute_combine.rs:63 — `set_bits_until`:
    //     if until <= 0 { return Vec::new(); }
    //     bits.into_iter().take(until as usize).collect()
    fn set_bits_until(bits: Vec<u8>, until: i32) -> Vec<u8> {
        if until <= 0 {
            return Vec::new();
        }
        bits.into_iter().take(until as usize).collect()
    }

    // SUFFIX: safe. Over the whole lattice on 0..5 and every bound count, a
    // canonical input yields a canonical output — the surviving last byte is
    // the input's, which is non-zero.
    let mut suffix_witnessed = 0usize;
    for indices in member_lattice(5) {
        let bits = create_bit_vector(&indices);
        for bound_count in 0..=6usize {
            let out = filter_and_adjust_bitset(bits.clone(), bound_count);
            assert!(
                is_canonical(&out),
                "filter_and_adjust_bitset({bits:?}, {bound_count}) = {out:?} ends in a clear byte"
            );
            suffix_witnessed += 1;
        }
    }
    assert_eq!(
        suffix_witnessed,
        32 * 7,
        "non-vacuity: the suffix verdict must be witnessed on the whole lattice"
    );

    // PREFIX: ★ NOT safe, and this is the sibling.
    assert_eq!(
        set_bits_until(vec![0, 1], 1),
        vec![0u8],
        "★ `set_bits_until` truncates at a POSITION, so it can cut away the only set byte and \
         leave clear bytes behind. `[0, 1]` is the canonical spelling of {{1}}; truncating at 1 \
         leaves `[0]`, the non-canonical spelling of ∅. This is the one PRODUCTION-REACHABLE \
         sibling of the `create_bit_vector(&[])` defect, it lives in \
         `rholang/src/rust/interpreter/substitute_combine.rs:63`, and `rholang/**` is owned \
         elsewhere. Fixing it is `canonical_bit_vector(...)` around the return — this crate now \
         exports it."
    );

    // and the smallest witness that it defeats the reader the fix exists for
    let cut = set_bits_until(vec![0, 1], 1);
    assert!(
        !cut.is_empty(),
        "the truncated value is not Vec-empty …"
    );
    assert!(
        members(&cut).is_empty(),
        "… while denoting ∅ — which is exactly the disagreement `fold_match.rs:103` reads"
    );
    assert_eq!(
        canonical_bit_vector(cut),
        Vec::<u8>::new(),
        "and `canonical_bit_vector` is the one-call repair"
    );
}
