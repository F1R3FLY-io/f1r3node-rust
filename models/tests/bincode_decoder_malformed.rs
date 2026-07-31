//! # The MALFORMED-INPUT differential — pinning the REJECTION SET
//!
//! ## Why this file is the one that must not be skipped
//!
//! A decoder has two halves. The half everyone tests is "does it decode valid
//! input correctly". The half that decides whether a network stays converged is
//! **which inputs it refuses**:
//!
//! > A node that accepts a byte string another node rejects **forks**.
//!
//! Cold-store bytes arrive from peers through `rspace_importer`, which writes
//! them to LMDB without deep-decoding them. So the bytes a node reads back are
//! not necessarily bytes it produced, and "well-formed" is not an assumption
//! the decoder is entitled to make. If the machine accepted a truncated datum
//! that the derived decoder rejected — or rejected one it accepted — two nodes
//! would disagree about the contents of the same trie leaf.
//!
//! ## What is asserted, and what deliberately is not
//!
//! `Ok`/`Err` **agreement**, and on `Ok`, **value** equality. NOT message
//! equality: bincode's errors are `Box<ErrorKind>` with prose `Display` text,
//! and pinning that text would be pinning an implementation detail of a third-
//! party crate rather than the property that matters. The error *kinds* are
//! documented as a correspondence table in
//! `models/src/rust/rholang/bincode_decoder.rs` §6.
//!
//! ## The mutation families
//!
//! Each family targets one thing the derived decoder is known to reject, and is
//! applied to **every** encoding in the corpus:
//!
//! ```text
//!   family                    what it probes
//!   ───────────────────────── ─────────────────────────────────────────────
//!   truncate at every offset  UnexpectedEof at every position in the stream —
//!                             the exhaustive one, and the only family that
//!                             cannot miss a resume point
//!   bool byte -> 2            InvalidBoolEncoding
//!   Option tag -> 2           InvalidTagEncoding (a DIFFERENT error from the
//!                             bool case, though both read one byte)
//!   variant index -> N        out-of-range oneof index
//!   u64 length -> usize::MAX  the pre-allocation hazard: without serde's
//!                             `size_hint::cautious` cap this is an OOM ABORT
//!                             on one side and a clean Err on the other
//!   append trailing bytes     allow_trailing_bytes: both must ACCEPT
//!   every single-byte flip    a broad sweep over short encodings
//! ```
//!
//! The last three families cannot know where a `bool`, a tag or a length
//! actually sits without re-implementing the parser — so instead of guessing,
//! they apply the mutation at **every** byte offset and let agreement do the
//! work. A mutation that lands on an unrelated byte still has to produce the
//! same disposition on both sides, which is exactly the property under test.

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::Deserialize;

mod par_corpus;
use par_corpus as corpus;

/// The one assertion this file makes, everywhere.
///
/// `hits` counts how many mutations produced `Err` on BOTH sides, so a family
/// that silently stopped perturbing anything can be caught by its caller
/// instead of reporting a comfortable pass.
fn assert_agree<T>(label: &str, bytes: &[u8], rejections: &mut usize)
where
    T: ColdStoreDecode + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let oracle: Result<T, _> = bincode::deserialize(bytes);
    let machine = T::cold_decode(bytes);
    match (oracle, machine) {
        (Ok(a), Ok(b)) => assert!(
            a == b,
            "{label}: BOTH accepted, but produced DIFFERENT values. The cold-store \
             decoder and the derived decoder must recognise the same language."
        ),
        (Err(_), Err(_)) => *rejections += 1,
        (Ok(a), Err(e)) => panic!(
            "{label}: the derived decoder ACCEPTED this byte string and the machine \
             REJECTED it ({e}). This narrows the accepted language — a node running \
             the machine would refuse state its peers accept.\n  value: {a:?}"
        ),
        (Err(_), Ok(b)) => panic!(
            "{label}: the machine ACCEPTED a byte string the derived decoder REJECTED. \
             This widens the accepted language — a node running the machine would \
             admit state its peers refuse.\n  value: {b:?}"
        ),
    }
}

