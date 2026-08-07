//! # S3 — the per-deploy event-log order, and the two questions that decide it
//!
//! Work item **#171**. The per-deploy RSpace event log arrives in a
//! nondeterministic order and is canonicalised at the last moment, on the genesis
//! path only, by `casper::rust::util::event_log_canonical::canonicalize_deploy_log`
//! (called from `Genesis::create_processed_deploy`). Two questions decide whether
//! that is a live replay-divergence risk or a masked tidiness defect, and both are
//! settleable — so this file settles them, executably.
//!
//! ## Q1 — does REPLAY apply the same canonicalisation?
//!
//! **No, and it does not need to.** `ReplayRSpace::rig`
//! (`rspace++/src/rspace/replay_rspace.rs:381-428`) does not keep the log. It
//! reduces it to two order-insensitive structures:
//!
//! ```text
//!   log : Vec<Event>
//!     │
//!     ├─ partition ─────────────────────────────────────────────────┐
//!     │                                                            │
//!     ▼                                                            ▼
//!   io_events                                                  comm_events
//!     │                                                            │
//!     ▼                                                            ▼
//!   new_stuff : HashSet<IOEvent>              for each COMM, for each of its
//!     (a SET — duplicates collapse,             (consume ∪ produces) that is in
//!      order is not represented)                new_stuff:
//!                                                 replay_data.add_binding(io, comm)
//!                                                        │
//!                                                        ▼
//!                                        MultisetMultiMap<IOEvent, COMM>
//!                                          = DashMap<IOEvent, Counter<COMM>>
//!                                          — `add_binding` INCREMENTS A COUNT
//!                                            (`internal.rs:155-169`); it does
//!                                            not append to a list, so insertion
//!                                            order is not represented either
//! ```
//!
//! Every operation in that reduction is a function of the event **multiset**.
//! [`rig_is_order_insensitive`] holds this as a measured claim over the real
//! `ReplayRSpace`, driven through the real `casper::rust::util::event_converter`
//! hop, with a **negative control** ([`rig_distinguishes_a_different_multiset`])
//! showing the same projection does move when the multiset changes — without
//! which "equal under every permutation" would also be satisfied by a `rig` that
//! built nothing at all.
//!
//! ⇒ The direction that actually needed establishing is the converse:
//! **canonicalising on the play side cannot break replay.** It cannot, because
//! replay reduces a multiset either way.
//!
//! ## Q2 — is the sort's own comparator sound?
//!
//! Three properties, each with its own test:
//!
//! | property | test | why it matters |
//! |---|---|---|
//! | the key function is a **pure function** (deterministic per call) | [`the_event_key_is_deterministic`] | a nondeterministic key makes `sort_by` an inconsistent comparator — Rust may then `panic!` with *"user-provided comparison function does not correctly implement a total order"*, i.e. a panic on the genesis path |
//! | the key order is **total** | [`the_key_order_is_total`] | a partial comparator reintroduces exactly the nondeterminism the sort was added to remove |
//! | **permutation invariance of the hashed bytes**, including under byte-equal ties | [`canonicalization_is_invariant_under_permutation`], [`ties_do_not_defeat_canonicalization`] | this is the property `Body.deploys` → `block_hash` needs |
//!
//! ★ And [`the_canonicalization_is_load_bearing`] shows the un-canonicalised log
//! DOES vary, so none of the above is vacuously true of an already-sorted fixture.
//!
//! ## What this file does NOT claim
//!
//! It does not localise **where** the arrival order becomes nondeterministic.
//! That is the open remainder of #171 and needs its own experiment; nothing here
//! should be read as closing it.

use std::collections::BTreeMap;
use std::sync::Arc;

use casper::rust::util::event_converter::to_rspace_event;
use casper::rust::util::event_log_canonical::{canonicalize_deploy_log, event_key, is_canonical};
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::casper::protocol::casper_message::{
    CommEvent, ConsumeEvent, Event, Peek, ProduceEvent,
};
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::trace::event::Event as RspaceEvent;

// ---------------------------------------------------------------------------
// fixtures — synthetic events with hand-chosen hashes
// ---------------------------------------------------------------------------

