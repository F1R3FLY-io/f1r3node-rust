# Versioned Registry — Pre-existing Blockers Encountered on the `regver` Branch

Things that got in the way during the Step 1–3 implementation work that
are NOT caused by the FIP changes. Filed here so a future cleanup pass
can pick them up; the regver branch routes around them.

## 1. Pre-commit hooks fail on `dev` and `regver` alike

The repo's `.githooks/pre-commit` runs `cargo deny check` and
`cargo clippy --workspace --all-targets`. Both fail on the current
`dev`-derived state even before any of my edits:

### 1a. `cargo deny` — security advisories

- `RUSTSEC` advisory: `proc-macro-error2` flagged unmaintained
  (transitive dependency).
- Plus several "advisory-not-detected" warnings around
  `RUSTSEC-2024-0437` and `RUSTSEC-2025-0119` that the `deny.toml`
  ignore list references but that no longer match any crate.

Net effect: `deny check` exits non-zero.

### 1b. `cargo clippy --workspace --all-targets` — unrelated lints

Several errors in files I never touched, e.g.:

- `casper/src/rust/blocks/block_processor.rs:76` — `unneeded return`.
- `casper/src/rust/engine/lfs_horizon_requester.rs:736-737` —
  `unneeded unit return type` on a couple of trait stub impls.
- A handful more across `casper/src/rust/`.

Net effect: `clippy --workspace --all-targets` exits non-zero.

### Workaround

For commits on `regver`, the pre-commit hooks are skipped via
`SKIP_DENY=1 SKIP_CLIPPY=1 git commit`. This was authorized by the
user for the `regver` work specifically, on the understanding that
the failures are pre-existing.

### Follow-ups (file separately)

- Update `deny.toml` to either resolve the `proc-macro-error2` advisory
  (upstream fix or local override) or remove the now-spurious
  `not-detected` entries from the ignore list.
- Run `cargo clippy --workspace --all-targets --fix` on `dev` and clean
  up the existing lint pile.

## 2. `casper/tests/helper/rho_spec.rs` silently vacuously-passes

The RhoSpec test harness used by every `casper/tests/genesis/contracts/*_spec.rs`
file reports `ok` even when its underlying assertions are wrong.

### Reproduction

Flipping a single assertion expectation in
`casper/src/test/resources/RegistryTest.rho`:

```diff
- rhoSpec!("assert", ((1, "foo"), "== <-", *valueCh), …)
+ rhoSpec!("assert", ((999, "PROBE-WRONG"), "== <-", *valueCh), …)
```

then running `cargo test -p casper --test mod registry_spec` still
reports:

```
test genesis::contracts::registry_spec::registry_spec ... ok
test result: ok. 1 passed; 0 failed; …
```

The flip was reverted immediately. This means the legacy
`registry_spec` (and presumably the rest of the suite using RhoSpec)
has been silently broken for some time.

### Root cause (suspected)

In `casper/tests/helper/rho_spec.rs::run_tests`:

```rust
let result = get_results(...).await?;
for (test_name, test_attempts) in &result.assertions {
    self.mk_test(test_name, test_attempts);
}
Ok(...)
```

If `result.assertions` is empty (no test ever called `rho:test:assertAck`
during the deploy), the `for` does nothing and `Ok` is returned. The
Rust-level `#[test]` passes vacuously.

Empirically the assertions ARE empty. Adding `stdout` debug to
`VersionedRegistryTest.rho` shows the `lookup!(rho:id:zphj…, *RhoSpecCh)`
call producing on `byte_name(14)`, then `for(@(_, RhoSpec) <- RhoSpecCh)`
never satisfying — RhoSpec is never resolved, the `testSuite` driver
never invokes the test contracts, and no assertion is recorded.

The reason `lookup` doesn't return:

- `casper/tests/util/genesis_builder.rs` builds genesis into one
  `scope_id` of the shared LMDB.
- `casper/tests/helper/rho_spec.rs::get_results` then generates a
  *fresh* `scope_id` and creates a new runtime against that
  namespace. Genesis state (including `Registry.rho`'s `lookup`
  contract) does not carry over.
- `bootstrap_registry` re-installs the one-time forwarders for the
  registry URNs, but those are 1-arg matchers. A 2-arg send like
  `lookup!(uri, retCh)` doesn't match and the message just
  accumulates on the fixed channel unconsumed.

Conjecture: this used to work in an earlier shape where the test
runtime DID inherit genesis state (or where the lookup chain was
fully Rust-side), and the migration to LMDB-shared-with-fresh-scope
broke it without anyone noticing because the silent pass-through hides
the regression.

### Workaround for new specs

Don't rely on RhoSpec for behavioral verification of new code. Write
Rust integration tests in the spec file that:

1. Set up a runtime (the same way `get_results` does).
2. Deploy any needed contracts as `extra_libs`.
3. Run a probe `.rho` that sends to the contract under test and
   captures the reply.
4. Assert on the captured reply at the Rust level.

The Step 3 verification in `versioned_registry_spec.rs` follows this
shape.

### Follow-ups (file separately)

- Investigate whether `get_results` should use the genesis `scope_id`
  (so genesis state IS inherited) or whether the test runtime should
  re-install `Registry.rho` automatically.
- Once the lookup chain works in the test runtime, every existing
  `*_spec.rs` will need to be re-validated — many may have been
  silently broken alongside `registry_spec`.
- Add a guard in `run_tests` that asserts a minimum number of
  assertions were recorded (driven by the `testSuite` test list), so
  vacuous passes can never happen again.
