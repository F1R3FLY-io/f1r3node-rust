//! Shared test-support utilities for the multi-sig + LL-rich algebra
//! property-based test suite (Phase 4). Provides:
//!
//! - `any_sig()` / `any_sig_bounded(depth, leaves)`: proptest strategies
//!   over the full `Sig` algebra (Unit, Ground, Quote, And, Threshold,
//!   Plus, With, Bang, WhyNot, Lolly).
//! - `channel_eq(&a, &b)`: thin wrapper around `SignatureChannel::eq` for
//!   readability at proptest call sites. `SignatureChannel` already
//!   derives `PartialEq`, and `from_sig` post-applies `ParSortMatcher::
//!   sort_match`, so this is true *canonical* channel-level equivalence.
//! - Atom-pool helpers: `tiny_atom_pool()` / `with_pool(rng_seed, size)`
//!   for property tests that need a small finite alphabet of leaf atoms
//!   (so different `Hash` bytes don't shadow non-trivial Sig structure).

#![allow(dead_code)] // many helpers are used by sibling test files

use proptest::prelude::*;
use rholang::rust::interpreter::accounting::{Sig, SignatureChannel};

/// Canonical channel-level equivalence. Two `Sig` expressions are
/// channel-equivalent iff their `SignatureChannel::from_sig` reflections
/// match after `ParSortMatcher::sort_match` canonicalization.
pub fn channel_eq(a: &Sig, b: &Sig) -> bool {
    SignatureChannel::from_sig(a) == SignatureChannel::from_sig(b)
}

/// Bounded-depth `Sig` strategy. Every `Sig` variant is reachable. Atom
/// payloads are drawn from a 4-element pool by default so non-trivial
/// repetition shows up in random samples.
///
/// - `depth`: maximum recursive nesting (clamps `prop_recursive` height).
///   Depth ≤ 5 keeps the search space tractable.
/// - `leaves`: maximum total nodes across the whole expression.
pub fn any_sig_bounded(depth: u32, leaves: u32) -> impl Strategy<Value = Sig> {
    // Generate BOTH atom axes (ground `g` and quote `#P`) so the proto
    // round-trip and channel-equivalence properties exercise the
    // AtomKind tag on both branches. `any_atom_payload()` returns an opaque
    // (non-Clone) Strategy, so we instantiate it once per branch.
    let leaf = prop_oneof![
        2 => Just(Sig::Unit),
        4 => any_atom_payload().prop_map(Sig::Ground),
        4 => any_atom_payload().prop_map(Sig::Quote),
    ];
    leaf.prop_recursive(depth, leaves, 4, |inner| {
        prop_oneof![
            4 => (inner.clone(), inner.clone())
                .prop_map(|(l, r)| Sig::And(Box::new(l), Box::new(r))),
            2 => (inner.clone(), inner.clone())
                .prop_map(|(l, r)| Sig::Plus(Box::new(l), Box::new(r))),
            2 => (inner.clone(), inner.clone())
                .prop_map(|(l, r)| Sig::With(Box::new(l), Box::new(r))),
            1 => inner.clone().prop_map(|s| Sig::Bang(Box::new(s))),
            1 => inner.clone().prop_map(|s| Sig::WhyNot(Box::new(s))),
            2 => (inner.clone(), inner.clone())
                .prop_map(|(l, r)| Sig::Lolly(Box::new(l), Box::new(r))),
            2 => threshold_strategy(inner),
        ]
    })
}

/// Default `Sig` strategy: depth 4, ≤ 32 nodes — small enough that
/// proptest runs stay snappy, large enough to exercise all combinators.
pub fn any_sig() -> impl Strategy<Value = Sig> {
    any_sig_bounded(4, 32)
}

/// Atom-payload strategy: 1–4 bytes drawn from a 4-element pool so
/// duplicate hashes appear at reasonable frequency.
fn any_atom_payload() -> impl Strategy<Value = Vec<u8>> {
    let pool: Vec<u8> = (0xA0u8..=0xA3u8).collect();
    proptest::collection::vec(prop::sample::select(pool), 1..=4)
}

/// Threshold-connective strategy: 1 ≤ threshold ≤ members.len(), with
/// 1–4 members of the inner strategy.
fn threshold_strategy(inner: BoxedStrategy<Sig>) -> impl Strategy<Value = Sig> {
    proptest::collection::vec(inner, 1..=4).prop_flat_map(|members| {
        let n = members.len();
        (1u32..=n as u32).prop_map(move |k| Sig::Threshold {
            threshold: k,
            members: members.clone(),
        })
    })
}

/// Deterministic hand-picked atom set. Useful for hand-written sanity
/// tests that pair with the proptest blocks — the same atoms appear
/// across `lhs` and `rhs` expressions so equivalences are exercised.
pub fn fixed_atoms() -> [Sig; 4] {
    [
        Sig::Ground(vec![0xA0]),
        Sig::Ground(vec![0xA1]),
        Sig::Ground(vec![0xA2]),
        Sig::Ground(vec![0xA3]),
    ]
}
