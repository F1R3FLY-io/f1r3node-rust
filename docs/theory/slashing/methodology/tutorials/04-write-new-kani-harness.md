# 04 · Write a new Kani harness

## 1 · Prerequisites

- Kani installed: `cargo install --locked kani-verifier && cargo kani setup`.
- Nightly Rust toolchain.
- The function under test is **pure** (no side effects) and
  **loop-free** (or has explicit `#[kani::unwind(N)]` bound).
- A precise mathematical specification of the function's expected
  behavior.

## 2 · Skeleton

Add to the file containing the function under test (e.g.
[`casper/src/rust/slashing_authorization.rs`](../../../../../casper/src/rust/slashing_authorization.rs)),
in a `#[cfg(kani)] mod kani_proofs` block:

```rust
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn <harness_name>() {
        let x: <PrimitiveType> = kani::any();
        kani::assume(<precondition>);  // narrow the domain

        let result = <function_under_test>(x);

        assert!(<property holds>, "violation: x={:?}", x);
    }
}
```

## 3 · Example from this repo

See [`casper/src/rust/slashing_authorization.rs`](../../../../../casper/src/rust/slashing_authorization.rs)
`kani_proofs` module — fifteen harnesses covering the arithmetic
and authorization helpers.

## 4 · Verification step

```sh
RUSTFLAGS='-Aexplicit-builtin-cfgs-in-flags --cfg target_feature="aes" --cfg target_feature="sse2"' \
    cargo kani -p casper --harness <harness_name>
```

Expected output ends with:

```
VERIFICATION:- SUCCESSFUL
```

If verification fails, Kani prints the concrete counterexample:

```
Failed Checks: <property> at <file>:<line>
Counterexample: x = <value>
```

Classify the counterexample via the witness rule.

## 5 · Common pitfalls

- **Function not pure** — Kani cannot reason about side effects;
  restrict harnesses to pure functions.
- **Implicit unbounded loop** — annotate with `#[kani::unwind(N)]`.
- **Type too wide** — `u64` exhausts 2⁶⁴ values; narrow to `u32`
  or `i32` where possible.
- **Multiple properties in one harness** — split into one harness
  per property; counterexamples are easier to interpret.

See [`../formal-methods/03-symbolic-rust-kani.md §5`](../formal-methods/03-symbolic-rust-kani.md)
for the full pitfall catalog.
