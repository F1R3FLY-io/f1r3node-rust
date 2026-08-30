# Cost-accounted Rholang

F1R3node meters Rholang with two related but distinct cost dimensions:

1. **Compute-based authority accounting** charges successful atomic RSpace
   communications and preserves the linear ownership semantics of signed and
   located processes.
2. **Storage-based quantitative accounting** charges the canonical bytes
   introduced to RSpace, delivered by a communication, and committed in its
   replay trace.

Both dimensions draw existing REV custody from the same authenticated
`SystemVault`, also called the **RevVault** in the cost-accounting papers. They
share one certificate, one execution witness, one failure-atomic settlement,
and one replay-validity decision. There is no client-selected
`phlo_limit × phlo_price` gas market, second fuel currency, or runtime A/B
switch.

The design refines the five gated communication rules and join-cost schema in
[*Cost-Accounted Rho Calculus*](../../../publications/cost-accounting/cost-accounted-rho.tex)
and the cost endofunctor over continued interactive generalized structured
labelled transition systems (GSLTs) in
[*Continued Interactive GSLTs and the Cost Endofunctor*](../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex).
The byte tariff is a native safety refinement: the papers require finite,
authority-backed work but deliberately do not prescribe a protobuf byte
schedule.

![End-to-end byte and authority lifecycle: a top-up changes only unreserved custody, admission freezes authority, byte, and fee allocations, RSpace charges before mutation, settlement burns exact cost and transfers the fee, and replay recomputes the same evidence.](../casper/theory/diagrams/vault-backed-byte-accounting.svg)

## Terms, units, and ledgers

| Term | Meaning |
| --- | --- |
| `COMM` | One successful atomic RSpace synchronization between a waiting continuation and all required data |
| Authority signature | Canonical identity of a wallet, located purse, or compound linear authority permitted to pay |
| Supply $`\Sigma(a)`$ | REV custody and authenticated prepaid stack cells available to authority lane $`a`$ in the certified pre-state |
| Compute bound $`B_A(a)`$ | Maximum physical authority allocated to successful communication events in lane $`a`$ |
| Byte bound $`B_Q(a)`$ | Maximum quantitative byte debit allocated to lane $`a`$ |
| Fee allocation $`F(a)`$ | Deterministic transfer from payer custody to the proposer, separate from burned execution cost |
| Realized authority $`\kappa(a)`$ | Physical authority consumed by successful committed communication events |
| Realized byte cost $`Q(a)`$ | Canonical introduction, delivery, and trace bytes charged to lane $`a`$ |
| Certificate | Domain-separated commitment to the protocol, program, pre-state, bounds, allocations, fee, and reservation identity |
| Witness | Exact causal events, physical draws, byte events, settlements, and adjacent state roots produced by execution |

Let $`C(E)`$ be the number of committed `COMM` events in execution $`E`$.
Let $`Q(E)`$ be its quantitative byte cost. `ProcessedDeploy.cost` reports:

```math
K(E)=C(E)+Q(E).
```

The reported scalar is not the physical REV debit. A single `COMM` can require
several linear authority components, while one compound component can select a
particular physical funding option. If $`A(E)`$ is the exact physical authority
draw and $`F(E)`$ the fee, the native custody change is:

```math
\operatorname{VaultDebit}(E)=A(E)+Q(E)+F(E).
```

Keeping $`C`$, $`A`$, and $`Q`$ distinct is load-bearing. Collapsing them would
either weaken compound authority, lose the one-interaction grade of the papers,
or leave storage work outside the hard REV ceiling.

## Compute-based authority accounting

A send that waits and a receive that waits perform no successful communication,
so each contributes zero to $`C(E)`$. When a complete match fires, the runtime
records one causal `COMM` and charges it once. An $`N`$-channel join is one
atomic communication, not $`N`$ partially committed events.

Authority is nevertheless component-wise. A whole redex inside one signed
region can draw one authority cell. Separately located or compound participants
can require several cells for the same `COMM`. Tensor requires all components;
additive choice reserves the point-wise maximum of alternatives; lollipop
transfers authority from its source obligation to its continuation; and a
located term draws from its named purse instead of falling back to the deploy
signer.

