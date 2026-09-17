# Language-provider ownership at prepared admission

The language provider is the host-owned collection of installation authority,
runtime language services and foreign-language-term (FLT) matching state.
Its lifecycle is separate from an RSpace checkpoint. Restoring the checkpoint
does not restore the provider or refund computation.

This document specifies the composition boundary before node activation. The
existing MeTTaIL service bundle already shares one language runtime; the node
still needs to inject that bundle and its matching component into its actual
setup. The accompanying
[Rocq model](../../formal/rocq/cost_accounted_rho/theories/LanguageProviderLifecycle.v)
does not claim that this node wiring is implemented or that the public demo runs.

## Owners and observations

An **owner** means the identity and lifetime of a host object, not its public
name or fingerprint. A Rust `Arc` is a shared owning reference: cloning it
retains the same object. A **pinned snapshot** is one immutable registry view
used for all lookups in an installation. A **registration** is retained matcher
data derived from a pattern, not an installed-language capability.

| Component | Current owner and source | Composition requirement |
|---|---|---|
| Registry snapshot, installed table, policy, revocation authorities | MeTTaIL `LanguageInstallService`, `language_install.rs` | Keep the injected snapshot and frozen policy; never substitute ambient registry or filesystem access. |
| Capability generations, entries, prepared-pattern handles and admission cache | MeTTaIL `RholangLanguageRuntime` | All services and the matcher use the same runtime object. |
| Parser and semantic callbacks | Runtime's `Arc<dyn RuntimeHost>` | Retain the injected host; grammar data cannot replace host authority. |
| Theorem-space service | `RholangTheoremService` | Its four handlers share one theorem service, which retains the same language runtime. |
| Pattern automaton and refusal ledger | `SubstrateGuardMatcher` and `FltAutomatonMatcher` | Keep the driver's clone of the matcher installed in RSpace; inspect its refusal ledger after execution. |
| Evaluation storage | Node setup's store manager and the handles it supplies | Keep owning resources alive through runtime use; a filesystem path alone is not an owner. |

The model's `shared_endpoints` covers the existing ten service ports—install,
parse, construct, pattern, theorem open/prepare/commit/revoke, reduce and
observe—and the matcher. The complete, duplicate-free roster is proved.
`endpoint_and_matcher_share_owner` and `endpoint_configuration_exact` prove
the constant-owner construction, including snapshot, policy, callbacks and
resource owner. These are construction laws; they do not establish pointer
identity for an arbitrary list of independently created Rust handlers.

## Two different state boundaries

Write the execution state as a pair: host state and provider state. The host
projection includes RSpace, budget and merge information. The provider
projection includes actual mutable authority and registrations, not merely
harmless caches. For the state *after* execution, the composition is:

```text
finish(checkpoint, after, result):
    host := existing_prepared_admission_wrapper(checkpoint, after.host, result)
    return (host, after.provider)
```

`finish_provider_wrapper` instantiates the existing
[prepared-admission model](f1r3lang-prepared-admission.md), not a new evaluator.
Its proved projections are:

| Outcome | RSpace | Budget and merge state | Provider |
|---|---|---|---|
| Successful evaluation, empty error list | Post-execution state | Post-execution state | Post-execution state |
| Nonempty evaluation error list | Checkpoint | Post-execution state | Post-execution state |
| Escaped interpreter error | Checkpoint | Post-execution state | Post-execution state |

These laws retain the exact existing empty-error-list behavior. In particular,
the old empty aggregate-error case does not imply rollback. The proof includes
a concrete counterexample to claiming that a space restore removes newly
installed provider entries.

The current node `RhoRuntime::evaluate_with_env_and_phlo` wraps evaluation in
this checkpoint behavior. `evaluate_prepared` and `evaluate_with_frontend`
themselves do **not** create or restore a checkpoint. Their public composition
must explicitly retain the intended wrapper. The in-memory MeTTaIL helper
`inj_on_runtime_unchecked` is not that public integration: it uses raw injection
and maximum funding and must not be copied as the node execution path.

## Registration, refusal and authority

`FltAutomatonMatcher::prepare` traverses and converts candidate patterns before
mutating retained state. A returned traversal error leaves that state unchanged.
Successful preparation registers patterns before injection. If subsequent
evaluation fails, those registrations are outside the RSpace checkpoint.
`refused_registration_preserves_provider`, `registration_preserves_host` and
`registration_does_not_grant_authority` model this exact returned-result order.
Panics and allocation aborts are not represented as ordinary returned errors.

Prepared dynamic patterns are a distinct capability-directory operation:
`SubstrateGuardMatcher::get` resolves their token through the shared runtime,
checks capture counts and subject admission, then projects checked captures.
Do not equate an automaton cache entry with authority to match a dynamic term.

Installation has its own atomic boundary. Existing
`InstalledLanguageAuthority` laws prove that rejected parser/semantic artifacts
publish no batch prefix and an admitted executable bundle publishes exactly
once. `RholangLanguageRuntime::install_all` acquires its capability-directory
write lock before the service commit, avoiding publication through an already
poisoned directory. Those laws are **per installation batch**, not proof of
whole-evaluation rollback.

