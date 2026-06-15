# Versioned Registry Implementation Plan

Branch: `regver`
Date: 2026-06-14
Scope: implement the [Versioned Registry FIP](../FIPS/fileio/approved/2025-09-16-Versioned-Registry) on top of the existing `rho:registry:*` machinery in this repo. This is Stage A of the four-stage rollout that ends in the File I/O FIP.

## Goal

Augment today's flat `rho:registry:lookup` / `insertArbitrary` / `insertSigned` surface with a versioned-URN scheme that supports semver wildcards, a `lib` vs. `serve` namespace split, the canonical `getX!?(*notify)` import pattern with a deprecation channel, and three new registry methods:
`insertVersion`, `deprecateVersion`, `approveVersion`. The existing flat URNs stay registered for backwards compatibility.

## Coexistence with the existing registry

The new design is **additive**. The legacy `rho:registry:lookup`, `rho:registry:insertArbitrary`, `rho:registry:insertSigned:secp256k1`, and `rho:registry:ops` URNs stay registered with the same fixed channels and the same handlers. The genesis-installed `Registry.rho` keeps its current `_registryStore` channel and its current `lookup` / `insertArbitrary` / `insertSigned` contracts untouched.

The new versioned surface lives next to the legacy one without conflict:

- **URN strings are disjoint.** Legacy: `rho:registry:lookup`, `rho:registry:insertArbitrary`, `rho:registry:insertSigned:secp256k1`, `rho:registry:ops`, plus URI keys of the form `rho:id:…`. New: `rho:registry:<version>` (with a version segment, e.g. `1.0.0` or the wildcard `1.*`), `rho:lib:…`, `rho:serve:…`. No URN string is shared.
- **Fixed channels are disjoint.** Legacy uses `FixedChannels::reg_lookup()` / `reg_insert_random()` / `reg_insert_signed()` / `reg_ops()` (`system_processes.rs:87-190`). The new surface adds `FixedChannels::reg_v1()` (or one per major) without touching the existing ones.
- **Storage channels are disjoint.** Legacy contracts read and write `_registryStore` (`casper/src/main/resources/Registry.rho:483`). New contracts read and write a brand-new `_versionedRegistryStore` channel. No restructure of the existing TreeHashMap.
- **Resolution paths are disjoint.** The legacy URNs are exact-match keys in `urn_map` and resolve through the existing `add_urn` fast path. The new URNs all carry a version segment that the new resolver recognizes; the existing `add_urn` doesn't change behavior for legacy URNs.

The one *intentional* design consequence: a value inserted via `insertArbitrary` (legacy) is not visible to `lookup`-by-version (new), and vice versa. They're two stores. If a future FIP wants unified discovery, that's its problem; this one keeps them separate.

## What we reuse without modifying

- The genesis-installed Rholang contract pattern: new versioned contracts live in `Registry.rho` (or a sibling file included from genesis) alongside the existing ones.
- The `TreeHashMap` storage backend (`Registry.rho:483`) — the new `_versionedRegistryStore` is just another TreeHashMap instance.
- The `Definition` registration scheme (`rho_runtime.rs:511-715`, `:1013-1044`) — new versioned URNs slot in as additional `Definition` rows.
- The `RhoSpec` test pattern (`casper/tests/genesis/contracts/registry_spec.rs`, `casper/src/test/resources/RegistryTest.rho`) — the new tests are a parallel file pair.

## What's new in this branch

1. A **semver parser and matcher** with wildcard support (`1.*`, `2.6.*`, `*`), preliminary-release rejection (`2.6.3-alpha` cannot match a wildcard), and total order. New module: `rholang/src/rust/interpreter/registry/semver.rs`. Pure Rust, no I/O.
2. **URN parsing for the versioned forms** at lookup time:
   - `rho:lib:<lib_ver>:<pub_key>:<project_id>:<project_ver>`
   - `rho:serve:<serve_ver>:<pub_key>:<project_id>:<project_ver>`
   - `rho:registry:<reg_ver>`
   The version segments may contain `*` wildcards.
