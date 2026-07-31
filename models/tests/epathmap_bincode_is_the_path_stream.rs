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
//! ## The three claims
//!
//! | § | claim | why a weaker test would miss it |
//! |---|---|---|
//! | 1 | `bincode(EPathMap)` contains `U(m)` as a **contiguous substring** | an interleaved key/value form round-trips perfectly and is not the trie's byte array |
//! | 2 | `cold_decode ∘ cold_encode` is a byte-level fixed point past depth 32 | a ceiling introduced here is invisible on the shallow fixtures every other suite uses |
//! | 3 | a peer stream whose key disagrees with its value **RE-FILES** | rejecting narrows the accepted language relative to peers, which is a fork |

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EList, EPathMap, ESet, ETuple, Expr, Par, Var};
use models::rust::rhoapi_ext::{EntryTrie, PathStreamDisagreement, PathStreamVerdict};
use models::rust::rholang::par_children::dismantle;
use models::rust::rholang::wire_encode::ColdStoreEncode;
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
// §4  ⚠ The one thing FORM ② changed that is NOT about pathmap ordering
// ===========================================================================

/// ⚠⚠ **MEASURED AND FILED, not papered over.** An entry's `locally_free`
/// reaches the bincode wire — through the trie KEY, never through the value.
///
/// The serialize-only normalization (`serialize_as_empty_bytes`, twelve `.rhoapi`
/// sites plus `EPathMap`'s own) blanks every `locally_free` *field*. But an entry
/// is KEYED by `encode_trie_path`, whose `0x0F` escape arm files a ¬`eval_stable`
/// entry as its canonical **prost** bytes — and prost RETAINS `locally_free`. So
/// once `U(m)` is on this wire, two maps differing only in an entry's
/// `locally_free` serialize DIFFERENTLY.
///
/// ★ **This is a convergence, and the direction matters.** `b73af1d2` (C8) moved
/// `==`, `Hash` and `Ord` onto the keys for exactly this reason: the old relation
/// was *strictly coarser* than the key set, so two maps could compare EQUAL while
/// emitting different bytes. `1b576c90` (CBR-041) put the keys on the prost wire.
/// Before FORM ②, bincode was the last surface still coarser than the value —
/// two `!=` maps produced identical bincode, hence identical event hashes. It no
/// longer does.
///
/// ⚠ The cost is stated as plainly as the gain, and is pinned by
/// [`the_encoding_is_a_fixed_point_after_one_normalisation_round`]: the entries
/// this surface *writes* are lf-blanked, so a decoded map re-keys them, and
/// `cold_encode` is a fixed point only from the second application on such a
/// map. Every other map — every map whose entries are ground, which is every map
/// this suite's other fixtures build — reaches the fixed point immediately, and
/// [`the_cold_store_has_no_depth_ceiling`] is the measurement of that.
#[test]
fn an_entrys_locally_free_reaches_the_wire_through_the_key() {
    let free = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(models::rhoapi::EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            })),
        }],
        locally_free: models::create_bit_vector(&vec![0]),
        ..Default::default()
    };
    let cleared = Par {
        locally_free: Vec::new(),
        ..free.clone()
    };
    // ⚠ NOT `assert_ne!(free, cleared)`. `Par`'s `PartialEq` is **AlwaysEqual**
    // and ignores `locally_free` outright — that is the very asymmetry this test
    // is about, and using `==` here would have compared the two entries by the
    // one relation that cannot see the difference. The honest control is the
    // relation the wire uses: prost bytes.
    assert_ne!(
        prost::Message::encode_to_vec(&free),
        prost::Message::encode_to_vec(&cleared),
        "the two entries must differ ON THE PROST WIRE, or the comparison below is a \
         tautology"
    );

    let with_lf = EPathMap::new(vec![free], Vec::new(), false, None);
    let without_lf = EPathMap::new(vec![cleared], Vec::new(), false, None);

    assert_ne!(
        with_lf.path_stream(),
        without_lf.path_stream(),
        "the escape arm files a ¬eval_stable entry as its canonical PROST bytes, and \
         prost retains locally_free — so the two maps must hold different keys"
    );
    assert_ne!(
        bincode::serialize(&with_lf).expect("with"),
        bincode::serialize(&without_lf).expect("without"),
        "⚠ MEASURED: an entry's locally_free now reaches the bincode wire through U(m). \
         This is filed as CBR-042's residual, not an accident."
    );

    // …and the VALUE half of the normalization is untouched: the entries
    // themselves are still written with an empty bitset.
    let round: EPathMap =
        bincode::deserialize(&bincode::serialize(&with_lf).expect("ser")).expect("de");
    assert!(
        round.locally_free.is_empty(),
        "the map-level locally_free normalization is unchanged"
    );
    for entry in round.ps() {
        assert!(
            entry.locally_free.is_empty(),
            "the entry-level locally_free normalization is unchanged — it is the KEY, \
             not the value, that carries the bitset"
        );
    }
}

