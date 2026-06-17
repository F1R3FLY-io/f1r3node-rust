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

### Step 2 — add a new versioned store in a sibling file ✅ DONE (2026-06-14)

Landed as a minimal placeholder. `casper/src/main/resources/VersionedRegistry.rho` allocates `_versionedRegistryStore` with empty `"lib"` and `"serve"` sub-maps and exits — no contracts yet, no fixed-channel bootstrap. The full TreeHashMap-backed store layout described above (sub-TreeHashMaps keyed by `(pub_key, project_id)` → version → `{ code, deprecated, notify_channels }`) will arrive in Step 3 once the contracts that read/write it exist; the current revision uses a plain Rholang `Map` only as a parsing-and-genesis-deploy smoke target.

Wiring:

- `casper/src/main/resources/VersionedRegistry.rho` — new file.
- `casper/src/rust/genesis/contracts/embedded_rho.rs` — new `pub const VERSIONED_REGISTRY` constant next to `REGISTRY`.
- `casper/src/rust/genesis/contracts/standard_deploys.rs` — new `VERSIONED_REGISTRY_PK`, `VERSIONED_REGISTRY_TIMESTAMP`, `VERSIONED_REGISTRY_PUB_KEY`, `versioned_registry()` function, plus an entry in `system_public_keys()`.
- `casper/src/rust/genesis/genesis.rs` — `versioned_registry` is sequenced immediately after `registry` in `default_blessed_terms_with_timestamp`; `all_deploys` capacity bumped from 11 to 12.

`Registry.rho` is not edited.

**Verification status:**

- `cargo test -p casper --test mod versioned_registry_spec` passes.
- `cargo test -p casper --test mod standard_deploys` passes (includes the fast `versioned_registry_embedded_source_compiles` parse/normalize check).
- `cargo test -p casper --test mod registry` passes — legacy `registry_spec`, `registry_ops_spec`, and `multi_parent_casper_should_be_able_to_use_the_registry` all green unchanged.

#### Testing

There's no user-callable surface yet, so the testable surface is small:

1. **Parse / normalize check (Rust unit).** New test in the rholang crate (or in `casper/src/rust/genesis/contracts/`) that pulls `embedded_rho::VERSIONED_REGISTRY` and runs it through the same normalize path the genesis loader uses; asserts no compile errors. Catches typos in the new `.rho` file without spinning up a runtime.
2. **Genesis-deploy smoke (Rust integration).** Run the standard genesis path with the new file embedded; assert the runtime initializes cleanly. If `_versionedRegistryStore` fails to install, genesis errors out and this fails.
3. **Legacy regression.** `cargo test -p casper registry_spec` and `cargo test -p casper registry_ops_spec` must pass unmodified — a checkpoint before moving to Step 3.
4. **Skeleton spec pair.** Create `casper/src/test/resources/VersionedRegistryTest.rho` and `casper/tests/genesis/contracts/versioned_registry_spec.rs` with a single placeholder `test_genesis_loaded` that asserts `true == true`. Proves the RhoSpec harness can find the new spec file. Real cases land at Step 3 and later.