3. **A two-phase URN resolver** in `eval_new`: try the exact `urn_map` lookup first (fast path for flat URNs); if missed, hand to the versioned resolver, which parses the URN, scans the appropriate namespace in the registry store, and binds the highest matching, non-deprecated version. Edits at `rholang/src/rust/interpreter/reduce.rs:1293-1359`.
4. **Three new registry contracts** in `Registry.rho`: `insertVersion`, `deprecateVersion`, `approveVersion`. Plus an extended store layout that segments the existing TreeHashMap into `lib`-namespace and `serve`-namespace subtrees and tags every entry with version metadata and a deprecation flag.
5. **The deprecation `notify` channel** plumbing: the registry's lookup path records the caller-supplied notify channel on a per-entry list; `deprecateVersion` walks that list and emits a one-shot warning down each. The notify list lives in the same TreeHashMap entry as the code reference.
6. **The two-name return shape** the FIP describes: looking up `rho:lib:1.*:…` now hands back a single agent bundle. Internally the registry's lookup process reads two names from the caller in `!?` style: the endpoint channel and the notify channel.
7. The **versioned `rho:registry:1.*`** entry point that exposes `insertArbitrary`, `insertSigned`, `insertVersion`, `deprecateVersion`, `approveVersion`, `lookup`. The old flat URNs (`rho:registry:lookup`, `rho:registry:insertArbitrary`, `rho:registry:insertSigned:secp256k1`) stay registered and resolve to the same underlying handlers, but new code should use the versioned form.

## Out of scope

- The `lib`-namespace static check that "code uses only temp names" — the FIP marks this as a goal; statically detecting persistent-name use requires type information that doesn't exist yet. Ship a runtime check (the contract is rejected if `insertVersion` is called with `"lib"` and the deployed code creates persistent names during execution) and leave the static version for a follow-up.
- API-stability enforcement on minor/patch upgrades — same reason; the FIP says "should enforce ... once we have type information."
- Migrating the existing genesis entries to versioned form. They get an implicit `1.0.0`, but a real migration plan is a separate concern.

## Plan, in build order

### Step 1 — semver module ✅ DONE (2026-06-14)

Landed in `rholang/src/rust/interpreter/registry/semver.rs` and exported from `registry/mod.rs`. The module provides:

- `Version { major: u32, minor: u32, patch: u32, pre: Option<String> }` with semver `Ord` (release > prerelease, prerelease tags compared lexicographically), `Display`, `Hash`.
- `Pattern` enum — `Exact(Version)`, `PatchWild { major, minor }`, `MinorWild { major }`, `MajorWild`. Mid-component wildcards like `1.*.3` are rejected as `SemverError::MidComponentWildcard`.
- `parse_version` (strict; `*` rejected) and `parse_pattern` (accepts all four FIP-enumerated wildcard shapes).
- `Pattern::matches` enforcing the FIP rule that wildcards never match prereleases.
- `Pattern::best_match` for picking the highest matching `Version` from a candidate iterator.
- `SemverError` enum with `Display + Error` for all parse failure modes.

**Verification status:** 28 unit tests pass. Run with `cargo test -p rholang --lib registry::semver`.

### Step 2 — add a new versioned store in a sibling file

Create `casper/src/main/resources/VersionedRegistry.rho`. It declares a top-level channel `_versionedRegistryStore`, initialized analogously to how `Registry.rho:483` initializes `_registryStore`. The new store is its own TreeHashMap with two top-level keys:

- `"lib"` → a sub-TreeHashMap keyed by `(pub_key, project_id)`, whose values are maps keyed by version string, whose values are `{ code, deprecated, notify_channels }`.
- `"serve"` → mirror of `"lib"` for stateful services.

Embed the new file the same way `Registry.rho` is embedded: add `pub const VERSIONED_REGISTRY: &str = include_str!("../../../main/resources/VersionedRegistry.rho");` to `casper/src/rust/genesis/contracts/embedded_rho.rs` next to the existing `REGISTRY` constant, and add it to the genesis compile/deploy sequence in the same order Registry is deployed (versioned must come after the base registry only if it consumes it; otherwise either order works — both write distinct stores).

`Registry.rho` is not edited.

