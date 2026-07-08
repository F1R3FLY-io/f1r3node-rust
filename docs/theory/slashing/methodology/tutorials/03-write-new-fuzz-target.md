# 03 · Write a new libFuzzer / cargo-fuzz target

## 1 · Prerequisites

- `cargo-fuzz` installed: `cargo install cargo-fuzz`.
- Nightly Rust toolchain (pinned by `rust-toolchain.toml`).
- Familiarity with the existing six fuzz targets
  ([`fuzz/fuzz_targets/`](../../../../../fuzz/fuzz_targets/)).
- The `arbitrary` crate for structure-aware fuzzing.

## 2 · Skeleton

Create `fuzz/fuzz_targets/<target_name>.rs`:

```rust
//! Fuzz target: <one-sentence purpose>.
//!
//! Drives <function or path> with structured fuzzer input.
//! Asserts <property> on every input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;

mod support;
use support::*;

#[derive(Arbitrary, Debug)]
struct <TargetInput> {
    // <fields with constrained types; the fuzzer mutates each independently>
}

fuzz_target!(|input: <TargetInput>| {
    let scenario = build_scenario(&input);

    // <call the function under test>
    let result = casper::rust::<function>(&scenario);

    // <assert the property>
    assert!(<property holds>, "violation: {:?}", input);
});
```

Register the target in [`fuzz/Cargo.toml`](../../../../../fuzz/Cargo.toml):

```toml
[[bin]]
name = "<target_name>"
path = "fuzz_targets/<target_name>.rs"
test = false
doc = false
```

## 3 · Example from this repo

See [`fuzz/fuzz_targets/slash_authorization_paths.rs`](../../../../../fuzz/fuzz_targets/slash_authorization_paths.rs)
— 271 lines, exercises the full authorization predicate with
structured Block + SlashDeploy inputs.

## 4 · Verification step

Smoke test:

```sh
ASAN_OPTIONS=detect_leaks=0 RUSTFLAGS="-C target-feature=+aes,+sse2" \
    cargo fuzz run <target_name> -- -runs=10000 -max_len=2048
```

The smoke run should complete in ≤ 60 seconds and produce *“Done
N runs in M second(s)”*. Any crash is minimized to
`fuzz/artifacts/<target_name>/<sha>` for review.

Long campaign (background or nightly):

```sh
cargo fuzz run <target_name> -- -max_len=2048
```

## 5 · Common pitfalls

- **`unwrap()` in input decoding** — use `?` or
  `.expect("descriptive message")` in support helpers; never
  panic on cosmetic decode failures.
- **No coverage gain** — run `cargo fuzz cmin <target_name>` after
  one minute; if edge coverage is zero, the target is not
  exercising the path.
- **`#[cfg(fuzzing)]` on the function under test** — the target
  must call the *production* function; only the input decoding may
  diverge.

See [`../randomized-search/03-coverage-guided-fuzzing.md §6`](../randomized-search/03-coverage-guided-fuzzing.md)
for the full anti-pattern catalog.
