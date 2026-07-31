//! # The bincode surface is TRIE-NATIVE — `U(m)`, verbatim and contiguous
//!
//! The owner's standing ruling is that *"serialization of pathmap should use its
//! byte array serialization directly, regardless the format"*. `1b576c90` did
//! that for prost (field 8; **CBR-041**) and left bincode emitting `u64 n ‖
//! n × Par`. This file pins the bincode half — **FORM ②**:
//!
//! ```text
//!   EPathMap serde/bincode ::= u64-LE |U(m)| ‖ U(m)      the trie's byte array
//!                              u64-LE n     ‖ n × Par    the values
//!                              locally_free ‖ connective_used ‖ remainder
//! ```
//!
//! ## ★ Why the values are carried, and why that is not a hedge
//!
//! `U(m)` **alone** would be smaller and would also make `decode_trie_path` the
//! reader. Its escape arm re-decodes a ¬`eval_stable` entry through prost's
//! recursion-limited decoder, and
//! `rholang/tests/pathmap_escape_depth_reachability.rs` measures that ceiling at
//! depth **32** — while ordinary Rholang compiles such an entry at depth **40**.
//! A reader built on it would refuse terms a deploy can write, which is a
//! *regression*, not a narrowing.
//!
//! FORM ② avoids it completely: the reader
//! (`EntryTrie::from_path_stream_and_values`) splits `U(m)` by **pure byte
//! slicing** and takes the entries from the value sequence, through the existing
//! iterative, depth-**unlimited** machinery. `decode_trie_path` is never called,
//! so this surface acquires **no depth ceiling** — which is what
//! [`the_cold_store_has_no_depth_ceiling`] measures at depths **34** and **64**,
//! both past the escape arm's 32.
//!
//! ## ★★ WHICH key stream — the CBR-043 correction
//!
//! There is one function `U` (`path_stream_of`, a read-zipper walk over a trie),
//! and this surface applies it to **the value this surface writes**. FORM ②
//! applied it to the value the map *stores*, and the two differ exactly when an
//! entry carries `locally_free`: the serde surface has always written lf-BLANKED
//! entries (`serialize_as_empty_bytes`), while `encode_trie_path`'s `0x0F`
//! escape arm keys a ¬`eval_stable` entry by its canonical **prost** bytes,
//! which retain the bitset.
//!
//! So the key stream FORM ② emitted was derived from entries that are not the
//! ones beside it — and that carried `locally_free` onto the event hash, in
//! breach of `models/src/rust/rholang/wire.rs`'s standing rule that it *"must
//! not reach an RSpace channel hash"*. §4 is the repair, measured.
//!
//! ⚠ Prost is unaffected and must stay so: it writes the entries as stored, so
//! it reads `EPathMap::path_stream`. Both streams exist, each on the surface
//! whose argument it is.
//!
//! ## The claims
//!
//! | § | claim | why a weaker test would miss it |
//! |---|---|---|
//! | 1 | `bincode(EPathMap)` contains the key stream as a **contiguous substring** | an interleaved key/value form round-trips perfectly and is not the trie's byte array |
//! | 2 | `cold_decode ∘ cold_encode` is a byte-level fixed point past depth 32 | a ceiling introduced here is invisible on the shallow fixtures every other suite uses |
//! | 3 | a peer stream whose key disagrees with its value **RE-FILES** | rejecting narrows the accepted language relative to peers, which is a fork |
//! | 4 | `locally_free` reaches neither these bytes nor the **event hash**, and both survive a cold-store round trip | the VALUE half was already blind to it — only the KEY half moved, and only on maps nobody fixtures |
//! | 5 | the size delta is exactly the framed key stream | a form that dropped or duplicated a key would still round-trip |

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EList, EPathMap, ESet, ETuple, Expr, Par, Var};
use models::rust::rhoapi_ext::{EntryTrie, PathStreamDisagreement, PathStreamVerdict};
use models::rust::rholang::bincode_encoder::ColdStoreEncode;
use models::rust::rholang::par_children::dismantle;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;

// ---------------------------------------------------------------------------
// fixtures, built ITERATIVELY so construction is never the constraint
// ---------------------------------------------------------------------------

fn gint(v: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }],
        ..Default::default()
    }
}

/// A `depth`-deep left spine of 1-tuples around `GInt(1)` — GROUND at every
/// depth, so it is keyed by the structural (`0x0C`) arm.
fn deep_tuple(depth: usize) -> Par {
    let mut par = gint(1);
    for _ in 0..depth {
        par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                    ps: vec![par],
                    locally_free: Vec::new(),
                    connective_used: false,
                })),
            }],
            ..Default::default()
        };
    }
    par
}

/// ★ A ¬`eval_stable` entry nested `depth` levels deep — **the shape the ceiling
/// is measured on**, rebuilt from `rholang/tests/pathmap_escape_depth_reachability.rs`
/// so the two files are talking about the same term family.
///
/// `ESet` is outside the stable alphabet, so the whole term takes the `0x0F`
/// **escape** arm — whose payload is the entry's canonical prost bytes, read
/// back through prost's 100-level recursion cap. The `EList` spine supplies the
/// depth. A GROUND fixture would take the structural arm, which has no such cap,
/// and §2 would then be a ladder over a cliff that is not there.
fn deep_escaped(depth: usize) -> Par {
    let mut inner = gint(1);
    for _ in 0..depth {
        inner = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![inner],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![inner],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

/// One `Par` holding one `EPathMap`.
fn pathmap_par(map: EPathMap) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }],
        ..Default::default()
    }
}