#### Testing

There's no user-callable surface yet, so the testable surface is small:

1. **Parse / normalize check (Rust unit).** New test in the rholang crate (or in `casper/src/rust/genesis/contracts/`) that pulls `embedded_rho::VERSIONED_REGISTRY` and runs it through the same normalize path the genesis loader uses; asserts no compile errors. Catches typos in the new `.rho` file without spinning up a runtime.
2. **Genesis-deploy smoke (Rust integration).** Run the standard genesis path with the new file embedded; assert the runtime initializes cleanly. If `_versionedRegistryStore` fails to install, genesis errors out and this fails.
3. **Legacy regression.** `cargo test -p casper registry_spec` and `cargo test -p casper registry_ops_spec` must pass unmodified — a checkpoint before moving to Step 3.
4. **Skeleton spec pair.** Create `casper/src/test/resources/VersionedRegistryTest.rho` and `casper/tests/genesis/contracts/versioned_registry_spec.rs` with a single placeholder `test_genesis_loaded` that asserts `true == true`. Proves the RhoSpec harness can find the new spec file. Real cases land at Step 3 and later.

What we don't test at Step 2: anything about the store's contents (no contracts read or write it yet) or the resolver (doesn't exist until Step 5).

### Step 3 — three new Rholang contracts (in the new sibling file)

These contracts operate on `_versionedRegistryStore` only. They never read or write `_registryStore`.

- `insertVersion(ret, namespace, deployerId, projectId, version, code)` — namespace is `"lib"` or `"serve"`. Returns `true`/`false` per FIP. Failure modes the contract checks: `deployerId` doesn't match the public key in the URN; version already exists; if namespace is `"lib"`, run a runtime check that the inserted code doesn't introduce persistent names (see the lib check below).
- `deprecateVersion(ret, namespace, deployerId, projectId, version)` — sets the `deprecated` flag and emits a warning on every channel currently in that version's `notify_channels` list. Returns `true`/`false`.
- `approveVersion(ret, namespace, deployerId, projectId, version)` — clears the `deprecated` flag. Returns `true`/`false`.

Use the existing `ops!("buildUri", pubKeyBytes, *uriCh)` mechanism for any URI bookkeeping the new contracts need.

#### Lib runtime check (Step 3, deferred fallback)

To meet the FIP's "code uses only temp names" requirement without a static analysis: at `insertVersion` time for `"lib"`, compile the candidate code, run it in a sandboxed deploy, and reject if any persistent-name production is observed. If that's too heavy, ship Step 3 *without* the check (accept all `"lib"` inserts) and file a follow-up issue; the check is non-blocking for the rest of the FIP.

#### Testing

The new contracts at this step are bound to internal channels inside `VersionedRegistry.rho` and won't be user-callable until Step 6 wires up `rho:registry:1.0.0`. So testing here is about exercising the contracts through a temporary handle, then ripping the handle out at Step 6:

1. **Test-only handle.** Add a single fixed channel `rho:registry:v1:internal` (or similar) registered for this step only, bundle-wrapping the three contracts. RhoSpec tests reach them through this URN. The URN goes away at Step 6 in favor of the public `rho:registry:1.0.0`. Mark with a `// TODO(Step 6): remove` comment so it's obvious in review.
2. **Contract-level tests in `VersionedRegistryTest.rho`**, replacing the Step 2 placeholder:
   - `test_insertVersion_lib_happy_path` — insert `1.0.0`, observe `true`.
   - `test_insertVersion_serve_happy_path` — same for `"serve"`.
   - `test_insertVersion_duplicate_rejected` — same `(ns, pubkey, projectId, version)` twice → second returns `false`.
   - `test_insertVersion_deployer_mismatch_rejected` — `deployerId` not derivable from the URN's `pub_key` → `false`.
   - `test_deprecateVersion_sets_flag` — insert, deprecate, then read the store directly and assert the `deprecated` field is `true`.
   - `test_approveVersion_clears_flag` — deprecate, approve, assert flag is `false`.
   - If the lib runtime check ships at this step: `test_lib_persistent_name_rejected`.
3. **Legacy regression** still green: `cargo test -p casper registry_spec`.

What we don't test at Step 3: anything about the resolver picking versions by pattern (no resolver yet) or about lookup ordering (lookup doesn't exist as a method until Step 6).

