//! # Canonicalising a per-deploy RSpace event log — **S3**, named and stated
//!
//! ## The defect this exists to mask
//!
//! `ProcessedDeploy::deploy_log` is the sequence of RSpace `Produce` / `Consume`
//! / `COMM` events a deploy generated while it executed. **The order it arrives
//! in is not stable**, because it is the order in which the reducer's
//! concurrently-evaluated `Par` members reached the `log_produce` / `log_consume`
//! / `log_comm` seams (`rspace++/src/rspace/rspace.rs`) — not a property of any
//! one container. That distinguishes it from its two siblings:
//!
//! | | what varies | where the fix belongs |
//! |---|---|---|
//! | **S1** `genesis/contracts/proof_of_stake.rs` | `ProofOfStake::validators`, collected out of a `HashMap` | at the source: a `BTreeMap`, or a `Vec` sorted at parse time |
//! | **S2** `genesis/genesis.rs::bonds_proto` | the same `HashMap` | the same source |
//! | **S3** *this module* | the **arrival order of RSpace events** during evaluation | **open** — see the work item; there is no single collection to sort |
//!
//! ⚠ **All three are load-bearing. Deleting any of them INTRODUCES a break that
//! was always latent** — the `f5b2e820` lesson, where two sorts were masking one
//! run-varying pathmap order and removing either alone would have created a
//! consensus break out of a merely-hidden one.
//!
//! ## What this canonicalisation buys, stated as an invariant
//!
//! Let $`L`$ be an event log, $`\pi`$ any permutation, and
//! $`\mathrm{enc} : \mathrm{Event} \to \mathrm{Bytes}`$ the prost encoding of
//! `Event::to_proto`. Then
//!
//! ```math
//! \mathrm{enc}^{*}\bigl(\mathrm{canonicalize}(L)\bigr)
//!   \;=\;
//! \mathrm{enc}^{*}\bigl(\mathrm{canonicalize}(\pi L)\bigr)
//! \qquad\text{for every } \pi ,
//! ```
//!
//! where $`\mathrm{enc}^{*}`$ is the elementwise encoding. In words: **the bytes
//! that reach the block hash are a function of the event MULTISET, not of the
//! order the events arrived in.** That is exactly the property `Body.deploys` →
//! `block_hash` needs, and it is what [`canonicalize_deploy_log`] guarantees.
//!
//! It is a statement about **bytes**, not about `Event` values, and the
//! difference is where the subtlety lives — see *totality* below.
//!
//! ## Totality, and stability under equal keys
//!
//! The comparator is `enc(a).cmp(&enc(b))` over `Vec<u8>`, which is a **total
//! order on the keys** (lexicographic byte order: reflexive, antisymmetric,
//! transitive, and any two byte strings are comparable). It is therefore a
//! **total preorder on `Event`**: total, but not antisymmetric on `Event` itself,
//! because two `Event` values that encode to the same bytes compare `Equal`.
//!
//! Two consequences, and both matter:
//!
//! 1. **No panic, and no unspecified output.** `slice::sort_by` is documented to
//!    possibly panic when handed a comparator that is not a total order
//!    (`user-provided comparison function does not correctly implement a total
//!    order`). A comparator that derived its key from a *nondeterministic*
//!    encoding — a prost message with a `map<K, V>` field, say, whose iteration
//!    order varies per call — would be exactly that, and would put a `panic!` on
//!    the genesis path. `EventProto` and its three payload messages
//!    (`ProduceEventProto`, `ConsumeEventProto`, `CommEventProto`, plus
//!    `PeekProto`) contain **no map fields**: every member is a scalar, `bytes`,
//!    or a `repeated` whose order comes from a `Vec`. prost emits fields in tag
//!    order and omits default-valued ones deterministically, so `enc` is a pure
//!    function and the comparator is consistent.
//! 2. **Ties are invisible to the hash.** `slice::sort_by` is a **stable** sort,
//!    so equal keys keep their arrival order — which means the *sequence of
//!    `Event` values* is NOT fully canonical when two byte-equal events are
//!    present. It does not matter: byte-equal elements are interchangeable in the
//!    concatenation, so $`\mathrm{enc}^{*}`$ of the result is unchanged. The
//!    canonicalisation is complete **on the hashed bytes**, which is the only
//!    thing `Body` carries. (Were the sort ever changed to
//!    `sort_unstable_by`, the invariant above would still hold for the same
//!    reason — the choice of stability is not load-bearing here, and this
//!    paragraph exists so nobody has to re-derive that.)
//!
//! ## ★★ Does REPLAY apply the same canonicalisation? — **it does not, and it
//! must not need to**
//!
//! This was the open question that made S3 potentially a live replay divergence
//! rather than a tidiness issue, and it is settleable from the code.
//!
//! ```text
//!   PLAY                                        REPLAY
//!   ────────────────────────────────────        ─────────────────────────────────
//!   RuntimeOps::process_deploy_cosigned         ReplayRuntimeOps::rig
//!     runtime.take_event_log()                    processed_deploy.deploy_log
//!     → deploy_log in ARRIVAL order                 .map(to_rspace_event)
//!            │                                            │
//!   (genesis only) canonicalize_deploy_log        RhoRuntimeImpl::rig
//!            │  ← THIS MODULE                            │
//!            ▼                                    ReplayRSpace::rig
//!   Body.deploys ▸ block_hash                            │
//!                                                partition into IO / COMM
//!                                                        │
//!                                        new_stuff : HashSet<IOEvent>   ← a SET
//!                                        replay_data: MultisetMultiMap  ← a
//!                                          <IOEvent, COMM>, i.e. a DashMap of
//!                                          per-key COUNTERS
//! ```
//!
//! `ReplayRSpace::rig` (`rspace++/src/rspace/replay_rspace.rs:381-428`) reduces
//! the log to two order-insensitive structures: a `HashSet` of the IO events, and
//! a `MultisetMultiMap<IOEvent, COMM>` built with `add_binding`, which
//! *increments a count* rather than appending to a list
//! (`rspace++/src/rspace/internal.rs:155-169`). Both are functions of the event
//! **multiset**. `replay_data` is therefore identical for a log and for every
//! permutation of it, and replay has no order to canonicalise.
//!
//! ⇒ **There is no play/replay asymmetry on event order**, and — the direction
//! that actually needed establishing — **applying the sort on the play side
//! cannot break replay**, because the input replay reduces is a multiset either
//! way. `casper/tests/deploy_log_canonicalization_and_replay.rs` holds both
//! halves as executable claims, with a negative control showing that a log whose
//! *multiset* differs does move `replay_data`.
//!
//! The same reduction happens a second time on the caching path:
//! `RuntimeManager::replay_payload_hash` sorts each event log's encodings before
//! hashing them into the `ReplayCacheKey`
//! (`casper/src/rust/util/rholang/runtime_manager.rs:156-166`), so two blocks
//! carrying the same event multiset in different orders share one cache entry —
//! consistent with `rig`, and not an accident worth breaking.
//!
//! ## What is still open
//!
//! **Where the arrival order becomes nondeterministic.** This module masks the
//! symptom for the one digest that needs cross-run reproducibility; it does not
//! localise the cause. That remains the substance of the work item.