/// A map whose single entry is a `depth`-deep ground tuple.
fn map_at_depth(depth: usize) -> EPathMap {
    EPathMap::new(vec![deep_tuple(depth)], Vec::new(), false, None)
}

/// A map whose single entry is a `depth`-deep **escape-arm** entry.
fn escaped_map_at_depth(depth: usize) -> EPathMap {
    EPathMap::new(vec![deep_escaped(depth)], Vec::new(), false, None)
}

// ===========================================================================
// §1  ★★ `U(m)` appears VERBATIM and CONTIGUOUSLY
// ===========================================================================

/// The `u64`-LE length prefix and the key stream itself, in that order, as one
/// unbroken run of bytes.
///
/// ⚠ **Contiguity is the claim, not mere presence.** A form that emitted the
/// keys interleaved with their values would still "contain every key byte" and
/// would still round-trip — and it would not be the trie's byte array. Only a
/// substring search can tell the two apart.
#[test]
fn the_encoding_contains_the_path_stream_verbatim() {
    for (label, map) in [
        ("two ground entries", EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None)),
        ("single deep entry", map_at_depth(34)),
        (
            "metadata-bearing",
            EPathMap::new(vec![gint(7)], vec![0xAA], true, None),
        ),
    ] {
        let path_stream = map.path_stream().to_vec();
        assert!(
            !path_stream.is_empty(),
            "`{label}`: the fixture must have a non-empty U(m), or the search below \
             proves nothing — the empty string is a substring of everything"
        );

        let mut framed = Vec::with_capacity(8 + path_stream.len());
        framed.extend_from_slice(&(path_stream.len() as u64).to_le_bytes());
        framed.extend_from_slice(&path_stream);

        let encoded = bincode::serialize(&map).expect("serialize");
        assert!(
            contains(&encoded, &framed),
            "`{label}`: the bincode encoding must carry `u64-LE |U(m)| ‖ U(m)` as ONE \
             unbroken run. |U(m)| = {}, encoding = {} B. A form that scattered the keys \
             among their values would pass a weaker containment test and would not be \
             the trie serialized as a trie.",
            path_stream.len(),
            encoded.len()
        );
        // …and it is the PREFIX, which is what makes the reader's first two
        // primitives `bytes()` then `u64()`.
        assert_eq!(
            &encoded[..framed.len()],
            &framed[..],
            "`{label}`: U(m) is the FIRST thing an EPathMap writes"
        );

        // The single-walk machine and the derived `Serialize` must agree, or the
        // claim is about one emitter rather than about the format.
        assert_eq!(
            pathmap_par(map.clone()).cold_encode(),
            bincode::serialize(&pathmap_par(map)).expect("oracle"),
            "`{label}`: the machine and the derived oracle must write identical bytes"
        );
    }
}

/// Naive substring search — no dependency, and the corpus is tiny.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// ★★ ANTI-VACUITY for §1: the search must REJECT a stream that does not carry
/// `U(m)` contiguously. Without this, a `contains` that answered `true` for
/// everything would be indistinguishable from a pass.
#[test]
fn the_containment_check_can_go_red() {
    let map = EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None);
    let path_stream = map.path_stream().to_vec();
    let mut framed = Vec::with_capacity(8 + path_stream.len());
    framed.extend_from_slice(&(path_stream.len() as u64).to_le_bytes());
    framed.extend_from_slice(&path_stream);

    // A scattered form: every byte of U(m) is present, none of it contiguous.
    let mut scattered = Vec::with_capacity(framed.len() * 2);
    for byte in &framed {
        scattered.push(*byte);
        scattered.push(0xFF);
    }
    assert!(
        !contains(&scattered, &framed),
        "the containment check must REJECT a scattered encoding, or §1 is vacuous"
    );
    assert!(
        contains(&framed, &framed),
        "…and must ACCEPT the contiguous one"
    );
}

// ===========================================================================
// §2  ★★ NO DEPTH CEILING — 34 and 64 are past the escape arm's measured 32
// ===========================================================================