The physical allocator searches canonical wallet balances and located stacks,
records the exact draw per event, and re-verifies that presentation before it
can enter block evidence. Candidate-created authority cannot fund the same
admission: only authenticated pre-state custody and causally available prepaid
stacks count.

## Storage-based quantitative byte accounting

Storage-based accounting covers three finite resource surfaces:

- a stable `produce` or `consume` **introduction** before any RSpace counter,
  store, lookup, or trace mutation;
- every payload delivered by a successful `COMM`; and
- the canonical consume/produce trace committed for replay.

For schedule $`S=(r_i,r_d,r_t)`$, the charge for event $`e`$ is:

```math
Q_S(e)=r_iI(e)+r_dD(e)+r_tT(e),
```

where $`I`$ is introduction footprint, $`D`$ is delivered payload, and $`T`$
is committed trace footprint. Schedule version 1 uses unit rates. The schedule
version and Blake2b-256 digest are consensus evidence, so a validator cannot
silently use different rates or encodings.

Both sides are charged as introductions regardless of arrival order. Charging
only the side retained in RSpace would make cost depend on Tokio scheduling.
Persistent operations reuse one stable introduction identity across internal
fixed-point retries, while distinct non-persistent operations preserve their
multiplicity. Peek restoration, persistence, and removal never create negative
byte credits.

Every addition, multiplication, and integer conversion is checked. Overflow or
insufficient capacity rejects before the affected mutation. See
[Vault-backed quantitative byte accounting](../casper/theory/cost-accounting-impl/vault-backed-byte-accounting.md)
for the exact canonical footprints and concurrency protocol.

## Unified admission and settlement

For every authority lane $`a`$, admission requires:

```math
B_A(a)+B_Q(a)+F(a)\leq\Sigma(a).
```

The certificate freezes all three allocations against one authenticated
pre-state. A later wallet top-up can fund a later execution, but it cannot
change an in-flight bound, rescue an exhausted execution, or alter the
certificate identifier.

The production algorithm is:

```text
verify and canonically order the complete cosigned envelope
normalize with the authenticated deployer and cosigner environment
derive candidate authority from the merged pre-state
execute in scratch state under finite authority and byte capacity
discard exhausted candidates and repeat until the retained set is stable
derive exact physical draws, byte settlement, fee, and adjacent roots
apply all vault debits, stack pops, fee credit, and state changes atomically
publish the certificate and witness only for the committed result
```

`SystemVault.applyCost` realizes the paper's reserve, exact debit, fee transfer,
and refund phases inside one lexical transaction. Unused maximum allocation
never becomes a durable debit. Unused located stack cells remain in RSpace.
Failure restores the enclosing node checkpoint, so user state, stack pops,
vault changes, and evidence commit together or not at all.

After successful settlement:

```math
\Sigma'(a)=\Sigma(a)-\kappa(a)-Q(a)-F(a).
```

Transfers and fee credits conserve ownership. Only authenticated genesis or
proof-of-stake system execution may call `SystemVault.protocolMint` to increase
total REV supply.

## Atomic RSpace ordering

The observer order is part of consensus:

```text
before a produce mutation: reserve its canonical introduction bytes
before a consume mutation: reserve its canonical introduction bytes
after selecting a complete match but before mutation:
    reserve one COMM unit
    reserve every delivered payload and trace byte
    record physical and quantitative event identities
commit the RSpace match and resume the continuation
```

If any reservation fails, the triggering operation is not recorded and the
affected datum or continuation is not removed. A located-stack production uses
a pending physical reservation that becomes visible only after its complete
byte-charged RSpace operation succeeds; cancellation or error restores exactly
those pending cells.

## Replay and consensus

The exact RSpace log is a causal replay witness, not proposer telemetry.
Independent validators:

1. reconstruct and verify the complete cosigned envelope;
2. normalize with the same authenticated environment;
3. verify the authority protocol and byte-schedule identities;
4. replay the causal events from the certified pre-state;
5. recompute compute cost, byte cost, physical draws, fee, and settlement; and
6. require exact post-state-root equality.