What we don't test at Step 2: anything about the store's contents (no contracts read or write it yet) or the resolver (doesn't exist until Step 5).

### Step 3 — three new Rholang contracts (in the new sibling file) ✅ DONE (2026-06-15)

Landed in `casper/src/main/resources/VersionedRegistry.rho`. The file's `new` block now defines three persistent contracts on a local `v1Api` channel, bootstrapped to the test-only `rho:registry:v1:internal` URN via the same `for(x <- channel) { x!(channel) }` forwarder pattern Registry.rho uses for `rho:registry:lookup`. Store layout is `{"lib": Map[(deployerId, projectId, version) -> {"code": _, "deprecated": Bool, "notify": List}], "serve": same}` — a plain Rholang Map, not TreeHashMap, deferred to a follow-up if scale demands it.

Contracts:

- `insertVersion(ret, ns, deployerId, projectId, version, code)` — returns `true` on first insert under `(ns, deployerId, projectId, version)`, `false` on duplicate or unknown namespace. The `deployerId`-vs-URN-pubkey check is deferred to Step 5 (where the resolver knows the URN structure).
- `deprecateVersion(ret, ns, deployerId, projectId, version)` — flips `deprecated` to `true`; returns `true` if the entry existed, `false` otherwise. Notify-channel firing is deferred to Step 7.
- `approveVersion(ret, ns, deployerId, projectId, version)` — flips `deprecated` back to `false`; returns `true`/`false` symmetrically.

Rust-side scaffolding to register the test handle:

- `FixedChannels::reg_v1_internal()` returning `byte_name(19)` in `system_processes.rs`.
- An entry in `basic_processes()` mapping `"rho:registry:v1:internal"` to a write-only bundle of that channel.
- An additional `bootstrap(FixedChannels::reg_v1_internal())` call in `registry::registry_bootstrap::ast()` so the bootstrap forwarder for byte_name(19) gets pre-installed alongside the legacy registry channels.

Each of the three additions is marked with `// TODO(Step 6): remove` so the rename to `rho:registry:1.0.0` is obvious in review.

#### Lib runtime check — deferred

Not implemented in this revision. Accept all `"lib"` inserts. The persistent-name check stays an open follow-up (see TODO at the top of `VersionedRegistry.rho`).

#### Testing

`casper/src/test/resources/VersionedRegistryTest.rho` is the probe (NOT a RhoSpec spec — see `regver-known-issues.md` for why) and `casper/tests/genesis/contracts/versioned_registry_spec.rs` is the Rust spec around it. The probe calls each contract, captures the reply, and sends `(test_name, attempt, (expected, "==", actual), clue, ackCh)` straight to `rho:test:assertAck`. The Rust spec calls `get_results` directly, then:

1. Asserts every name in `EXPECTED_TEST_NAMES` shows up in `result.assertions` — guards against vacuous passes.
2. Iterates every recorded assertion and `assert_eq!`s on `expected == actual`, surfacing the clue on failure.

Confirmed working in both directions: deliberately wrong assertions in the probe cause the Rust spec to fail with a clear "left vs right" diagnostic, and removing a test from the probe causes the spec to fail with "recorded no assertions."

Test cases (all 7 currently pass):

- `insertVersion_lib_happy_path` — first insert under `"lib"` returns `true`.
- `insertVersion_serve_happy_path` — same for `"serve"`.
- `insertVersion_duplicate_rejected` — repeating the same `(deployerId, projectId, version)` returns `false`.
- `insertVersion_bad_namespace_rejected` — `"other"` namespace returns `false`.
- `deprecateVersion_sets_flag` — deprecating an existing entry returns `true`.
- `deprecateVersion_unknown_rejected` — deprecating a missing entry returns `false`.
- `approveVersion_clears_flag` — `insert → deprecate → approve` returns `true`.

The `deployerId`-mismatch test and the lib persistent-name check are deferred to Steps 5 and 6+ respectively.

Legacy regression: `cargo test -p casper --test mod registry` runs `registry_spec`, `registry_ops_spec`, and `multi_parent_casper_should_be_able_to_use_the_registry` — all still green unchanged.

What we don't test at Step 3: anything about the resolver picking versions by pattern (no resolver yet) or about lookup ordering (lookup doesn't exist as a method until Step 6).

### Step 4 — URI parsing helper, exposed via a new ops URN ✅ DONE (2026-06-15)

Landed as `rho:registry:ops:1.0.0`, a parallel system process to the legacy `rho:registry:ops`. The legacy handler is untouched.

- New module `rholang/src/rust/interpreter/registry/versioned_urn.rs` with a pure-Rust `parse_urn(&str) -> Option<ParsedUrn>` that splits a `rho:lib:…` / `rho:serve:…` / `rho:registry:…` URN into segments. Preserves wildcards and prerelease tags verbatim; semver semantics stay in `semver.rs`.
- `FixedChannels::reg_ops_v1()` at `byte_name(23)` and `BodyRefs::REG_OPS_V1 = 17` in `system_processes.rs`.
- New `registry_ops_v1` handler method that dispatches on the first arg:
  - `"buildUri"(bytes)` — identical to the legacy path, returns a `rho:id:…` URI.
  - `"parseVersionedUri"(urn)` — returns the 5-tuple `(namespace, service_version, pub_key, project_id, project_version)` where the trailing three are `Nil` for the `rho:registry:` shape. Returns `Nil` on malformed input.
- `Definition` row added in `rho_runtime.rs` next to the legacy `rho:registry:ops` row.

**Verification status:**

- `cargo test -p rholang --lib registry::versioned_urn` — 9 parser unit tests pass (lib/serve/registry happy paths, wildcards preserved, prerelease tags preserved, every rejection class).
- `cargo test -p casper --test mod versioned_registry_spec` — 11 Rholang-level assertions pass, including:
  - `opsV1_buildUri_matches_legacy` — `rho:registry:ops:1.0.0` and `rho:registry:ops` produce the same URI on the same input.
  - `opsV1_parseVersionedUri_lib` — round-trips `rho:lib:1.0.0:abc123:myproj:2.6.3` to its 5-tuple.
  - `opsV1_parseVersionedUri_registry` — `rho:registry:1.0.0` returns `("registry", "1.0.0", Nil, Nil, Nil)`.
  - `opsV1_parseVersionedUri_malformed` — garbage returns `Nil`.
- Verified end-to-end with a deliberately wrong assertion on the parse-lib case: the Rust spec failed with a clear `left vs right` diagnostic showing the tuple shape from the host.
- Legacy regression: `registry_spec`, `registry_ops_spec`, `multi_parent_casper_should_be_able_to_use_the_registry` all green unchanged.