/// A 32-byte hash whose bytes are determined by `tag`, so every fixture event is
/// distinguishable and every ordering question has an unambiguous answer.
fn hash_bytes(tag: u8) -> prost::bytes::Bytes { prost::bytes::Bytes::from(vec![tag; 32]) }

fn produce_event(channel: u8, datum: u8) -> ProduceEvent {
    ProduceEvent {
        channels_hash: hash_bytes(channel),
        hash: hash_bytes(datum),
        persistent: false,
        times_repeated: 0,
        is_deterministic: true,
        output_value: Vec::new(),
        failed: false,
    }
}

fn consume_event(channel: u8, tag: u8) -> ConsumeEvent {
    ConsumeEvent {
        channels_hashes: vec![hash_bytes(channel)],
        hash: hash_bytes(tag),
        persistent: false,
    }
}

/// A log with the three event shapes, two COMMs sharing one consume, and a
/// duplicate produce — i.e. every structural feature `rig` treats specially:
///
/// * an IO event that is NOT part of any COMM (`new_stuff` keeps it, no binding);
/// * an IO event that IS part of a COMM (a binding is added);
/// * **two distinct COMMs bound to the SAME `IOEvent` key**, which is the only
///   configuration in which `Counter<COMM>` holds more than one value for a key —
///   the case a list-valued multimap would have made order-sensitive;
/// * a duplicated IO event, which the `HashSet` collapses and the counter counts.
fn representative_log() -> Vec<Event> {
    let shared_consume = consume_event(0x10, 0x11);
    vec![
        Event::Produce(produce_event(0x20, 0x21)),
        Event::Consume(shared_consume.clone()),
        Event::Comm(CommEvent {
            consume: shared_consume.clone(),
            produces: vec![produce_event(0x20, 0x21)],
            peeks: vec![Peek { channel_index: 0 }],
        }),
        Event::Comm(CommEvent {
            consume: shared_consume.clone(),
            produces: vec![produce_event(0x30, 0x31)],
            peeks: Vec::new(),
        }),
        Event::Produce(produce_event(0x30, 0x31)),
        // the duplicate
        Event::Produce(produce_event(0x20, 0x21)),
        Event::Consume(consume_event(0x40, 0x41)),
    ]
}

/// The elementwise encoding — the bytes that reach `Body.deploys`.
fn encoded(log: &[Event]) -> Vec<Vec<u8>> { log.iter().map(event_key).collect() }

// ---------------------------------------------------------------------------
// Q2 — the sort's own soundness
// ---------------------------------------------------------------------------

/// ★ **The key function is a pure function of the event.**
///
/// This is the property whose failure would be worst, and it is invisible to
/// every other test in this file: a key derived from an encoding with
/// nondeterministic field order (a prost `map<K, V>`, whose iteration order
/// varies per call) makes the comparator inconsistent, and `slice::sort_by` is
/// documented to possibly `panic!` — *"user-provided comparison function does not
/// correctly implement a total order"* — when handed one. That would put a panic
/// on the genesis path.
///
/// `EventProto` / `ProduceEventProto` / `ConsumeEventProto` / `CommEventProto` /
/// `PeekProto` contain no map fields; this asserts the consequence rather than
/// trusting the schema, over enough repetitions that a `HashMap`-order dependency
/// would have to be extraordinarily unlucky to hide.
#[test]
fn the_event_key_is_deterministic() {
    for event in representative_log() {
        let first = event_key(&event);
        for repetition in 1..64 {
            assert_eq!(
                first,
                event_key(&event),
                "event_key is NOT a pure function: repetition {repetition} of \
                 {event:?} produced different bytes. The genesis sort's comparator \
                 is then inconsistent, which Rust may answer with a panic."
            );
        }
    }
}