/// Every corpus encoding, as `(label, bytes)`, for all four root types plus a
/// deep case per root.
fn encodings() -> Vec<(String, Kind, Vec<u8>)> {
    let mut out: Vec<(String, Kind, Vec<u8>)> = Vec::new();
    for (label, par) in corpus::par_corpus() {
        out.push((
            format!("Par::{label}"),
            Kind::Par,
            bincode::serialize(&par).expect("serialize"),
        ));
    }
    for (label, v) in corpus::list_par_with_random_corpus() {
        out.push((
            format!("ListParWithRandom::{label}"),
            Kind::ListParWithRandom,
            bincode::serialize(&v).expect("serialize"),
        ));
    }
    for (label, v) in corpus::bind_pattern_corpus() {
        out.push((
            format!("BindPattern::{label}"),
            Kind::BindPattern,
            bincode::serialize(&v).expect("serialize"),
        ));
    }
    for (label, v) in corpus::tagged_continuation_corpus() {
        out.push((
            format!("TaggedContinuation::{label}"),
            Kind::TaggedContinuation,
            bincode::serialize(&v).expect("serialize"),
        ));
    }
    out
}

#[derive(Clone, Copy)]
enum Kind {
    Par,
    ListParWithRandom,
    BindPattern,
    TaggedContinuation,
}

fn check(label: &str, kind: Kind, bytes: &[u8], rejections: &mut usize) {
    match kind {
        Kind::Par => assert_agree::<Par>(label, bytes, rejections),
        Kind::ListParWithRandom => assert_agree::<ListParWithRandom>(label, bytes, rejections),
        Kind::BindPattern => assert_agree::<BindPattern>(label, bytes, rejections),
        Kind::TaggedContinuation => assert_agree::<TaggedContinuation>(label, bytes, rejections),
    }
}

// ===========================================================================
// Family 1 — truncation at EVERY byte offset
// ===========================================================================

/// The exhaustive family. Every prefix of every corpus encoding is fed to both
/// decoders. This is the only mutation that provably visits every resume point
/// in the machine: a `*Build` op that read its scalars in the wrong order, or
/// a resume point that read one byte too many, changes *where* the stream runs
/// out and therefore changes the disposition of some prefix.
///
/// ⚠ Cost is Θ(Σ len²) over the corpus, which is why the deep cases are capped
/// at depth 48 and the corpus is not the place to add megabyte fixtures.
#[test]
fn truncation_at_every_offset_agrees() {
    let mut rejections = 0usize;
    let mut prefixes = 0usize;
    for (label, kind, bytes) in encodings() {
        for cut in 0..bytes.len() {
            prefixes += 1;
            check(
                &format!("{label} truncated at {cut}"),
                kind,
                &bytes[..cut],
                &mut rejections,
            );
        }
    }
    assert!(
        prefixes > 50_000,
        "ANTI-VACUITY: only {prefixes} truncations were tried; the corpus has shrunk"
    );
    assert!(
        rejections > prefixes / 2,
        "ANTI-VACUITY: only {rejections} of {prefixes} truncations were REJECTED by both \
         decoders. A truncated encoding should almost always be rejected; if most are \
         accepted, the corpus is producing near-empty encodings and this test is not \
         probing anything."
    );
    println!("  truncation: {prefixes} prefixes, {rejections} joint rejections");
}

// ===========================================================================
// Family 2 — byte substitutions at EVERY offset
// ===========================================================================