### Step 5 — versioned-URN resolver ✅ DONE (2026-06-15, resolver only)

Step 5 split into two pieces. **The resolver mechanism is done**; the `eval_new` desugaring that lets `new x(`rho:lib:…`)` auto-call it is deferred to a Step 5b in the lead-up to Step 6, where it pairs naturally with the public `rho:registry:1.0.0` entry-point wiring.

**Resolver delivered (this commit):**

- `lookupVersion(@urnStr, ret)` contract added to `VersionedRegistry.rho`. Parses the URN through `rho:registry:ops:1.0.0`'s `parseVersionedUri`, scans the namespace, filters out deprecated versions via a `collectNonDeprecated` recursive helper, picks the highest matching version via `selectBestVersion`, and returns the stored `code` on `ret`. Returns `Nil` on malformed URN, unknown `(pk, project)`, or no matching non-deprecated version. The `"registry"` namespace currently returns `Nil`; Step 6 wires it to the v1 API bundle.
- Store layout refactored from `(deployerId, projectId, version)` flat keys to `(deployerId, projectId) → Map[version → entry]` so the resolver can iterate the version map cheaply. `insertVersion` / `deprecateVersion` / `approveVersion` updated; Step 3 tests still pass unchanged.
- Two new ops on `rho:registry:ops:1.0.0`:
  - `"matchesVersion"((pattern, version))` — semver match via `Pattern::matches`. Returns `false` on malformed input rather than failing.
  - `"selectBestVersion"((pattern, [version, …]))` — picks the highest matching version from a list, via `Pattern::best_match`. Pushes the semver ordering into Rust so the Rholang resolver doesn't need a comparator.

**Test coverage (8 new cases, all real-verified):**

- `resolve_exact_version` — exact lookup returns the stored code.
- `resolve_patch_wildcard` — `1.0.*` across `1.0.0`/`1.0.1`/`1.0.2` returns `1.0.2`'s code.
- `resolve_minor_wildcard` — `1.*` across `1.0.5`/`1.1.9`/`1.2.3` returns `1.2.3`'s code.
- `resolve_major_wildcard` — `*` across `1.5.0`/`2.0.0` returns `2.0.0`'s code.
- `resolve_prerelease_skipped` — `1.*` across `1.0.0` and `1.1.0-alpha` returns the stable `1.0.0`'s code.
- `resolve_deprecated_skipped` — deprecating `1.0.1` makes `1.0.*` fall back to `1.0.0`'s code.
- `resolve_miss_returns_nil` — unknown `(pk, project)` returns `Nil`.
- `resolve_malformed_returns_nil` — garbage URN returns `Nil`.

Verified end-to-end by flipping the expected value on `resolve_patch_wildcard` from `"patch2"` to `"patch0"`: the Rust spec failed with `left: "patch0"`, `right: "patch2"`.

**Deferred to a follow-up branch:** the `eval_new` / normalizer rewrite that makes `new x(`URN`) in P` desugar automatically to the for-receive pattern.

After Commits A (`735760aa`) and B (`de54c618`) on this branch, the unified dispatcher exists as the `rho:internal:registry_lookup` URN and works for both legacy and versioned URNs. Users who want the unified semantics today can write:

```rho
new rl(`rho:internal:registry_lookup`), got in {
  rl!?("rho:lib:1.0.0:abc:proj:2.6.3", *got) |
  for (foo <- got) { /* foo is the resolved code; works for both for-receive and send patterns */ }
}
```

The remaining sugar — getting `new foo(`URN`) in P` to compile to this shape automatically — needs either:

- A synchronous rspace round-trip inside `eval_new` that calls `registry_lookup`. Requires wiring `eval_new` into the dispatcher and handling replay-mode / cost-accounting subtleties.
- An AST-level rewrite in `p_new_normalizer.rs` that transforms URN bindings into `new tmpRet in { rl!(URN, *tmpRet) | for(foo <- tmpRet) { P } }` before normalization. Requires constructing Rholang AST nodes via the parser and verifying DeBruijn index handling end-to-end for multi-binding `new`s.

Both are doable but want focused design + multi-commit work. Documented as the natural next branch.

Plan text retained below for reference.

---

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

### Step 6 — the `rho:registry:1.0.0` entry point ✅ DONE (2026-06-15)

Wired up the public versioned-registry entry point. Clients now import the v1 API surface via the canonical FIP shape:

```
new getReg(`rho:registry:1.0.0`), notify in {
  for (reg <- getReg!?(*notify)) {
    reg!("insertVersion", "lib", deployerId, projectId, version, code, *ackCh) |
    ...
  }
}
```

**Delivered:**