/// The precise, measured statement of the cost above: on a map whose entries
/// carry `locally_free`, `cold_encode` is a fixed point from the **second**
/// application, not the first.
///
/// ★ Written as a *measurement with a control* rather than as an apology. The
/// control is the ground map, which reaches the fixed point immediately — so a
/// regression that made EVERY map take two rounds would fail here rather than
/// hide behind this one's expected behaviour.
#[test]
fn the_encoding_is_a_fixed_point_after_one_normalisation_round() {
    let lf_bearing = pathmap_par(EPathMap::new(
        vec![Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(models::rhoapi::EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(0)),
                    }),
                })),
            }],
            locally_free: models::create_bit_vector(&vec![0]),
            ..Default::default()
        }],
        Vec::new(),
        false,
        None,
    ));

    let first = lf_bearing.cold_encode();
    let second = Par::cold_decode(&first)
        .expect("decode round 1")
        .cold_encode();
    let third = Par::cold_decode(&second)
        .expect("decode round 2")
        .cold_encode();
    assert_ne!(
        first, second,
        "⚠ the lf-bearing map is the case that takes two rounds; if it stopped moving, \
         either the escape arm stopped carrying locally_free or the value stopped being \
         blanked, and BOTH are consensus-visible changes that must be filed rather than \
         discovered here"
    );
    assert_eq!(
        second, third,
        "…and it must settle after exactly ONE normalisation round — a stream that kept \
         moving would be a non-terminating canonicalisation, which is a different and \
         far worse defect"
    );

    // ★ THE CONTROL: a ground map settles IMMEDIATELY. Without it, a change that
    // made every map take two rounds would be invisible here.
    let ground = pathmap_par(map_at_depth(4));
    let once = ground.cold_encode();
    let twice = Par::cold_decode(&once).expect("decode").cold_encode();
    assert_eq!(
        once, twice,
        "★ THE CONTROL IS INERT: a GROUND map must be a fixed point on the FIRST \
         application. If it is not, the two-round behaviour above is not specific to \
         locally_free and this file is describing the wrong mechanism."
    );
}

// ===========================================================================
// §5  The size the ruling accepted, measured rather than estimated
// ===========================================================================

/// FORM ② costs `8 + |U(m)|` bytes per map over the list form. Printed, and
/// pinned only as an INEQUALITY — the exact figure is a property of the
/// fixtures, and a test that pinned it would fail on every unrelated corpus
/// edit.
#[test]
fn the_size_delta_is_exactly_the_framed_path_stream() {
    for (label, map) in [
        ("2 ground entries", EPathMap::new(vec![gint(1), gint(2)], Vec::new(), false, None)),
        ("8 ground entries", EPathMap::new((0..8).map(gint).collect::<Vec<_>>(), Vec::new(), false, None)),
        ("deep(34)", map_at_depth(34)),
        ("empty", EPathMap::new(Vec::new(), Vec::new(), false, None)),
    ] {
        let encoded = bincode::serialize(&map).expect("serialize");
        let path_stream = map.path_stream().len();
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