/// `cold_decode ∘ cold_encode` is a **byte-level fixed point**, at depths on
/// both sides of the escape arm's measured read ceiling of 32.
///
/// ⚠ 34 and 64 are the whole point, and they are carried on the **escape-arm**
/// fixture ([`deep_escaped`]) rather than a ground one — that distinction was a
/// measured correction, not a stylistic one. A ground entry is keyed by the
/// structural `0x0C` arm, which is iterative and has no cap at all, so a ground
/// ladder would pass even against a key-reading decoder and would prove nothing.
/// `rholang/tests/pathmap_escape_depth_reachability.rs` measures the escape arm
/// refusing at 32; [`the_ceiling_this_avoids_is_real`] re-measures the refusal
/// here so the ladder is known to span a real cliff.
///
/// This passes because `EntryTrie::from_path_stream_and_values` never calls
/// `decode_trie_path`.
#[test]
fn the_cold_store_has_no_depth_ceiling() {
    for depth in [4usize, 34, 64] {
        let par = pathmap_par(escaped_map_at_depth(depth));

        let once = par.cold_encode();
        let decoded = Par::cold_decode(&once)
            .unwrap_or_else(|e| panic!("depth {depth}: cold_decode refused its own bytes: {e:?}"));
        let twice = decoded.cold_encode();
        assert_eq!(
            once, twice,
            "depth {depth}: `cold_encode` must be a byte-level fixed point through \
             `cold_decode`. Depths past 32 are the measured escape-arm ceiling — a \
             failure HERE and not at depth 4 means the reader started decoding trie \
             keys instead of reading the value sequence."
        );

        // …and the derived oracle agrees at the same depth, so the property is
        // about the FORMAT and not about one emitter.
        assert_eq!(
            once,
            bincode::serialize(&par).expect("oracle"),
            "depth {depth}: the machine and the derived `Serialize` must agree"
        );
        let oracle_round: Par =
            bincode::deserialize(&once).expect("oracle: deserialize its own bytes");
        assert_eq!(
            oracle_round.cold_encode(),
            once,
            "depth {depth}: the derived `Deserialize` must reach the same fixed point"
        );
        dismantle(decoded);
        dismantle(oracle_round);
        dismantle(par);
    }
}

/// Depth **4,096** — so the claim is *unbounded reader*, not *a higher bound*.
///
/// ⚠ Run on its own 64 MiB stack and torn down through
/// [`models::rust::rholang::par_children::dismantle`], because the things this
/// test is NOT about are still Θ(depth) on the native stack: `<Par as Drop>`,
/// `<Par as PartialEq>`, and the derived `Serialize`/`Deserialize`. Only
/// `cold_encode` / `cold_decode` — the pair under test — are iterative, and
/// exercising them is the whole point.
#[test]
fn the_cold_store_reader_is_unbounded_not_merely_deeper() {
    let bytes = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let par = pathmap_par(escaped_map_at_depth(4_096));
            let once = par.cold_encode();
            let decoded = Par::cold_decode(&once).expect("cold_decode at depth 4,096");
            let twice = decoded.cold_encode();
            assert_eq!(
                once, twice,
                "depth 4,096: the cold store must be a byte-level fixed point — the reader \
                 is UNBOUNDED, not merely deeper than 32"
            );
            dismantle(decoded);
            dismantle(par);
            once.len()
        })
        .expect("spawn")
        .join()
        .expect("a depth-4,096 round trip must survive");
    println!("  depth 4,096 cold-store encoding: {bytes} B");
    assert!(bytes > 0);
}

/// The RED that gives §2 its meaning: `decode_trie_path` — the reader FORM ②
/// declines to use — really does refuse at these depths.
///
/// ★ Without this the depth ladder above would be a ladder over a cliff that
/// might not be there, and "no ceiling" would be unfalsifiable.
#[test]
fn the_ceiling_this_avoids_is_real() {
    use models::rust::canonical_path::{decode_trie_path, encode_trie_path};

    let shallow = encode_trie_path(&deep_escaped(4));
    assert!(
        decode_trie_path(&shallow).is_ok(),
        "the key codec must READ a shallow escape-arm key, or the ladder is measuring a \
         broken codec rather than a ceiling"
    );

    for depth in [34usize, 64] {
        let deep = encode_trie_path(&deep_escaped(depth));
        assert!(
            !deep.is_empty(),
            "encode_trie_path is TOTAL — it must produce a key at depth {depth}"
        );
        assert!(
            decode_trie_path(&deep).is_err(),
            "★ THE RED IS INERT. `decode_trie_path` must REFUSE a depth-{depth} escape-arm \
             key — that refusal is the entire reason FORM ② carries the values beside \
             U(m). If it succeeds, the ceiling has moved and \
             `the_cold_store_has_no_depth_ceiling` is no longer distinguishing a \
             key-reading decoder from a value-reading one."
        );
    }
}

// ===========================================================================
// §3  ⚠ A disagreeing peer stream is RE-FILED, never rejected
// ===========================================================================