/// ★ **The key order is TOTAL** — every pair is comparable, comparison is
/// antisymmetric on keys, and it is transitive.
///
/// Checked exhaustively over the fixture's `n(n+1)/2` pairs and `n³` triples
/// rather than argued from `Vec<u8>: Ord`, because the claim that matters is
/// about the *composition* `event ▸ to_proto ▸ encode_to_vec ▸ cmp` and a lossy
/// or non-deterministic stage anywhere in it would break the composition while
/// leaving `Vec<u8>: Ord` intact.
#[test]
fn the_key_order_is_total() {
    let log = representative_log();
    let keys: Vec<Vec<u8>> = encoded(&log);

    for (i, a) in keys.iter().enumerate() {
        for (j, b) in keys.iter().enumerate() {
            let forward = a.cmp(b);
            let backward = b.cmp(a);
            assert_eq!(
                forward,
                backward.reverse(),
                "the key order is not antisymmetric at ({i}, {j})"
            );
            if i == j {
                assert_eq!(
                    forward,
                    std::cmp::Ordering::Equal,
                    "the key order is not reflexive at {i}"
                );
            }
        }
    }

    for a in &keys {
        for b in &keys {
            for c in &keys {
                if a <= b && b <= c {
                    assert!(
                        a <= c,
                        "the key order is not transitive: {:?} ≤ {:?} ≤ {:?} but not \
                         {:?} ≤ {:?}",
                        a,
                        b,
                        c,
                        a,
                        c
                    );
                }
            }
        }
    }
}

/// ★★ **THE INVARIANT: the hashed bytes are a function of the event MULTISET.**
///
/// Over every permutation of a 7-element log (5,040 of them), the encoded byte
/// sequence after canonicalisation is identical. This is exactly the property
/// `Body.deploys` → `block_hash` needs, and it is stated over *bytes* rather than
/// over `Event` values on purpose — see [`ties_do_not_defeat_canonicalization`].
///
/// [`is_canonical`] is asserted alongside so the claim is not merely "two runs of
/// the same sort agree" (which a broken sort would also satisfy) but "the result
/// is in canonical order".
#[test]
fn canonicalization_is_invariant_under_permutation() {
    let base = representative_log();
    let mut expected: Option<Vec<Vec<u8>>> = None;
    let mut permutations = 0usize;

    for_each_permutation(&base, &mut |permuted: &[Event]| {
        let mut log = permuted.to_vec();
        canonicalize_deploy_log(&mut log);
        assert!(
            is_canonical(&log),
            "canonicalize_deploy_log left a log that is NOT in canonical order"
        );
        let bytes = encoded(&log);
        match expected {
            None => expected = Some(bytes),
            Some(ref first) => assert_eq!(
                *first, bytes,
                "CANONICALISATION IS NOT PERMUTATION-INVARIANT. Two orderings of \
                 the same event multiset canonicalised to different bytes, so the \
                 genesis block hash depends on the arrival order of RSpace events \
                 — which is precisely the defect this sort exists to mask."
            ),
        }
        permutations += 1;
    });

    assert_eq!(
        permutations, 5_040,
        "VACUOUS TEST: the permutation generator produced {permutations} orderings \
         of a 7-element log instead of 7! = 5,040, so most orderings were never \
         checked"
    );
}

/// ★ **Idempotence.** Canonicalising a canonical log is a no-op, which is what
/// lets a caller apply it defensively (or twice, through two code paths) without
/// changing the digest.
#[test]
fn canonicalization_is_idempotent() {
    let mut once = representative_log();
    canonicalize_deploy_log(&mut once);
    let mut twice = once.clone();
    canonicalize_deploy_log(&mut twice);
    assert_eq!(
        encoded(&once),
        encoded(&twice),
        "canonicalize_deploy_log is not idempotent"
    );
}