### Step 4 — URI parsing helper, exposed via a new ops URN

The existing `rho:registry:ops` handler (`system_processes.rs:651-675`) only accepts `"buildUri"` and is referenced by the legacy `Registry.rho`. Don't add arms to it — extending its dispatch surface would change the legacy file's call sites. Instead, register a new `rho:registry:ops:1.0.0` (with its own `FixedChannels::reg_ops_v1()` and `BodyRefs::REG_OPS_V1`) that the new contracts call. It can support the same `"buildUri"` for code that prefers the versioned form, plus the new operation:

- `"parseVersionedUri"(uri_str)` — splits a `rho:lib:…` / `rho:serve:…` URN into its segments. Returns a tuple or `Nil`. Lets `insertVersion` validate the projectId/deployerId match without re-parsing in Rholang.

The new handler lives next to `registry_ops` in `system_processes.rs`; copy the dispatch shape (`match on the first arg`) rather than editing the existing one.

#### Testing

1. **Rust unit tests on `parseVersionedUri`** in `system_processes.rs` (a `#[cfg(test)] mod tests` block at the bottom, following the pattern crypto handlers already use). Cover: well-formed `rho:lib:…` and `rho:serve:…` URNs, malformed URNs, URNs with wildcards in version segments (parse-only — the resolver decides what to do with them), empty segments. Don't go through a full runtime — call the parsing helper directly.
2. **Rholang-level smoke** in `VersionedRegistryTest.rho`:
   - `test_ops_v1_buildUri_matches_legacy` — call `rho:registry:ops:1.0.0` with `"buildUri"` and the legacy `rho:registry:ops` with the same byte array; assert the returned URI is identical. Proves the new handler doesn't drift from the old one on the shared op.
   - `test_ops_v1_parseVersionedUri_lib` — round-trip parse on a known `rho:lib:1.0.0:<pk>:proj:2.6.3` URN, assert each segment matches.
   - `test_ops_v1_parseVersionedUri_malformed` — assert `Nil` (or whatever the error sentinel is) for garbage input.
3. **Legacy regression**: `cargo test -p casper registry_ops_spec` passes — the original `rho:registry:ops` handler is untouched.

### Step 5 — versioned URN resolution in `eval_new` (additive)

Edit `rholang/src/rust/interpreter/reduce.rs:1333-1346` (the `add_urn` closure inside `eval_new`). The existing exact-match-then-injections sequence keeps its behavior for every legacy URN. We add one new branch *between* them:

```
// Current shape (pseudo):
if self.urn_map.contains_key(&urn) {
    bind from urn_map           // legacy fast path — unchanged
} else {
    check injections             // legacy fallback — unchanged
}
```

becomes:

```
if self.urn_map.contains_key(&urn) {
    bind from urn_map           // legacy fast path — unchanged
} else if let Some(req) = parse_versioned_urn(&urn) {
    match resolve_versioned(req)? {
        Some(par) => bind par,
        None => Err(InterpreterError::ReduceError(
            format!("No matching version for urn: {}", urn))),
    }
} else {
    check injections             // legacy fallback — unchanged
}
```

`parse_versioned_urn` returns `Some(req)` only for the new URN shapes (`rho:lib:…`, `rho:serve:…`, `rho:registry:<version>`); it returns `None` for everything else, so legacy URNs that happen to miss `urn_map` still fall through to the injections path unchanged.

`resolve_versioned` is a new function. It:
1. Uses the parsed `(namespace, pub_key, project_id, version_pattern)` from Step 1 and Step 4.
2. Sends a message into the new versioned registry's lookup channel with the parsed parameters and a fresh return channel.
3. `await`s the response. The new contract reads the candidate set out of `_versionedRegistryStore`, filters by pattern + deprecation, and replies with the chosen version's stored channel.
4. Returns the resulting `Par`.

Storage logic stays in Rholang; the Rust side just routes. Legacy `_registryStore` is never touched.