/// A node that refuses a byte string its peers accept has **forked**. So every
/// way a peer's `U(m)` can fail to be this node's own key stream yields the trie
/// built from the VALUES — and yields the *same* trie a canonical stream would.
///
/// ★ Both halves are asserted. That the disagreement is **detected** (the
/// verdict) is what stops this from being a test that a `_ => {}` arm would
/// pass; that the value is **unchanged** is the property `axis_acceptance = NO`
/// rests on.
#[test]
fn a_disagreeing_peer_stream_refiles_rather_than_being_rejected() {
    let values = vec![gint(1), gint(2), gint(3)];
    let canonical = EntryTrie::from(values.clone());
    let honest = canonical.path_stream().to_vec();

    // The control: the honest stream AGREES, and produces the canonical trie.
    let (from_honest, verdict) = EntryTrie::from_path_stream_and_values(&honest, values.clone());
    assert_eq!(
        verdict,
        PathStreamVerdict::Agrees,
        "the encoder's own key stream must agree with itself, or every case below is \
         measuring a broken control"
    );
    assert_eq!(from_honest, canonical);

    // Each hostile stream, named by the disagreement it must produce.
    let mut a_key_bit_flipped = honest.clone();
    let last = a_key_bit_flipped.len() - 1;
    a_key_bit_flipped[last] ^= 0xFF;

    let truncated_key = honest[..honest.len() - 1].to_vec();
    let orphaned_header = {
        let mut s = honest.clone();
        s.extend_from_slice(&[0u8, 0, 0]); // 3 bytes: not a u32 header
        s
    };
    let excess_frame = {
        let mut s = honest.clone();
        s.extend_from_slice(&2u32.to_le_bytes());
        s.extend_from_slice(&[0xAB, 0xCD]);
        s
    };
    let empty = Vec::new();

    // ⚠ Each expectation is a PATTERN over the disagreement, not a bare
    // "something went wrong": a test that accepted any fault would pass against
    // a classifier that answered one constant.
    type Expectation = (&'static str, fn(&PathStreamDisagreement) -> bool);
    let key_disagrees_at_2: Expectation = (
        "KeyDisagrees { index: 2 }",
        |d| matches!(d, PathStreamDisagreement::KeyDisagrees { index: 2 }),
    );
    let truncated_key_fault: Expectation = (
        "MalformedFraming(TruncatedKey { .. })",
        |d| {
            matches!(
                d,
                PathStreamDisagreement::MalformedFraming(
                    models::rust::pathmap_crate_type_mapper::PathFrameError::TruncatedKey { .. }
                )
            )
        },
    );
    let excess_frames: Expectation = (
        "ExcessFrames { values: 3 }",
        |d| matches!(d, PathStreamDisagreement::ExcessFrames { values: 3 }),
    );
    let orphaned_trailing: Expectation = (
        "MalformedFraming(TruncatedLength { .. })",
        |d| {
            matches!(
                d,
                PathStreamDisagreement::MalformedFraming(
                    models::rust::pathmap_crate_type_mapper::PathFrameError::TruncatedLength { .. }
                )
            )
        },
    );
    let missing_frames: Expectation = (
        "MissingFrames { at: 0, values: 3 }",
        |d| matches!(d, PathStreamDisagreement::MissingFrames { at: 0, values: 3 }),
    );

    for (label, hostile, expected) in [
        ("a flipped key byte", a_key_bit_flipped, key_disagrees_at_2),
        ("a truncated final key", truncated_key, truncated_key_fault),
        (
            "three orphaned trailing bytes",
            orphaned_header,
            orphaned_trailing,
        ),
        ("one frame too many", excess_frame, excess_frames),
        ("no key stream at all", empty, missing_frames),
    ] {
        let (refiled, verdict) =
            EntryTrie::from_path_stream_and_values(&hostile, values.clone());
        // ⚠ NOT REJECTED — there is no error channel here at all, by construction.
        assert_eq!(
            refiled, canonical,
            "`{label}`: a disagreeing stream must RE-FILE to the canonical trie, never \
             narrow what this node accepts"
        );
        assert_eq!(
            refiled.path_stream(),
            canonical.path_stream(),
            "`{label}`: …and re-emit the canonical key stream, so the disagreement \
             cannot propagate"
        );
        match verdict {
            PathStreamVerdict::Agrees => panic!(
                "`{label}`: the disagreement was NOT DETECTED. The re-file assertion \
                 above would pass even if the key stream were ignored entirely, so \
                 without this the check is dead code."
            ),
            PathStreamVerdict::Refiled(found) => {
                let (want, matches) = expected;
                assert!(
                    matches(&found),
                    "`{label}`: the disagreement must be classified as the fault it is — \
                     wanted {want}, got {found:?}"
                );
            }
        }
    }
}

/// The same policy reached through the two SURFACES rather than the shared
/// function — a hand-spliced bincode stream carrying a wrong key stream must
/// decode, on both the derived `Deserialize` and the trampolined machine, to the
/// map its values denote.
#[test]
fn both_surfaces_refile_a_hostile_stream_to_the_same_map() {
    let values = vec![gint(4), gint(5)];
    let honest = EPathMap::new(values.clone(), Vec::new(), false, None);
    let canonical_bytes = bincode::serialize(&honest).expect("serialize");

    // Splice a WRONG key stream of the same length in front of the values, so
    // every offset after it is unchanged.
    let path_stream_len = honest.path_stream().len();
    let mut hostile = canonical_bytes.clone();
    for byte in hostile[8..8 + path_stream_len].iter_mut() {
        *byte ^= 0xFF;
    }
    assert_ne!(
        hostile, canonical_bytes,
        "the splice must actually change the stream"
    );

    let derived: EPathMap = bincode::deserialize(&hostile).expect("the derived reader must ACCEPT");
    assert_eq!(
        derived, honest,
        "the derived `Deserialize` must re-file a hostile key stream to the map its \
         values denote"
    );

    // …and through the trampolined machine, reached as a `Par`.
    let par_bytes = {
        let mut framed = bincode::serialize(&pathmap_par(honest.clone())).expect("par");
        let at = find(&framed, &canonical_bytes).expect("the map's bytes sit inside the Par's");
        for byte in framed[at + 8..at + 8 + path_stream_len].iter_mut() {
            *byte ^= 0xFF;
        }
        framed
    };
    let machine = Par::cold_decode(&par_bytes).expect("the machine must ACCEPT");
    let oracle: Par = bincode::deserialize(&par_bytes).expect("the oracle must ACCEPT");
    assert_eq!(
        machine, oracle,
        "the machine and the derived decoder must dispose of a hostile key stream \
         IDENTICALLY — a divergence here is a fork, not a robustness difference"
    );
    assert_eq!(
        machine,
        pathmap_par(honest),
        "…and both must land on the map the values denote"
    );
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ===========================================================================
// §4  ★★ THE ACCEPTANCE PROPERTIES — `locally_free` does not reach this wire
// ===========================================================================
//
// FORM ② (`3a32cf07`, CBR-042) put `U(m)` — the key stream of the entries as
// STORED — on the bincode wire, beside values this surface has always written
// lf-BLANKED. A key derived from the unblanked entries, emitted next to the
// blanked ones, carries the bitset: `encode_trie_path`'s `0x0F` escape arm files
// a ¬`eval_stable` entry as its canonical prost bytes, and prost retains
// `locally_free`.
//
// That breached a standing rule stated in `models/src/rust/rholang/wire.rs`:
//
//   > `locally_free` is transient analysis data that must not reach an RSpace
//   > channel hash.
//
// **CBR-043** applies one function `U` to the value this surface writes
// (`EntryTrie::wire_path_stream`). The five properties below are what that buys,
// each with a control so none of them can pass vacuously.

/// A `Par` whose `locally_free` is set, and its cleared twin — the minimal pair
/// that differs in nothing else.
///
/// A bound-variable `EVar` is non-ground BY CONTENT, so clearing the bitset does
/// not flip the entry to `eval_stable`: both twins take the same `0x0F` escape
/// arm, and the ONLY difference between their keys is the bitset itself.
fn lf_pair() -> (Par, Par) {
    let free = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(models::rhoapi::EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            })),
        }],
        locally_free: models::create_bit_vector(&[0]),
        ..Default::default()
    };
    let cleared = Par {
        locally_free: Vec::new(),
        ..free.clone()
    };
    (free, cleared)
}

