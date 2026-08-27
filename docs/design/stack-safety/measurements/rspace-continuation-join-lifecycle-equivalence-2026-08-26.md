# RSpace continuation/join lifecycle equivalence — 2026-08-26

## Claim and boundary

This report tests the final semantic gap between the legacy `HashMap`/`Vec` continuation-and-join
model and the production PathMap authority. It asks whether both representations produce identical
continuation identity, candidate selection, join multiplicity, cold bytes, history roots,
authenticated transfer objects, COMM trace, replay choice, and post-state across one complete
lifecycle.

The claim is **equivalence**, not a throughput result. It does not claim that the planned binary WAL
or content-addressed PathMap-DAG backend is implemented. The PathMap crate is unchanged.

## Independent control

The control is physically test-only at
`rspace++/tests/support/legacy_continuation_join_oracle.rs`. It stores continuations, data, and joins
in `HashMap<K, Vec<V>>`, retains the first representative of a source-free continuation identity,
and preserves every join occurrence. It shares frozen record types and their compatibility
serializers because those bytes are the contract under test, but it does not call the production
PathMap hot store, zipper, candidate-order helper, or checkpoint encoder.

Its candidate order is independently reconstructed as

```text
(BLAKE2b-256(complete canonical record bytes), complete canonical record bytes, legacy vector index)
```

and typed legacy actions independently construct the history-root control.

## Non-vacuous fixture and procedure

The fixture contains two channels, one persistent resting datum, two distinct continuations, a
third source-alias record with the first continuation's source-free identity, and duplicate join
occurrences on both channels. It searches a bounded frozen record population for two continuations
whose scheduler order is opposite their canonical cold-store byte order. Thus agreement cannot be
inherited accidentally from trie traversal order.

The executable procedure is:

1. normalize the legacy snapshot with first-wins identity and compare the live semantic state;
2. checkpoint production and typed legacy actions and compare the initial history root;
3. compare complete authenticated history and cold-leaf object projections;
4. import the initial closure through the affine session, reset both stores, and compare state;
5. inject the missing datum in a private probe, require exactly two enabled candidates, and compare
   their names and original store indexes with the independent scheduler order;
6. produce two distinct persistent arrivals, require two different selected continuations and two
   COMMs, and apply the same exact removals to the control;
7. compare final semantic state and the final typed-history root;
8. replay both arrivals from the initial root and recorded log, require the same choices, exhaust
   replay bindings, and compare the replay root;
9. export, import, reset, and compare the final state once more.

Absent and loaded-empty cache entries are normalized before semantic comparison because the marker
records load state, not RSpace contents.

## Mutation calibration

The fixture kills all nine named mutations:

| Mutant | Required changed observation |
|---|---|
| last-wins continuation identity | selected source digest |
| collapsed duplicate join multiplicity | channel-A join occurrences |
| reversed operational candidate order | selected continuation sequence |
| omitted COMM | COMM count |
| dropped join during reset | channel-B join occurrences |
| dropped checkpoint occurrence | channel-A join occurrences |
| dropped authenticated export object | history-closure cardinality |
| replay selects the next continuation | replay selection sequence |
| removed continuation retained | final continuation cardinality |

## Results

| Gate | Result | Resource policy | Durable evidence |
|---|---:|---|---|
| focused lifecycle differential | 1/1 | current-thread Tokio; bounded parent test scope | included in consolidated log below |
| consolidated `rspace_plus_plus --test mod` | **165/165**, 105.62 s | systemd scope, 8 GiB RSS ceiling, zero swap, 24-core quota | SHA-256 `ff7843d7d89305fd8c8e9c0116279afa1cbd22575891256ee4d679f5d726d5ca` |
| composed formal runner | **PASS** | outer 8 GiB/zero-swap scope; internally capped tools; 16 TLC workers | SHA-256 `6424ceb403d6af09c3648af5b38c00bc74ecc96c7668bb48ce0605a27eabe7c3` |

The formal runner checks the six axiom-free RSpace-commit Rocq capstones, the Verus
canonical-egress/identity/multiplicity/deletion/publication/replay laws without cheating, the Z3
mode-law query, the TLA+ control models and required mutant witnesses, and the mCRL2 publication and
journal properties. Its terminal sentinel is:

```text
stack-safe-pda: Rocq, Verus, Z3, TLC, and mCRL2 verification passed
```

## Decision

The continuation/join PathMap authority and its native canonical-egress cursor preserve the frozen
legacy lifecycle through replay and post-state. CBR-053's verdict and post-state axes can therefore
be recorded as neutral with measured evidence. The next campaign phase is the separate persistent
PathMap-DAG and binary-WAL backend.