/// `bool` bytes flipped to 2, `Option` tags pushed to 2, and a general sweep.
///
/// The decoder cannot be asked "where are the bools" without re-implementing
/// itself, so the mutation is applied at every offset. `2` is the smallest
/// value that is invalid for both a `bool` and an `Option` tag, so a single
/// sweep covers `InvalidBoolEncoding` and `InvalidTagEncoding` at once — and
/// the two must still be *distinguished* correctly, because a byte that is a
/// valid `Option` tag position and an invalid `bool` position (or vice versa)
/// changes which error fires and, more importantly, whether one fires at all.
#[test]
fn byte_substitution_at_every_offset_agrees() {
    let mut rejections = 0usize;
    let mut mutations = 0usize;
    for (label, kind, bytes) in encodings() {
        for offset in 0..bytes.len() {
            for replacement in [0u8, 1, 2, 0x7F, 0xFF] {
                if bytes[offset] == replacement {
                    continue;
                }
                let mut mutant = bytes.clone();
                mutant[offset] = replacement;
                mutations += 1;
                check(
                    &format!("{label} byte[{offset}] := {replacement:#04x}"),
                    kind,
                    &mutant,
                    &mut rejections,
                );
            }
        }
    }
    assert!(
        mutations > 100_000,
        "ANTI-VACUITY: only {mutations} substitutions were tried"
    );
    assert!(
        rejections > 0,
        "ANTI-VACUITY: no substitution was rejected by either decoder"
    );
    println!("  substitution: {mutations} mutants, {rejections} joint rejections");
}

// ===========================================================================
// Family 3 — variant indices pushed out of range
// ===========================================================================

/// Every 4-byte window is overwritten with a value just past each oneof's
/// variant count, and with `u32::MAX`.
///
/// `36`, `9` and `2` are the first INVALID index for `ExprInstance`,
/// `ConnectiveInstance` and `TaggedCont` respectively — the off-by-one that a
/// range check written with `>` instead of `>=` would let through, which would
/// then be a decoder that accepts a term the derived decoder refuses.
#[test]
fn out_of_range_variant_indices_agree() {
    let mut rejections = 0usize;
    let mut mutations = 0usize;
    for (label, kind, bytes) in encodings() {
        if bytes.len() < 4 {
            continue;
        }
        for offset in 0..bytes.len() - 4 {
            for index in [2u32, 9, 36, 37, u32::MAX] {
                let mut mutant = bytes.clone();
                mutant[offset..offset + 4].copy_from_slice(&index.to_le_bytes());
                mutations += 1;
                check(
                    &format!("{label} variant@{offset} := {index}"),
                    kind,
                    &mutant,
                    &mut rejections,
                );
            }
        }
    }
    assert!(
        mutations > 100_000,
        "ANTI-VACUITY: only {mutations} variant-index mutations were tried"
    );
    assert!(rejections > 0, "ANTI-VACUITY: no variant mutation was rejected");
    println!("  variant index: {mutations} mutants, {rejections} joint rejections");
}

// ===========================================================================
// Family 4 — hostile lengths
// ===========================================================================

/// Every 8-byte window is overwritten with `usize::MAX`, `u64::MAX` and other
/// enormous counts.
///
/// ⚠ **This is the family that catches an out-of-memory abort.** Without
/// serde's `size_hint::cautious` cap (reproduced by `bincode_decoder`'s
/// `cautious_capacity`, and made unnecessary for `Par`-bearing sequences by the
/// counted-repeat design), a length near `usize::MAX` would make one side try
/// to pre-allocate and die while the other returns `Err` — a divergence that no
/// amount of valid-input testing can reach. A regression here does not fail
/// politely: the test process is killed. That is the intended signal.
#[test]
fn hostile_lengths_agree() {
    let mut rejections = 0usize;
    let mut mutations = 0usize;
    let hostile: [u64; 6] = [
        u64::MAX,
        usize::MAX as u64,
        u64::MAX / 2,
        1 << 62,
        1 << 40,
        4097, // just past the historical serde 4,096-element prealloc cap
    ];
    for (label, kind, bytes) in encodings() {
        if bytes.len() < 8 {
            continue;
        }
        for offset in 0..bytes.len() - 8 {
            for len in hostile {
                let mut mutant = bytes.clone();
                mutant[offset..offset + 8].copy_from_slice(&len.to_le_bytes());
                mutations += 1;
                check(
                    &format!("{label} len@{offset} := {len}"),
                    kind,
                    &mutant,
                    &mut rejections,
                );
            }
        }
    }
    assert!(
        mutations > 100_000,
        "ANTI-VACUITY: only {mutations} length mutations were tried"
    );
    assert!(rejections > 0, "ANTI-VACUITY: no length mutation was rejected");
    println!("  hostile lengths: {mutations} mutants, {rejections} joint rejections");
}