#### Testing

1. **Rust unit tests on `parse_versioned_urn`** (added in the same module as the new branch). Each new URN shape (`rho:lib:…`, `rho:serve:…`, `rho:registry:<ver>`) parses to the expected `(namespace, pub_key, project_id, version_pattern)` shape; every legacy URN (`rho:registry:lookup`, `rho:registry:ops`, `rho:io:stdout`, etc.) returns `None` so the existing `urn_map`/injection paths still own them.
2. **Rust integration tests on `eval_new`** in `reduce.rs`-adjacent test modules. Three required cases:
   - *Legacy URN in `urn_map`* — bind via the fast path, no resolver call. Assert by mocking the resolver to panic if it's reached.
   - *Versioned URN, miss* — store has nothing matching; the new branch returns `InterpreterError::ReduceError` and the `new` doesn't bind.
   - *Versioned URN, hit* — store has a matching version; the new branch binds the stored `Par`. Stub the registry lookup so the test doesn't depend on Steps 3/6 yet.
3. **End-to-end resolver tests** in `VersionedRegistryTest.rho`, now that the resolver can be exercised through `new x(`rho:lib:…`)`:
   - `test_resolve_exact_version` — insert `1.0.0`, look up exact, get the right code back.
   - `test_resolve_patch_wildcard` — insert `1.0.0`, `1.0.1`, `1.0.2`, look up `1.0.*`, expect `1.0.2`.
   - `test_resolve_minor_wildcard` — across `1.0.x` and `1.1.x`.
   - `test_resolve_major_wildcard`.
   - `test_prerelease_skipped_by_wildcard` — insert `1.0.0` and `1.1.0-alpha`, look up `1.*`, expect `1.0.0`.
   - `test_resolve_miss_errors` — look up `7.*` with nothing matching; assert the deploy errors out (rather than silently binding `Nil`).
4. **Legacy URN regression**: `cargo test -p rholang` (covers `eval_new`) plus the casper registry suites — all green without modification.

### Step 6 — the `rho:registry:1.*` entry point

Register a new `Definition` row in `rho_runtime.rs:511-715`:

```
Definition {
    urn: "rho:registry:1.0.0".to_string(),
    fixed_channel: FixedChannels::reg_v1(),
    arity: 2,   // endpoint channel + notify channel, per `!?`
    body_ref: BodyRefs::REG_V1,
    handler: ...,
    remainder: None,
}
```

`FixedChannels::reg_v1()` is a new entry alongside `reg_ops()`. `BodyRefs::REG_V1` is a new dispatch id.

The handler reads the two-name shape from the caller (endpoint and notify) and replies with the agent bundle for the v1 API. Since the v1 API is itself a Rholang contract (the new `insertVersion`/`deprecateVersion`/`approveVersion` + the existing operations), the handler's job is just to forward the lookup request into the contract and pass the agent bundle back. Roughly: the handler reads two `Par`s (endpoint, notify), then `produce`s the bundled endpoint to the endpoint channel and registers the notify in the registry store's per-entry list.

Resolution of `rho:registry:1.*` goes through Step 5's versioned resolver, hitting the `rho:registry:1.0.0` entry — same machinery as any other versioned URN.

#### Testing

This is the step where the temporary internal-channel handle from Step 3 goes away (delete the `// TODO(Step 6): remove` block) and the contracts become reachable through the public URN. All the contract tests written for Step 3 get re-pointed at the public URN; nothing else about them changes.

1. **Migration sweep on `VersionedRegistryTest.rho`** — replace every `for (api <- internalCh)` opening with `new getReg(`rho:registry:1.*`), notify in { for (reg <- getReg!?(*notify)) { ... } }`. All Step 3 / Step 5 tests must continue to pass against the new entry point.
2. **`!?` two-name shape sanity**:
   - `test_v1_returns_bundle` — the resolved value behaves like a bundle (can be sent to but not received from on the recv side).
   - `test_v1_notify_channel_captured` — install a notify channel via `getReg!?(*notify)`, then assert deprecation-driven traffic on `notify` once Step 7 lands (placeholder marker now; activated then).