/// ★★ **TIES — the case where the sort is NOT a total order on `Event`, only on
/// its key, and where that turns out not to matter.**
///
/// The comparator compares encodings, so two events with equal encodings compare
/// `Equal`. `sort_by_cached_key` is a stable sort, so those ties keep their
/// arrival order — meaning the *sequence of `Event` values* is not fully
/// canonical when a log contains byte-equal duplicates.
///
/// The reason that is sound rather than a hole: byte-equal elements are
/// interchangeable in the concatenation, so the property that reaches the block
/// hash — the encoded byte sequence — is unchanged. This test makes that concrete
/// by building a log that is **half duplicates** and showing that all of its
/// permutations still canonicalise to one byte sequence.
///
/// ⚠ It would be wrong to "fix" the tie by extending the comparator with a
/// secondary key over some non-encoded field: there is no such field (the
/// encoding is what `Body` carries), so a secondary key could only be derived
/// from arrival position — which is the nondeterministic thing.
#[test]
fn ties_do_not_defeat_canonicalization() {
    let duplicate = Event::Produce(produce_event(0x20, 0x21));
    let other = Event::Consume(consume_event(0x10, 0x11));
    let base = vec![
        duplicate.clone(),
        other.clone(),
        duplicate.clone(),
        other.clone(),
        duplicate.clone(),
        other.clone(),
    ];

    // The fixture really is tied: three pairs of byte-equal events.
    let keys = encoded(&base);
    let mut distinct = keys.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        2,
        "VACUOUS TEST: the tie fixture has {} distinct keys, not 2 — it is not \
         exercising ties at all",
        distinct.len()
    );

    let mut expected: Option<Vec<Vec<u8>>> = None;
    for_each_permutation(&base, &mut |permuted: &[Event]| {
        let mut log = permuted.to_vec();
        canonicalize_deploy_log(&mut log);
        let bytes = encoded(&log);
        match expected {
            None => expected = Some(bytes),
            Some(ref first) => assert_eq!(
                *first, bytes,
                "a log containing BYTE-EQUAL events canonicalised to different \
                 bytes under two permutations — the tie is reaching the digest"
            ),
        }
    });
}

/// ★★ **THE CANONICALISATION IS LOAD-BEARING, and this is the RED that proves the
/// other tests are not vacuous.**
///
/// Every assertion above would also hold of a fixture whose orderings were
/// already byte-identical — for instance a log of seven copies of one event. This
/// test asserts the opposite for the same fixture: **without** canonicalisation
/// the encoded byte sequences of two permutations DIFFER.
///
/// So `canonicalization_is_invariant_under_permutation` is a statement about the
/// function, not about the fixture. And the pair together is what a future
/// `#[test]` cannot satisfy by deleting the sort: delete it, and this test still
/// passes while that one fails.
#[test]
fn the_canonicalization_is_load_bearing() {
    let base = representative_log();
    let mut reversed = base.clone();
    reversed.reverse();

    assert_ne!(
        encoded(&base),
        encoded(&reversed),
        "VACUOUS FIXTURE: the log's encodings are permutation-invariant even \
         WITHOUT canonicalisation, so every other test in this file is trivially \
         satisfied. Rebuild `representative_log` with distinguishable events."
    );

    let mut canonical_base = base;
    let mut canonical_reversed = reversed;
    canonicalize_deploy_log(&mut canonical_base);
    canonicalize_deploy_log(&mut canonical_reversed);
    assert_eq!(
        encoded(&canonical_base),
        encoded(&canonical_reversed),
        "the same fixture that differs unsorted must agree sorted"
    );
}

/// ★★ **THE REFACTOR IS BYTE-FOR-BYTE THE OLD SORT.**
///
/// `canonicalize_deploy_log` uses `sort_by_cached_key(event_key)`. What stood at
/// `genesis.rs:232-241` before it was named was
///
/// ```text
/// deploy.deploy_log.sort_by(|a, b| {
///     let a_bytes = a.to_proto().encode_to_vec();
///     let b_bytes = b.to_proto().encode_to_vec();
///     a_bytes.cmp(&b_bytes)
/// });
/// ```
///
/// The two agree for every input, and the argument is short enough to state:
/// `sort_by_cached_key` extracts each key once, pairs it with its original index,
/// and sorts those pairs — which are pairwise distinct — so equal keys come out in
/// ascending original-index order, exactly as a stable sort by key produces. The
/// swap therefore changes the number of prost encodings from Θ(n log n) to n and
/// changes nothing observable.
///
/// ★ This test keeps the **pre-refactor body** as the oracle rather than arguing
/// from the documentation, and checks it over all 5,040 permutations of the
/// representative log plus the tie fixture — because "byte-equal ties" is the only
/// input class where a stability difference could show, and it is the class the
/// argument above turns on. The duplication is deliberate and bounded: this is the
/// only place in the tree that spells the old comparator, and it exists to be
/// compared against, not to be maintained in parallel.
#[test]
fn the_named_canonicalization_equals_the_inline_sort_it_replaced() {
    fn the_original_inline_sort(log: &mut [Event]) {
        use prost::Message;
        log.sort_by(|a, b| {
            let a_bytes = a.to_proto().encode_to_vec();
            let b_bytes = b.to_proto().encode_to_vec();
            a_bytes.cmp(&b_bytes)
        });
    }

    let duplicate = Event::Produce(produce_event(0x20, 0x21));
    let other = Event::Consume(consume_event(0x10, 0x11));
    let fixtures = [representative_log(), vec![
        duplicate.clone(),
        other.clone(),
        duplicate.clone(),
        other,
        duplicate,
    ]];

    let mut compared = 0usize;
    for base in fixtures {
        for_each_permutation(&base, &mut |permuted: &[Event]| {
            let mut named = permuted.to_vec();
            let mut original = permuted.to_vec();
            canonicalize_deploy_log(&mut named);
            the_original_inline_sort(&mut original);
            assert_eq!(
                encoded(&named),
                encoded(&original),
                "THE REFACTOR CHANGED THE PERMUTATION. `sort_by_cached_key` and the \
                 pre-refactor `sort_by` disagree on this ordering, so naming the \
                 sort was NOT behaviour-preserving and the genesis block hash has \
                 moved."
            );
            compared += 1;
        });
    }
    assert_eq!(
        compared,
        5_040 + 120,
        "VACUOUS TEST: {compared} orderings compared instead of 7! + 5! = 5,160"
    );
}