A cost, schedule, event, allocation, status, or root mismatch makes the block
invalid. Missing local history and temporarily unavailable state are local
validation failures and must not be converted into slash evidence.

Multi-parent merge operates on durable effects, not proof-local reservations.
Accepted effects retain exact source-block and execution-index provenance.
Finality and fork choice may advance only through state lineage that preserves
already certified effects. Mergeable evidence is replayed locally and keyed by
complete execution identity; unauthenticated peer-provided merge data cannot
authorize a state transition.

## Examples

An unmatched output has no compute charge but does have an introduction-byte
charge:

```rholang
@"requests"!(42)
```

For this execution, $`C(E)=0`$ and $`Q(E)>0`$.

A matched pair contributes two introductions, one delivered payload and trace,
and one compute event:

```rholang
@"requests"!(42) |
for (value <- @"requests") { Nil }
```

Reversing arrival order leaves both $`C(E)`$ and $`Q(E)`$ unchanged.

A two-channel join is still one compute event, but its byte charge includes all
introductions, both delivered payloads, and the complete join trace:

```rholang
@"left"!(1) |
@"right"!(2) |
for (x <- @"left"; y <- @"right") { Nil }
```

No input is partially consumed when either the authority proof or byte
reservation is insufficient.

## Failure semantics

- Parser or signature failure occurs before user execution and publishes no
  cost certificate or witness.
- Insufficient authenticated supply rejects admission without committed user
  state or vault debit.
- Introduction, payload, trace, or arithmetic failure rejects before the
  affected RSpace mutation.
- Runtime exhaustion cannot certify itself and cannot commit its speculative
  root.
- Replay mismatch is objective block invalidity.
- Local storage unavailability is retried or recovered and never becomes
  consensus evidence against a peer.

## Implementation and verification map

| Obligation | Implementation | Verification |
| --- | --- | --- |
| One compute event per successful atomic match | `rspace_interface::CommObserver`; locked RSpace match path | `AtomicCommAccounting.v`; `AtomicCommAccounting.tla`; trigger-order and join tests |
| Canonical introduction, delivery, and trace bytes | `accounting/byte_accounting.rs`; proposal and replay RSpace observers | `VaultBackedByteAccounting.v`; `VaultBackedByteAccounting.tla`; RSpace unit/property tests |
| One hard REV ceiling across compute, bytes, and fee | `FundingCertificate`; `RuntimeBudget`; `SystemVault.applyCost` | `EndToEndAuthority.v`; `AtomicVaultSettlementRefinement.v`; admission and settlement tests |
| Located purse and lollipop isolation | authority regions, funding slots, stack reservations, canonical physical allocator | `WalletFundedLollipop.v`; `WalletFundedLollipop.tla`; cross-deploy wallet-funded tests |
| Failure-atomic stack introduction | pending stack reservation plus node checkpoint | `StackIntroductionAtomicity.v`; TLA+ unsafe controls; Loom and rollback tests |
| Independent proposal/replay equality | certificate and witness validation in Casper runtime and replay runtime | `StateBoundValidatorConvergence.tla`; replay and independent-runtime regressions |
| State-preserving finality and merge | exact state-effect provenance, finalized floor, deterministic merge algebra | finalized-floor, deploy-recovery, and merge-algebra TLA+/Rocq suites plus Casper integration tests |

Read [Wallet-funded process lifecycle](20-wallet-funded-processes.md) for the
end-to-end user and operator workflow, including wallet refill and cryptographic
ownership. Read
[End-to-end cost authority and native RevVault settlement](../casper/theory/cost-accounting-impl/end-to-end-authority-settlement.md)
for the implementation contract and
[Formal Verification of Cost-Accounted Rho](../casper/theory/cost-accounted-rho-verification.md)
for the proof catalog.

## References

1. J.-Y. Girard, “Linear Logic,” *Theoretical Computer Science* 50 (1987),
   1–101. [doi:10.1016/0304-3975(87)90045-4](https://doi.org/10.1016/0304-3975(87)90045-4).
2. L. G. Meredith and M. Radestock, “A Reflective Higher-order Calculus,”
   *Electronic Notes in Theoretical Computer Science* 141(5) (2005), 49–67.
   [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