Consequently node integration must not advertise transactional provider rollback
on the strength of an RSpace checkpoint. Reauthorization after revocation and
guarded publication remain required at their existing service boundaries.
If a public route requires installation state to roll back with its evaluation,
that route needs an explicit provider transaction and its correspondence proof;
neither the current source nor this model supplies one. This is an integration
obligation, not permission to silently weaken route semantics.

## Exact source correspondence and handoff

Inspected node baseline: `70186668fbcc47df9bddca2a91005a2d19f455f5` on
`feature/f1r3lang-mettail-only`. Inspected MeTTaIL source belongs to
`feature/rholang-mettail-modules`; the relevant service and matcher files were
read at `fa7a5e4540f1829c9185a283c0cb62bbb9fbf2db`.

| Law or boundary | Source inspected | Status |
|---|---|---|
| Frozen policy and injected snapshot | MeTTaIL `language_install.rs`: `LanguageInstallService::new` | Existing constructor refreshes the policy fingerprint and retains snapshot ownership. Snapshot immutability is the host implementation's obligation, not proved from `Arc`. |
| Same runtime in service ports | `language_runtime_definitions_with_theorem_checker`, theorem and semantic definition constructors | Existing constructors clone the same runtime; the existing ten-port unit test checks distinct channels/body references. That test alone does not prove pointer identity. |
| Same runtime in matcher | `SubstrateGuardMatcher::with_language_runtime`; MeTTaIL `build_runtime_with_language_runtime` | Existing explicit path supports it. Supplying only similarly named definitions does not establish common ownership. |
| Registration refusal | `FltAutomatonMatcher::prepare` | Fallible traversal precedes lock acquisition and registration; no returned failure follows mutation. |
| Checkpoint projection | Node `rho_runtime.rs`: `evaluate_with_env_and_phlo`, `revert_to_soft_checkpoint` | Existing source restores space only; the new proof imports the exact prepared-wrapper laws. |
| Actual node wiring | Node `node/src/rust/runtime/setup.rs`, eval-runtime construction | Still passes empty additional definitions and the plain matcher. Activation remains separate work. |
| Resource lifetime | Node setup retains its store manager; `interpreter/test_utils/resources.rs::with_runtime` retains its temporary directory across the awaited callback | No additional directory framework is required by this inspection. |

The next composition implementation must construct one explicit provider owner,
derive all services and the matcher from it, validate reserved-channel/body-ID
collisions before registration, retain the driver's shared matcher handle, and
use the prepared signed/metered execution path with explicit error cleanup.
The owner laws do not prove those collision checks, callback behavior, node
registration, consensus/replay, parser correctness or the complete FLT `where`
judgment. They must not be cited as if those implementations were complete.

## Reproduction and proof scope

The focused proof imports only `PreparedProgramAdmission` and standard-library
modules. No arbitrary language parser or regex evaluator is introduced.
Compile into a `target` directory with `CostAccountedRho` as its logical package,
then run the separate kernel checker:

```sh
mkdir -p target/verification/provider-lifecycle
systemd-run --user --scope --quiet \
  -p MemoryMax=1G -p MemoryHigh=900M -p MemorySwapMax=0 -p TasksMax=8 \
  coqc -q -Q target/verification/provider-lifecycle CostAccountedRho \
  -o target/verification/provider-lifecycle/PreparedProgramAdmission.vo \
  formal/rocq/cost_accounted_rho/theories/PreparedProgramAdmission.v \
  > target/verification/provider-lifecycle/prepared-compile.log 2>&1
systemd-run --user --scope --quiet \
  -p MemoryMax=1G -p MemoryHigh=900M -p MemorySwapMax=0 -p TasksMax=8 \
  coqc -q -Q target/verification/provider-lifecycle CostAccountedRho \
  -o target/verification/provider-lifecycle/LanguageProviderLifecycle.vo \
  formal/rocq/cost_accounted_rho/theories/LanguageProviderLifecycle.v \
  > target/verification/provider-lifecycle/provider-compile.log 2>&1
systemd-run --user --scope --quiet \
  -p MemoryMax=1G -p MemoryHigh=900M -p MemorySwapMax=0 -p TasksMax=8 \
  coqchk -silent -Q target/verification/provider-lifecycle CostAccountedRho \
  CostAccountedRho.LanguageProviderLifecycle \
  > target/verification/provider-lifecycle/provider-kernel.log 2>&1
```

Every theorem/example prints its assumptions. All must be closed under the
global context; no admitted theorem or axiom supplies a lifecycle fact.
The finite owner/registration model uses constructor cases, and checkpoint
composition reuses already-proved wrapper laws. This is mathematical model
verification plus a source-correspondence audit, not mechanically verified Rust.

Focused verification passed: the reused prepared-admission dependency compiled,
all 17 provider laws/examples reported closed contexts, and a separate kernel
check of the final provider module and its dependency closure exited successfully.
The retained final logs are `provider-compile-2.log` and `provider-kernel-2.log`
in `target/verification/provider-lifecycle`. Compilation took 0.56 seconds with
401,312 KiB peak resident memory; the kernel check took 12.82 seconds with
277,292 KiB. Both ran serially with the limits above and swap disabled. These
are proof-validation measurements, not node runtime performance claims.