// ---------------------------------------------------------------------------
// Q1 — replay's view of the log
// ---------------------------------------------------------------------------

type RholangReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

fn in_memory_store() -> RSpaceStore {
    RSpaceStore {
        history: Arc::new(InMemoryKeyValueStore::new()),
        roots: Arc::new(InMemoryKeyValueStore::new()),
        cold: Arc::new(InMemoryKeyValueStore::new()),
    }
}

fn fresh_replay_space() -> RholangReplay {
    let (_play, replay) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            in_memory_store(),
            Arc::new(Box::new(Matcher)),
        )
        .expect("create_with_replay must succeed over in-memory stores");
    replay
}

/// A canonical, comparable projection of `ReplayRSpace::replay_data`.
///
/// `replay_data` is a `DashMap<IOEvent, Counter<COMM>>`; neither the `DashMap`
/// nor the `Counter` (a `HashMap`) has a stable iteration order, so a projection
/// has to sort. The projection is over `Debug` renderings, which is legitimate
/// here for one reason and one reason only: every component of `IOEvent` and
/// `COMM` renders deterministically (`Vec<u8>` hashes, `BTreeSet` peeks,
/// `BTreeMap` times_repeated, and a `produces` list that `COMM::new` sorts by
/// `(channel_hash, hash, persistent)`), so the rendering is injective enough to
/// separate the fixtures and stable across calls. It is compared only to another
/// projection produced the same way.
fn replay_data_projection(space: &RholangReplay) -> BTreeMap<String, Vec<(String, usize)>> {
    let guard = space.replay_data.lock().expect("replay data lock");
    let mut out: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    for entry in guard.map.iter() {
        let mut values: Vec<(String, usize)> = entry
            .value()
            .iter()
            .map(|(comm, count)| (format!("{comm:?}"), *count))
            .collect();
        values.sort();
        out.insert(format!("{:?}", entry.key()), values);
    }
    out
}

fn rig_projection(log: &[Event]) -> BTreeMap<String, Vec<(String, usize)>> {
    let rspace_log: Vec<RspaceEvent> = log.iter().map(to_rspace_event).collect();
    let space = fresh_replay_space();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async {
            ISpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::rig(
                &space, rspace_log,
            )
            .await
            .expect("rig must succeed");
        });
    replay_data_projection(&space)
}