/// The map the acceptance properties are carried on: one lf-bearing,
/// ¬`eval_stable` entry.
fn lf_bearing_map() -> EPathMap {
    EPathMap::new(vec![lf_pair().0], Vec::new(), false, None)
}

/// A produce hash over a datum holding one `Par`. The event hash is blake2b over
/// the bincode preimage, so this is the *consensus-visible* reading of "what did
/// this surface write".
fn produce_hash(par: Par) -> Vec<u8> {
    use models::rhoapi::ListParWithRandom;
    use rspace_plus_plus::rspace::trace::event::Produce;

    let channel = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString("cbr-043".into())),
        }],
        ..Default::default()
    };
    let datum = ListParWithRandom {
        pars: vec![par],
        random_state: vec![7u8; 32],
    };
    Produce::create(&channel, &datum, true)
        .hash
        .bytes()
        .to_vec()
}

// ---------------------------------------------------------------------------
// §4.1  Property 1 — the event hash does not see entry-level `locally_free`
// ---------------------------------------------------------------------------

/// Two maps differing ONLY in an entry's `locally_free` must produce the SAME
/// produce hash.
///
/// ⚠ Read through `Produce::create`, not through `bincode::serialize`, on
/// purpose: the claim that matters is about the consensus artifact, and a test
/// that stopped at the preimage would still pass if the hashing leg started
/// reading something else.
///
/// ★ The control is `path_stream()`: the two maps' STORED key streams must still
/// DIFFER. They are what prost writes, and prost retains the bitset — so if this
/// control ever goes equal, the fixture has stopped exercising the mechanism and
/// the assertion above is comparing a map with itself.
#[test]
fn the_event_hash_does_not_see_an_entrys_locally_free() {
    let (free, cleared) = lf_pair();

    // ⚠ NOT `assert_ne!(free, cleared)`. `Par`'s `PartialEq` is AlwaysEqual and
    // ignores `locally_free` outright — that is the very asymmetry this file is
    // about, and using `==` here would compare the two entries by the one
    // relation that cannot see the difference. The honest control is the
    // relation the KEY uses: prost bytes.
    assert_ne!(
        prost::Message::encode_to_vec(&free),
        prost::Message::encode_to_vec(&cleared),
        "the two entries must differ ON THE PROST WIRE, or everything below is a tautology"
    );

    let with_lf = EPathMap::new(vec![free], Vec::new(), false, None);
    let without_lf = EPathMap::new(vec![cleared], Vec::new(), false, None);

    assert_ne!(
        with_lf.path_stream(),
        without_lf.path_stream(),
        "★ THE CONTROL IS INERT. The two maps must hold different STORED keys — the \
         escape arm files a ¬eval_stable entry as its canonical prost bytes, which \
         retain locally_free. That difference is what the wire must NOT propagate; if \
         it is absent, there is nothing to propagate and this test proves nothing."
    );
    assert_eq!(
        with_lf.wire_path_stream(),
        without_lf.wire_path_stream(),
        "★ THE REPAIR, at the seam: the surface writes lf-blanked entries, so the key \
         stream it writes must be the keys of those entries — and blanking maps both \
         fixtures onto one entry set."
    );
    assert_eq!(
        bincode::serialize(&with_lf).expect("with"),
        bincode::serialize(&without_lf).expect("without"),
        "…hence identical bincode preimages"
    );
    assert_eq!(
        produce_hash(pathmap_par(with_lf)),
        produce_hash(pathmap_par(without_lf)),
        "★★ PROPERTY 1. An entry's locally_free must NOT reach an RSpace channel hash \
         (`wire.rs`: transient analysis data). A difference here is two nodes disagreeing \
         about the identity of one event."
    );
}