use models::rust::casper::protocol::casper_message::Event;
use prost::Message;

/// The canonical encoding of one event — the sort key, and the bytes that reach
/// `Body.deploys`.
///
/// Exposed because a test that recomputed it would be a **second copy of a
/// computable thing**, and two copies of one truth do not stay equal. Every
/// assertion about the sort's totality, stability or permutation-invariance is
/// made against *this* function, so it cannot drift from the comparator.
pub fn event_key(event: &Event) -> Vec<u8> {
    event.to_proto().encode_to_vec()
}

/// Canonicalise a per-deploy RSpace event log in place.
///
/// ⚠ **LOAD-BEARING. Do not delete this call, and do not "fix" S3 by deleting
/// it** — see the module documentation for why removing it would turn a hidden
/// defect into a live consensus break.
///
/// After this returns, [`event_key`] applied elementwise is a function of the
/// input multiset alone. The operation is idempotent.
pub fn canonicalize_deploy_log(log: &mut [Event]) {
    // ★ THE SAME PERMUTATION AS THE `sort_by` THIS REPLACED, PROVABLY, AND ONE
    // ENCODING PER EVENT INSTEAD OF Θ(n log n) OF THEM.
    //
    // `sort_by_cached_key` extracts every key once, forms `(key, index)` pairs
    // — which are pairwise distinct because the indices are — and sorts those.
    // Sorting distinct `(key, index)` pairs orders equal keys by ascending
    // original index, which is precisely what a STABLE sort by key does. So the
    // output is identical to `sort_by(|a, b| event_key(a).cmp(&event_key(b)))`
    // for every input, INCLUDING inputs with byte-equal ties, while the number
    // of prost encodings drops from one per comparison to one per element.
    log.sort_by_cached_key(event_key);
}

/// `true` iff `log` is already canonical under [`canonicalize_deploy_log`].
///
/// ★ This is the *verifier* half, and it exists so that a claim about the sort
/// can be checked without re-running the sort — in particular so a test can
/// assert "the builder's output is canonical" rather than "the builder's output
/// equals what I get when I sort it myself", which would pass even if both sides
/// were wrong in the same way.
pub fn is_canonical(log: &[Event]) -> bool {
    let mut previous: Option<Vec<u8>> = None;
    for event in log {
        let key = event_key(event);
        match previous {
            Some(ref last) if *last > key => return false,
            _ => previous = Some(key),
        }
    }
    true
}