- `FixedChannels::reg_v1()` at `byte_name(24)`.
- `basic_processes()` entry mapping `"rho:registry:1.0.0"` to a write-only bundle of that channel.
- Bootstrap forwarder added to `registry_bootstrap::ast()` so the byte_name(24) channel has the one-time `for(x <- ch){x!(ch)}` pre-installed.
- VersionedRegistry.rho declares `bootstrapPublicV1(`rho:registry:1.0.0`)` + `publicV1Ch`, runs the bootstrap inside the inner `for (v1Api <- v1ApiCh) { ... }` block so the handler has access to the resolved `v1Api` channel, and installs the persistent listener:

  ```
  contract publicV1(ret, _notify) = {
    ret!(bundle+{*v1Api})
  }
  ```

  The `_notify` channel is captured here for Step 7's deprecation broadcast; currently unused.

- The test-only `rho:registry:v1:internal` URN stays in place (cleanup deferred). Its TODOs are repointed to reference a cleanup commit instead of Step 6 directly.

**Test coverage (2 new cases, both real-verified):**

- `public_v1_returns_v1Api_bundle` — `getReg!(*reg, *notify)` then `reg!("insertVersion", ...)` returns `true`.
- `public_v1_insert_then_lookup` — insert via the public URN's bundle then `lookupVersion` via the same bundle; the inserted code round-trips.

Verified end-to-end by flipping the expected `"publicCode"` value on the round-trip case: the Rust spec failed with `left: "WRONG"`, `right: "publicCode"`, proving the full chain (public URN → v1Api bundle → insertVersion → lookupVersion → response) executes.

**Step 5b status:** the `eval_new` desugaring for parametric `rho:lib:…` / `rho:serve:…` URNs is still deferred. With the public registry URN now usable through the FIP-canonical shape, the deferral cost is just that clients of `lib`/`serve` packages have to call `reg!("lookupVersion", urn, *ret)` explicitly instead of writing `new getLib(`rho:lib:…`)`. Not blocking for Step 7 (notify wiring).

---

Original Step 6 plan text:

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

### Step 7 — deprecation notify wiring ✅ DONE (2026-06-15)

Deprecation notifications are end-to-end: clients pass a notify channel at lookup time, deprecation fires `("deprecated", deployerId, projectId, version)` on every recorded channel.

**Delivered:**

- New recursive helper `fireNotifications(@notifyList, @msg, done)` in `VersionedRegistry.rho` — walks a list of stored notify channels and emits `msg` on each.
- `lookupVersion` signature changed from `(urn, ret)` to `(urn, notifyCh, ret)`. When `notifyCh` is `Nil` the entry is unchanged; otherwise the channel is appended to the resolved entry's `notify` list.
- `deprecateVersion` flips `deprecated: true` and then calls `fireNotifications` on the entry's `notify` list before returning `true`. The list stays in the entry after firing, so repeated `deprecateVersion` calls re-fire (idempotent at the contract level; clients dedupe).
- Notify channels are stored per-resolution. Two `lookupVersion` calls produce two independent entries, both fire on a single `deprecateVersion`.

**Scope notes:**

- All existing Step 5 / Step 6 test cases were updated to pass `Nil` for the new notify slot (no behavior change).
- The public URN's `_notify` (from `getReg!?(*notify)`) is still captured but unused; there is no registry-level deprecation list yet. The FIP example imports a versioned library, which already works through the per-`lookupVersion` notify slot. Registry-level notification (deprecating `rho:registry:1.0.0` in favor of `rho:registry:2.0.0`) is a follow-up when `rho:registry:2.0.0` actually ships.
- Channel layout: notify channels are stored as the dereferenced process. Callers pass `*myNotify` and the contract appends that process to the list. `fireNotifications` quotes each list element with `@` to send.

**Test coverage (3 new cases, all real-verified):**

- `notify_fires_to_one_listener` — single lookup with notify, deprecate, expect one `("deprecated", deployerId, projectId, version)` on the notify channel.
- `notify_fires_to_many_listeners` — three independent `lookupVersion` calls register three notify channels; a single `deprecateVersion` fires all three with identical payloads.
- `approve_then_deprecate_refires` — insert, lookup with notify, deprecate (fire 1), approve, deprecate again (fire 2). Verifies the per-resolution list survives the approve→deprecate cycle and re-fires.

Verified by flipping the expected tag in `notify_fires_to_one_listener` from `"deprecated"` to `"WRONG_TAG"` — the Rust spec failed with `left: "(\"WRONG_TAG\", \"alice\", \"p22\", \"1.0.0\")"`, `right: "(\"deprecated\", \"alice\", \"p22\", \"1.0.0\")"`, proving the full chain (lookup → notify recording → deprecate → fire → receive) executes.

Total tests in `versioned_registry_spec`: 24, all passing. Legacy regression suite unchanged.

---

Original Step 7 plan text:

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