/// ★★ ANTI-VACUITY for Property 1: `produce_hash` must be able to tell two
/// genuinely different maps apart.
///
/// Without this, a `produce_hash` that answered one constant — or a fixture pair
/// that had collapsed to one value — would be indistinguishable from a pass.
#[test]
fn the_produce_hash_can_still_tell_two_maps_apart() {
    let one = EPathMap::new(vec![gint(1)], Vec::new(), false, None);
    let two = EPathMap::new(vec![gint(2)], Vec::new(), false, None);
    assert_ne!(
        produce_hash(pathmap_par(one)),
        produce_hash(pathmap_par(two)),
        "★ THE RED IS INERT: two maps with different entry SETS must hash differently, \
         or Property 1 is measuring a constant function"
    );
}

// ---------------------------------------------------------------------------
// §4.2  Property 2 — the event hash survives a cold-store round trip
// ---------------------------------------------------------------------------

/// `hash(m) == hash(cold_decode(cold_encode(m)))` for the lf-bearing fixture.
///
/// ★★ This is the play/replay property. A map that hashed one way in play and
/// another way after coming back off the cold store would make replay disagree
/// with play about the same event — which is a consensus fault, not a
/// serialization curiosity. FORM ② had exactly that: `e48b249c…` before the trip
/// and `7259192343…` after it.
///
/// ★ The ground control below is what stops this passing for the wrong reason: a
/// round trip that had become the identity on EVERYTHING (say, by the reader
/// adopting the wire's keys) would pass this leg while destroying the property
/// the reader exists for.
#[test]
fn the_event_hash_is_invariant_under_a_cold_store_round_trip() {
    for (label, map) in [
        ("lf-bearing", lf_bearing_map()),
        ("ground (control)", EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None)),
    ] {
        let before = produce_hash(pathmap_par(map.clone()));
        let round: EPathMap =
            bincode::deserialize(&bincode::serialize(&map).expect("ser")).expect("de");
        let after = produce_hash(pathmap_par(round));
        assert_eq!(
            before, after,
            "★★ PROPERTY 2 (`{label}`): the produce hash must be INVARIANT under a \
             cold-store round trip. A difference is a play/replay divergence — the same \
             map hashing differently depending on whether it has been through the store."
        );
    }
}

// ---------------------------------------------------------------------------
// §4.3  Property 3 — `cold_encode` is a fixed point on the FIRST application
// ---------------------------------------------------------------------------

/// The lf-bearing map reaches its byte-level fixed point IMMEDIATELY, like every
/// other map.
///
/// ⚠ This test previously asserted the OPPOSITE — `assert_ne!(first, second)`,
/// "the lf-bearing map is the case that takes two rounds" — and filed the second
/// round as a stated cost. It was not a cost; it was the defect's signature. A
/// stream that moves on re-encoding means the writer wrote something that is not
/// a function of what it wrote, and here that something was the trie key.
///
/// ★ The ground control is retained unchanged: it settled on the first
/// application before and must still, so a change that made EVERY map settle
/// immediately for some unrelated reason (a reader that ignored keys entirely,
/// say) cannot be mistaken for this repair.
#[test]
fn the_encoding_is_a_fixed_point_on_the_first_application() {
    for (label, map) in [
        ("lf-bearing", lf_bearing_map()),
        ("ground (control)", map_at_depth(4)),
    ] {
        let par = pathmap_par(map);
        let once = par.cold_encode();
        let decoded = Par::cold_decode(&once)
            .unwrap_or_else(|e| panic!("`{label}`: cold_decode refused its own bytes: {e:?}"));
        let twice = decoded.cold_encode();
        assert_eq!(
            once, twice,
            "★★ PROPERTY 3 (`{label}`): `cold_encode` must be a byte-level fixed point on \
             the FIRST application. A stream that still moved on the second would mean the \
             writer emitted a quantity derived from something other than what it wrote."
        );

        // …and the derived oracle agrees, so the property is about the FORMAT
        // rather than about one emitter.
        let oracle = bincode::serialize(&par).expect("oracle");
        assert_eq!(once, oracle, "`{label}`: machine and derived `Serialize` must agree");
        dismantle(decoded);
        dismantle(par);
    }
}