// ===========================================================================
// Family 5 — trailing bytes must be ACCEPTED
// ===========================================================================

/// `allow_trailing_bytes()` is part of `bincode::deserialize`'s configuration
/// (`bincode/src/lib.rs:177-185`). Rejecting trailing bytes would NARROW the
/// accepted language — a fork in the other direction, and an easy mistake for a
/// hand-written parser that "helpfully" checks it consumed everything.
#[test]
fn trailing_bytes_are_accepted_by_both() {
    let mut rejections = 0usize;
    let mut cases = 0usize;
    for (label, kind, bytes) in encodings() {
        for suffix in [
            vec![0u8],
            vec![0xFF; 16],
            vec![0x01, 0x02, 0x03, 0x04, 0x05],
        ] {
            let mut mutant = bytes.clone();
            mutant.extend_from_slice(&suffix);
            cases += 1;
            check(
                &format!("{label} + {} trailing bytes", suffix.len()),
                kind,
                &mutant,
                &mut rejections,
            );
        }
    }
    assert!(cases > 100, "ANTI-VACUITY: only {cases} trailing-byte cases");
    assert_eq!(
        rejections, 0,
        "trailing bytes must be ACCEPTED by both decoders; {rejections} case(s) were rejected"
    );
}

// ===========================================================================
// Family 6 — arbitrary short byte strings
// ===========================================================================

/// Byte strings that were never an encoding of anything. Most are rejected;
/// a few short ones legitimately decode to an empty value, and both decoders
/// must make the same call.
#[test]
fn arbitrary_byte_strings_agree() {
    let mut rejections = 0usize;
    let mut cases = 0usize;
    let mut seed: u64 = 0x2026_07_27;
    for len in 0..80usize {
        for _ in 0..40 {
            // xorshift64* — deterministic, so a failure is reproducible.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let bytes: Vec<u8> = (0..len)
                .map(|i| ((seed >> ((i % 8) * 8)) as u8) ^ (i as u8))
                .collect();
            cases += 1;
            check(
                &format!("arbitrary[{len}] seed {seed:#x}"),
                Kind::Par,
                &bytes,
                &mut rejections,
            );
            check(
                &format!("arbitrary[{len}] seed {seed:#x} (TaggedContinuation)"),
                Kind::TaggedContinuation,
                &bytes,
                &mut rejections,
            );
        }
    }
    assert!(cases > 1_000, "ANTI-VACUITY: only {cases} arbitrary strings");
    assert!(
        rejections > cases,
        "ANTI-VACUITY: {rejections} joint rejections over {cases} strings x 2 types — \
         random byte strings should almost all be rejected"
    );
    println!("  arbitrary: {cases} strings, {rejections} joint rejections");
}

// ===========================================================================
// Family 7 — the machine must never report a MachineInvariant
// ===========================================================================

/// `ColdStoreDecodeError::MachineInvariant` is documented as unreachable by
/// construction: it means a `*Build` op found its own `*Start`'s value missing.
/// It is an error rather than a panic so a hypothetical defect degrades one
/// read instead of aborting the node — but it must never actually fire, and
/// "never" is only a claim until something checks it against hostile input.
#[test]
fn no_input_reaches_a_machine_invariant() {
    use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecodeError;

    let mut checked = 0usize;
    for (label, _kind, bytes) in encodings() {
        for cut in 0..bytes.len() {
            checked += 1;
            if let Err(ColdStoreDecodeError::MachineInvariant(what)) = Par::cold_decode(&bytes[..cut])
            {
                panic!(
                    "{label} truncated at {cut} reached MachineInvariant({what}) — the \
                     opcode program's push/take pairing is wrong, which is a DECODER \
                     defect, not a bad input"
                );
            }
        }
    }
    assert!(checked > 50_000, "ANTI-VACUITY: only {checked} inputs checked");
}