3. **Version-pattern coverage on the entry-point URN** — every wildcard shape from Step 5's resolver tests, but invoked through `rho:registry:*`, `rho:registry:1.*`, and `rho:registry:1.0.*` (not just `rho:lib:…`). Same machinery, different namespace.
4. **Legacy URN coexistence** — `rho:registry:lookup` (legacy) and `rho:registry:1.*` (new) can both be `new`-bound in the same `.rho` file without conflict. Add `test_legacy_and_v1_coexist` covering this.
5. **Full regression** — `cargo test -p rholang -p casper`.

### Step 7 — deprecation notify wiring

Two pieces:

- *Recording*: when `resolve_versioned` (Step 5) successfully picks a version, the registry contract appends the caller's notify channel to the chosen version's `notify_channels` list in the store. (The Rust side passes the notify channel down; the contract does the append.)
- *Firing*: `deprecateVersion` walks the list and `send`s a one-shot warning down each channel. After firing, the list stays — repeated `deprecateVersion` calls re-fire (idempotent at the contract level; clients dedupe).

The notify channel is a per-resolution capability, not a per-version one. Two different `getFS!?(*notify)` calls produce two distinct entries in the same version's list.

#### Testing

All Rholang-level, in `VersionedRegistryTest.rho`:

1. `test_deprecate_notify_fires_to_one_listener` — install a single notify channel via lookup, call `deprecateVersion`, assert exactly one message arrives on the notify channel within a small time bound.
2. `test_deprecate_notify_fires_to_many_listeners` — same lookup from three independent `for` blocks (three distinct notify channels), deprecate once, assert each receives the message.
3. `test_deprecate_then_lookup_attaches_no_new_notifies` — after a version is already deprecated, a new lookup should still resolve (until Step 8 changes that — see below) but its notify channel should *also* fire immediately (the new client has just adopted a deprecated dep). Pick one semantics and lock it down in this test; the FIP allows either but should be consistent.
4. `test_approve_then_deprecate_refires` — verify that `approve` → `deprecate` re-fires the warning. Documents the per-resolution semantics.
5. `test_deprecateVersion_skipped_in_lookup` — the resolver from Step 5 must now filter out deprecated versions: insert `1.0.0`, `1.0.1`, `1.0.2`, deprecate `1.0.2`, look up `1.0.*`, expect `1.0.1`. (This is the join point between Step 5 and Step 7. If Step 5's resolver was written naively without the deprecation filter, fix it here.)
6. `test_approveVersion_restores` — undeprecate and verify the lookup now picks the restored version again.

If the notify-firing path uses a different produce mechanism than the rest of the registry (e.g., emits onto a list of channels in parallel), add a Rust-level test in the casper crate that verifies the cost-charging on `deprecateVersion` scales linearly with the notify-list length — guards against an accidental O(n²) implementation.

### Step 8 — examples, tutorial, and cross-cutting end-to-end test

By this step the substantive behavioral tests have already been added in Steps 2–7 (each step's own `#### Testing` subsection contributes contracts to `VersionedRegistryTest.rho`). Step 8 is what's left:

- `rholang/examples/tut-versioned-registry.rho` — a copy of `tut-registry.rho` adapted to the new surface, suitable for the docs. Should exercise every public method (`insertVersion`, `deprecateVersion`, `approveVersion`, lookup via wildcard, lookup via exact). The example must run cleanly via `cargo run --release --bin rholang-cli rholang/examples/tut-versioned-registry.rho` and exit 0.

#### Cross-cutting tests added here

These verify properties that span steps and don't naturally fit in any earlier per-step block:

1. **`test_full_lifecycle`** in `VersionedRegistryTest.rho` — single contract that performs: `insertVersion 1.0.0` → lookup `1.*` → `insertVersion 1.0.1` → lookup `1.*` (now returns `1.0.1`) → `deprecateVersion 1.0.1` → lookup `1.*` (back to `1.0.0`) → `approveVersion 1.0.1` → lookup `1.*` (back to `1.0.1`). All four state transitions in a single run with shared state.
2. **`test_back_compat_flat_lookup`** — the legacy `rho:registry:lookup` / `insertArbitrary` path still works inside a deploy that also uses `rho:registry:1.*`. Asserts the disjointness claim from "Coexistence with the existing registry" above isn't broken by some accidental shared mutable state.
3. **`test_concurrent_inserts_different_projects`** — two `insertVersion` calls in parallel branches of the same deploy, different `projectId`s, both succeed. Tests the TreeHashMap concurrency story for `_versionedRegistryStore`.
4. **Tutorial-as-test.** Wire `tut-versioned-registry.rho` into the test harness as an additional integration case so docs and code can't drift apart.

### Step 9 — back-compat verification

Because the change is additive (no edits to `_registryStore`, the legacy contracts, the legacy fixed channels, or the legacy `registry_ops` arity-3 handler), the existing test suite should pass without any changes to test code:

- `cargo test -p rholang`
- `cargo test -p casper registry_spec`
- `cargo test -p casper registry_ops_spec`

If any of these break, the patch is doing more than it should be — investigate before relaxing. The legacy `insertArbitrary` / `lookup` paths, including the `rho:lang:either` and other shorthand mappings at `Registry.rho:492-511`, must behave bit-for-bit as today.

## File touch list

| File | What changes |
|---|---|
| `rholang/src/rust/interpreter/registry/semver.rs` | NEW — parser, matcher, ordering, tests |
| `rholang/src/rust/interpreter/registry/mod.rs` | Add `pub mod semver;` |
| `rholang/src/rust/interpreter/reduce.rs` (around lines 1333-1346) | Insert one new `else if` arm for versioned URNs between the existing exact-match and injection-fallback branches. Existing branches unchanged. |
| `rholang/src/rust/interpreter/rho_runtime.rs` (around 511-715) | Add new `Definition` rows for `rho:registry:1.0.0` and `rho:registry:ops:1.0.0`. Existing rows untouched. |
| `rholang/src/rust/interpreter/system_processes.rs` (around 87-190) | Add `FixedChannels::reg_v1()`, `FixedChannels::reg_ops_v1()`, `BodyRefs::REG_V1`, `BodyRefs::REG_OPS_V1`. Existing constants untouched. |
| `rholang/src/rust/interpreter/system_processes.rs` (new method alongside `registry_ops` at 651-675) | NEW `registry_ops_v1` handler with `"buildUri"` and `"parseVersionedUri"`. The original `registry_ops` is left as-is. |
| `casper/src/main/resources/VersionedRegistry.rho` | NEW — sibling to `Registry.rho`. Declares `_versionedRegistryStore` and the three contracts plus a versioned `lookup`. |
| `casper/src/rust/genesis/contracts/embedded_rho.rs` | Add `pub const VERSIONED_REGISTRY: &str = include_str!(...);` and wire it into the genesis loading sequence next to `REGISTRY`. |
| `casper/src/test/resources/VersionedRegistryTest.rho` | NEW — RhoSpec contracts |
| `casper/tests/genesis/contracts/versioned_registry_spec.rs` | NEW — Rust spec wrapper |
| `rholang/examples/tut-versioned-registry.rho` | NEW — walkthrough |

`casper/src/main/resources/Registry.rho`, `casper/src/test/resources/RegistryTest.rho`, `casper/src/test/resources/RegistryOpsTest.rho`, and the legacy `registry_ops` handler do **not** appear in this list — they should remain byte-identical to `main`.

## Verification before merge

- `cargo test -p rholang -p casper` green.
- The existing `RegistryTest.rho` + `RegistryOpsTest.rho` pass without edits.
- The new `VersionedRegistryTest.rho` passes.
- `tut-registry.rho` (the existing example) still runs through
  `cargo run --release --bin rholang-cli rholang/examples/tut-registry.rho`.
- `tut-versioned-registry.rho` runs through the same CLI and exits 0.

## What this unblocks

Stage B of the four-FIP rollout (Agents pre-elaboration sugar) can land independently of this branch, but Stage D (File I/O) needs *this* branch merged first so file/dir agents can be registered under `rho:io:fs:1.*` instead of as hardcoded `Definition` rows.