// ---------------------------------------------------------------------------
// §4.4  ⚠ The NON-GOAL, measured — prost is NOT a round-trip fixed point
// ---------------------------------------------------------------------------

/// ⚠⚠ **`prost(cold_decode(cold_encode(m))) == prost(m)` is NOT a property of
/// this surface, and CBR-043 does not make it one.** Measured, so nobody
/// re-opens it as a bug.
///
/// The reason has nothing to do with keys. `EPathMap`'s **map-level**
/// `locally_free` (proto tag 3) is blanked by serde independently of anything
/// this file is about — the hand-written `Serialize` writes it as empty bytes,
/// the same normalization the twelve injected `.rhoapi` sites provide — while
/// prost RETAINS it. So a map that carries a map-level bitset comes back off the
/// cold store without one, and its prost encoding is shorter by exactly that
/// field. That is the serialize-only asymmetry working as designed, and it
/// predates FORM ② entirely.
///
/// ⇒ the two halves are separated here rather than argued about:
///
/// | quantity | round-trip stable? | why |
/// |---|---|---|
/// | the bincode encoding | **YES** (Property 3) | what CBR-043 repaired |
/// | the map's ENTRIES | **YES** | blanking is idempotent |
/// | `prost(m)` | **NO** | map-level `locally_free` is blanked by serde, retained by prost |
#[test]
fn prost_is_not_round_trip_stable_and_the_cause_is_key_independent() {
    use prost::Message as _;

    // The named fixture: a map-level bitset AND entry-level ones.
    let fixture = {
        use models::rust::utils::{new_boundvar_par, new_elist_par, new_gstring_par};
        let plain = new_elist_par(
            vec![new_gstring_par("plain".to_string(), Vec::new(), false)],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        );
        let var_entry = new_boundvar_par(1, models::create_bit_vector(&[1]), false);
        EPathMap::new(
            vec![plain, var_entry],
            models::create_bit_vector(&[0]),
            false,
            None,
        )
    };
    assert!(
        !fixture.locally_free.is_empty(),
        "the fixture must carry a MAP-level bitset, or the non-goal below is not exercised"
    );

    let round: EPathMap =
        bincode::deserialize(&bincode::serialize(&fixture).expect("ser")).expect("de");
    assert!(
        round.locally_free.is_empty(),
        "the map-level normalization is unchanged: serialize writes it EMPTY"
    );
    assert_ne!(
        fixture.encode_to_vec(),
        round.encode_to_vec(),
        "⚠ THE NON-GOAL, MEASURED. prost is not a round-trip fixed point here, and the \
         cause is the map-level locally_free normalization — NOT the trie key. If this \
         ever goes equal, the serialize-only asymmetry has been dropped, which is a \
         consensus-visible change that must be filed rather than discovered here."
    );

    // ★★ THE ISOLATION — the whole gap is the `locally_free` NORMALIZATION, and
    // nothing about keys, entries, or order.
    //
    // Build the fixture's fully lf-cleared twin by hand (map level and entry
    // level — the two places this fixture carries a bitset) and assert that the
    // ROUND TRIP's prost image is EXACTLY that twin's. If any part of the gap
    // came from the key stream or from a re-ordering, these would differ.
    let cleared_twin = EPathMap::new(
        fixture
            .ps()
            .iter()
            .map(|entry| Par {
                locally_free: Vec::new(),
                ..entry.clone()
            })
            .collect::<Vec<_>>(),
        Vec::new(),
        fixture.connective_used,
        fixture.remainder.clone(),
    );
    assert_eq!(
        round.encode_to_vec(),
        cleared_twin.encode_to_vec(),
        "★ THE ISOLATION. A round trip's prost image must be exactly the prost image of \
         the lf-CLEARED fixture — i.e. dropping `locally_free` is the ONLY thing the trip \
         does. That is what makes the inequality above a property of the serialize-only \
         normalization rather than a residue of the trie key, and it is why CBR-043 files \
         it as a NON-GOAL instead of a bug."
    );
    // …and the map-level field really is one of the two places, so the two legs
    // are not restating each other.
    assert_ne!(
        fixture.locally_free,
        cleared_twin.locally_free,
        "★ the twin must actually differ from the fixture at the MAP level (prost tag 3)"
    );
}

// ---------------------------------------------------------------------------
// §4.5  ★★ Why the O(1) discriminator is `entries_stable` — MEASURED
// ---------------------------------------------------------------------------

