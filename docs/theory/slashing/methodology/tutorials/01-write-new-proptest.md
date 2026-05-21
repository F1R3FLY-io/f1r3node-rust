# 01 · Write a new proptest property

## 1 · Prerequisites

- Familiarity with the `SlashingTestHarness` API
  ([`casper/tests/slashing/harness.rs`](../../../../../casper/tests/slashing/harness.rs)).
- Knowledge of the property's expected behavior (which theorem or
  invariant it corroborates; see
  [`../../slashing-verification.md`](../../slashing-verification.md)).
- The Rust strategy generators in
  [`casper/tests/slashing/generators.rs`](../../../../../casper/tests/slashing/generators.rs).

## 2 · Skeleton

Create `casper/tests/slashing/prop_t_<name>.rs`:

```rust
// Property-based test for <T-N>: <property in one sentence>.
//
// Theorem: T-N (`<rocq_theorem_name>`,
//   formal/rocq/slashing/theories/<Module>.v).
// Reference: docs/theory/slashing/slashing-specification.md §<N>.
//
// Property: <restate the property here>.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::types::Status;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_n_property_name(
        validator_count in 2usize..8,
        depth in 1u64..10,
        // Add other sampled parameters here, with explicit ranges.
    ) {
        let mut harness = SlashingTestHarness::new(validator_count, 100);
        // <build the scenario through harness API only>
        // <assert the property using prop_assert! / prop_assert_eq!>
    }
}
```

Register the module in
[`casper/tests/slashing/mod.rs`](../../../../../casper/tests/slashing/mod.rs).

## 3 · Example from this repo

See [`casper/tests/slashing/prop_t_1_detection_sound.rs`](../../../../../casper/tests/slashing/prop_t_1_detection_sound.rs)
— 60 lines, exercises Theorem T-1 (*no honest validator is ever
slashed*).

## 4 · Verification step

```
cargo test -p casper --test mod -- slashing::prop_t_<name>
```

The test must pass; the proptest framework will print *“256 cases
passed”* on success.

## 5 · Common pitfalls

- **Implicit precondition** — use `prop_assume!` to declare any
  precondition explicitly.
- **Stateful test in proptest skin** — multi-step lifecycle
  properties belong in Hypothesis; see
  [`06-write-new-hypothesis-state-machine.md`](./06-write-new-hypothesis-state-machine.md).
- **Tautology against the implementation** — the property must be a
  statement about the *behavior*, not a paraphrase of the
  implementation under test.

See [`../randomized-search/01-property-testing-proptest.md §5`](../randomized-search/01-property-testing-proptest.md)
for the full anti-pattern catalog.