/// ★★ **REPLAY DOES NOT OBSERVE THE LOG ORDER.** The answer to Q1, measured
/// against the real `ReplayRSpace` over the real `event_converter` hop.
///
/// All 5,040 permutations of the representative log are rigged and the resulting
/// `replay_data` is projected and compared. If any permutation moved it, a
/// validator whose block carried a differently-ordered log could rig a different
/// replay table and diverge — and the play-side canonicalisation would be
/// changing replay's input rather than only the digest.
///
/// ⇒ Since it does not move, two things follow, and the second is the one that
/// needed establishing:
///
/// 1. replay has no order to canonicalise, so there is no play/replay asymmetry;
/// 2. **applying `canonicalize_deploy_log` on the play side cannot break
///    replay** — the sorted log and the arrival-order log rig identically.
#[test]
fn rig_is_order_insensitive() {
    let base = representative_log();
    let expected = rig_projection(&base);

    assert!(
        !expected.is_empty(),
        "VACUOUS TEST: rigging the representative log produced an EMPTY \
         replay_data, so 'every permutation agrees' is the agreement of nothing. \
         The fixture must contain COMM events whose IO events also appear \
         standalone in the log."
    );

    let mut permutations = 0usize;
    for_each_permutation(&base, &mut |permuted: &[Event]| {
        assert_eq!(
            expected,
            rig_projection(permuted),
            "REPLAY IS ORDER-SENSITIVE. Rigging a permutation of the same event \
             multiset produced a different replay_data table. `rig` is documented \
             to reduce the log to a HashSet plus a counter-valued multimap, both \
             functions of the multiset; if that is no longer true then the genesis \
             canonicalisation is changing replay's input and S3 is a live \
             divergence, not a masked one."
        );
        permutations += 1;
    });
    assert_eq!(
        permutations, 5_040,
        "VACUOUS TEST: only {permutations} of 7! = 5,040 permutations were rigged"
    );
}

/// ★ **THE NEGATIVE CONTROL for [`rig_is_order_insensitive`].**
///
/// "Every permutation agrees" is satisfied by a projection that cannot tell
/// anything apart. This shows the same projection DOES separate logs that differ
/// in their multiset — one event added, one event's channel changed, and one COMM
/// removed — so the order-insensitivity result is a property of `rig` and not of
/// the instrument.
#[test]
fn rig_distinguishes_a_different_multiset() {
    let base = representative_log();
    let baseline = rig_projection(&base);

    // (a) one extra COMM bound to the shared consume.
    let mut extra_comm = base.clone();
    extra_comm.push(Event::Comm(CommEvent {
        consume: consume_event(0x10, 0x11),
        produces: vec![produce_event(0x50, 0x51)],
        peeks: Vec::new(),
    }));
    extra_comm.push(Event::Produce(produce_event(0x50, 0x51)));
    assert_ne!(
        baseline,
        rig_projection(&extra_comm),
        "THE PROJECTION CANNOT GO RED: adding a COMM (and the produce it binds) \
         left replay_data unchanged, so it cannot witness order-sensitivity either"
    );

    // (b) a COMM removed.
    let fewer: Vec<Event> = base
        .iter()
        .filter(|event| !matches!(event, Event::Comm(_)))
        .cloned()
        .collect();
    assert_ne!(
        baseline,
        rig_projection(&fewer),
        "THE PROJECTION CANNOT GO RED: removing every COMM left replay_data \
         unchanged"
    );

    // (c) one produce's IDENTITY changed — same shape, same count, different key.
    //
    // ⚠ `ProduceEvent::hash`, not `channels_hash`. See
    // [`a_produces_channel_hash_is_not_part_of_its_replay_identity`] for the
    // finding that made this distinction necessary: a first draft of this control
    // relabelled `channels_hash` and went RED, because `Produce`'s `Hash`/`Eq`
    // read `self.hash` alone.
    let moved: Vec<Event> = base
        .iter()
        .map(|event| match event {
            Event::Produce(pe) if pe.hash == hash_bytes(0x31) => {
                Event::Produce(produce_event(0x30, 0x61))
            }
            other => other.clone(),
        })
        .collect();
    assert_ne!(
        baseline,
        rig_projection(&moved),
        "THE PROJECTION CANNOT GO RED: relabelling a produce's identity hash left \
         replay_data unchanged"
    );
}