/// `EntryTrie::wire_path_stream` answers in O(1) when the trie is
/// `entries_stable`, and the obvious alternative — `union_locally_free.is_empty()`
/// — is **UNSOUND**. This is the measurement, not the argument.
///
/// # Why it matters
///
/// The O(1) arm is what keeps `bincode_encoder_space`'s zero-allocation gate at zero
/// for the shapes that dominate. An unsound discriminator would take that arm on
/// a map whose blanked key stream DIFFERS, and would therefore re-introduce the
/// exact defect CBR-043 repairs — silently, on precisely the maps nobody
/// fixtures.
///
/// # The two properties, and the difference between them
///
/// `union_locally_free` folds the entries' **top-level** `Par::locally_free`
/// only. It is not hereditary: a bitset one level down — inside a nested
/// `EPathMap`, or inside a plain `EList` — never reaches it.
///
/// `eval_stable_par` demands `locally_free.is_empty()` at EVERY level of the
/// stable alphabet: the `Par` itself, `EList`, `ETuple`, and a nested `EPathMap`
/// through `eval_stable_epathmap`, which checks that map's own bitset and then
/// recurses into its `entries_stable()`. So `entries_stable` ⟹ no `locally_free`
/// anywhere ⟹ blanking is the identity ⟹ the two streams coincide.
#[test]
fn the_o1_discriminator_is_entries_stable_because_the_lf_fold_is_not_hereditary() {
    // A bitset one map-nesting level down: the OUTER entry's own lf is empty.
    let nested = {
        let inner = EPathMap::new(vec![lf_pair().0], Vec::new(), false, None);
        let entry = pathmap_par(inner);
        assert!(
            entry.locally_free.is_empty(),
            "the nested fixture is only interesting if the OUTER entry's own lf is empty"
        );
        EPathMap::new(vec![entry], Vec::new(), false, None)
    };

    // A bitset inside a plain `EList` — not even a map, and still invisible to
    // the top-level fold. Blanking makes this entry `eval_stable`, so its key
    // changes ARM (escape `0x0F` → structural).
    let list_lf = EPathMap::new(
        vec![Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![gint(9)],
                    locally_free: models::create_bit_vector(&[2]),
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        }],
        Vec::new(),
        false,
        None,
    );

    let mut union_guard_was_wrong = 0usize;
    for (label, map) in [
        ("ground", EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None)),
        ("flat lf entry", lf_bearing_map()),
        ("NESTED lf entry", nested),
        ("lf inside a plain EList", list_lf),
    ] {
        let trie = map.entry_trie();
        let stored = map.path_stream();
        let wire = map.wire_path_stream();
        let differs = stored != wire;

        println!(
            "  {label:24} union_lf={:5} entries_stable={:5} |U(stored)|={:3} \
             |U(wire)|={:3} differ={differs}",
            trie.union_locally_free().is_empty(),
            trie.entries_stable(),
            stored.len(),
            wire.len()
        );

        // ★★ THE SOUNDNESS PROPERTY. A discriminator may only take the O(1) arm
        // when the two streams really do coincide.
        assert!(
            !(trie.entries_stable() && differs),
            "★★ `{label}`: `entries_stable` is UNSOUND as the O(1) discriminator — it \
             answered `true` on a map whose blanked key stream DIFFERS. \
             `EntryTrie::wire_path_stream` takes that arm without computing anything, so \
             this is the defect CBR-043 repaired, back again and silent."
        );
        if trie.union_locally_free().is_empty() && differs {
            union_guard_was_wrong += 1;
        }
    }

    // ⚠ THE COUNTER-MEASUREMENT, asserted rather than printed: the rejected
    // guard is not merely unproven, it is WRONG — on two of the four shapes.
    // Pinned as a count so a future refactor that made `union_locally_free`
    // hereditary would fail here and force this file to be re-read, rather than
    // leaving prose claiming an unsoundness that no longer exists.
    assert_eq!(
        union_guard_was_wrong, 2,
        "⚠ `union_locally_free.is_empty()` must be WRONG on exactly the two non-top-level \
         shapes (the nested map and the lf-bearing EList). If this count moved, either the \
         fold became hereditary — in which case `wire_path_stream`'s doc comment is now \
         false — or a fixture stopped exercising the case."
    );
}

// ===========================================================================
// §5  The size the ruling accepted, measured rather than estimated
// ===========================================================================

/// FORM ② costs `8 + |U(m)|` bytes per map over the list form. Printed, and
/// pinned only as an INEQUALITY — the exact figure is a property of the
/// fixtures, and a test that pinned it would fail on every unrelated corpus
/// edit.
///
/// ⚠ Measured against `wire_path_stream()`, which is the stream this surface
/// actually writes. On every fixture here the two coincide (all are
/// `entries_stable`); using the stored one would make the arithmetic accidental
/// rather than derived.
#[test]
fn the_size_delta_is_exactly_the_framed_path_stream() {
    for (label, map) in [
        ("2 ground entries", EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None)),
        ("8 ground entries", EPathMap::new((0..8).map(gint).collect::<Vec<_>>(), Vec::new(), false, None)),
        ("deep(34)", map_at_depth(34)),
        ("empty", EPathMap::new(Vec::new(), Vec::new(), false, None)),
        ("lf-bearing", lf_bearing_map()),
    ] {
        let encoded = bincode::serialize(&map).expect("serialize");
        let path_stream = map.wire_path_stream().len();
        // What the list form would have written: everything except the framed
        // key stream.
        let list_form = encoded.len() - 8 - path_stream;
        println!(
            "  {label:20} bincode {:6} B = {list_form:6} B (list form) + 8 + {path_stream} B (U(m))",
            encoded.len()
        );
        assert_eq!(
            encoded.len(),
            list_form + 8 + path_stream,
            "`{label}`: the delta must be exactly the framed key stream"
        );
    }
}