/// ★★ **CHARACTERISED, NOT ASSERTED-AWAY: a `Produce`'s `channels_hash` plays no
/// part in its replay identity.**
///
/// `Produce`'s manual `Hash` / `Eq` / `Ord` impls
/// (`rspace++/src/rspace/trace/event.rs:112-124`) read `self.hash` and nothing
/// else, so `rig`'s `HashSet<IOEvent>` and `MultisetMultiMap<IOEvent, COMM>` are
/// keyed on the identity hash alone. Relabelling `channels_hash` on a produce
/// event that arrives from a block therefore does **not** move `replay_data`.
///
/// **Why that is sound**, and it took looking up to be sure: in production
/// `Produce::create` sets `hash = hash_produce(channel_hash.bytes(), datum,
/// persistent)` — the channel is *folded into* the identity hash, so
/// `channel_hash` is redundant with `hash` for identity purposes and a
/// hash-only comparison loses nothing. A pair that disagrees is
/// **unconstructible by the production path**.
///
/// **What is nonetheless worth recording:** `to_rspace_event` reconstructs both
/// fields verbatim from a block's `ProduceEventProto`, and nothing on the replay
/// path re-derives `hash` from `channels_hash`. So a *hand-crafted* block can
/// carry a produce event whose two fields disagree, and replay will not notice.
/// It is inert — every replay decision reads `hash`, and `COMM`'s derived `Eq`
/// compares `produces` through `Produce::eq`, which is also hash-only — but it
/// is an unvalidated field riding in a consensus message, which is the shape that
/// deserves to be written down rather than discovered twice.
///
/// This test asserts the CURRENT behaviour. If a future change makes
/// `channels_hash` part of the identity, this goes RED and the paragraph above
/// has to be re-derived — which is the point.
#[test]
fn a_produces_channel_hash_is_not_part_of_its_replay_identity() {
    let base = representative_log();
    let baseline = rig_projection(&base);

    let relabelled: Vec<Event> = base
        .iter()
        .map(|event| match event {
            Event::Produce(pe) if pe.channels_hash == hash_bytes(0x30) => {
                // same identity hash (0x31), a DIFFERENT channel hash
                Event::Produce(produce_event(0x60, 0x31))
            }
            other => other.clone(),
        })
        .collect();

    // The fixture really did change — otherwise this test proves nothing.
    assert_ne!(
        encoded(&base),
        encoded(&relabelled),
        "VACUOUS TEST: the relabelled log encodes identically to the base one, so \
         no produce's channels_hash was actually changed"
    );

    assert_eq!(
        baseline,
        rig_projection(&relabelled),
        "A produce's `channels_hash` HAS become part of its replay identity. That \
         is not necessarily wrong, but the reasoning recorded on this test — that \
         hash-only identity is lossless because `Produce::create` folds the \
         channel into `hash` — no longer describes the code and must be redone."
    );
}

// ---------------------------------------------------------------------------
// the permutation generator
// ---------------------------------------------------------------------------

/// Every permutation of `items`, by Heap's algorithm, applied to `visit`.
///
/// Iterative rather than recursive so the generator cannot itself be the reason a
/// large fixture fails, and exhaustive rather than sampled because 7! = 5,040 is
/// small and a sampled order-invariance claim is weaker than it looks.
fn for_each_permutation<T: Clone>(items: &[T], visit: &mut impl FnMut(&[T])) {
    let n = items.len();
    let mut working = items.to_vec();
    let mut counters = vec![0usize; n];
    visit(&working);

    let mut i = 1usize;
    while i < n {
        if counters[i] < i {
            let swap_with = if i % 2 == 0 { 0 } else { counters[i] };
            working.swap(swap_with, i);
            visit(&working);
            counters[i] += 1;
            i = 1;
        } else {
            counters[i] = 0;
            i += 1;
        }
    }
}

/// ★ The permutation generator is itself checked, because every exhaustiveness
/// claim in this file rests on it. Heap's algorithm silently degrades to fewer
/// than `n!` visits if the counter reset is misplaced, and a test that visited 6
/// of 5,040 orderings would look identical from the outside.
#[test]
fn the_permutation_generator_is_exhaustive() {
    for n in 1..=7usize {
        let items: Vec<usize> = (0..n).collect();
        let mut seen: Vec<Vec<usize>> = Vec::new();
        for_each_permutation(&items, &mut |p: &[usize]| seen.push(p.to_vec()));

        let expected: usize = (1..=n).product();
        assert_eq!(
            seen.len(),
            expected,
            "Heap's algorithm visited {} orderings of {n} items, expected {n}! = \
             {expected}",
            seen.len()
        );

        seen.sort();
        let before = seen.len();
        seen.dedup();
        assert_eq!(
            seen.len(),
            before,
            "the generator repeated an ordering of {n} items, so its visit count \
             overstates its coverage"
        );
    }
}
