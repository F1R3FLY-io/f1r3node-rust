# Vault-backed quantitative byte accounting

## Status and scope

This document is the consensus contract for quantitative RSpace byte accounting
within the native cost-accounting refinement. It covers storage introductions,
payload delivery, committed trace storage, RevVault reservation and settlement,
authorized purse top-ups, proposal/replay agreement, and failure atomicity.

The governing semantic sources are
[*Cost-Accounted Rho Calculus*](../../../../publications/cost-accounting/cost-accounted-rho.tex)
and [*Continued Interactive GSLTs and the Cost Endofunctor*](../../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex). The first
paper makes token consumption the gate for a communication and treats a token
stack as a phlogiston balance. The second makes metering lazy: work is charged
when performed, not merely when syntax is exposed, and located purses decompose
the global resource proof into local proofs. Neither paper assigns a concrete
protobuf-byte tariff. The tariff in this document is therefore a native
F1R3node safety refinement: it bounds the finite storage, transfer, and trace
work required to realize the papers' authorized reductions without changing
their linear authority semantics.

Complete MeTTaIL/GSLT integration remains outside this scope. The native Rust
implementation satisfies the current GSLT-facing cost traits and does not add a
second accounting mode.

![Authorized credits increase only unreserved SystemVault custody; admission freezes a physical-authority, byte-bound, and fee reservation; RSpace charges canonical introductions and COMMs before mutation; settlement burns exact physical and byte cost, transfers the fee, refunds unused custody, and independent replay recomputes every quantity.](../diagrams/vault-backed-byte-accounting.svg)

The cross-subsystem rollback rules that keep this accounting witness aligned
with RSpace and the active replay root are specified in
[evaluation transaction isolation](evaluation-transaction-isolation.md).

## Vocabulary and value domains

The **RevVault** is the existing `SystemVault` registered at
`rho:vault:system`. A wallet is reusable ownership and authentication material;
its SystemVault purse is persistent custody. An authorized transfer may refill
that purse at any time. A refill is a conserving ownership transfer, not a
protocol mint and not a new wallet.

An **introduction** is one stable semantic `produce` or `consume` identity. It
is charged even if it immediately matches. A persistent process may invoke the
same RSpace operation repeatedly while finding all matches and then reaching
its stored fixed point; those internal retries retain one identity and one
introduction charge. Distinct non-persistent operations retain multiplicity.
This term deliberately differs from a retained datum or continuation: whether
an introduction waits in RSpace is an arrival-order fact and cannot determine
consensus cost.

The **introduction sponsor** is the located purse responsible for the bytes of
that introduction. The **stored interaction authority** is the optional linear
authority carried by a datum or continuation for a later COMM. These are
different roles. A funding-stack datum is interaction-authority-neutral, so
storing or restoring it must not attach the deploy payer as authority that a
later process could spend. Its introduction nevertheless consumes resources:
the runtime pins the active deploy payer as the sponsor for that event identity.
Once pinned, concurrent retries may repeat the same payer idempotently, but a
different payer cannot overwrite it. A deploy reset clears the registry before
the next execution installs its own sponsor identities.

Fallback resolution is one linearizable registry operation. It does not read
an empty slot, release the registry, and later return an uncommitted fallback.
Instead, it atomically installs the fallback only if the slot is still empty
and returns the payer that actually occupies the slot. Consequently an
explicit registration that wins the race is returned by the resolver, while a
fallback that wins the race permanently excludes a conflicting late payer.

An **unmetered execution** is the trusted genesis or system-construction path
that runs before a user funding certificate exists. It emits neither authority
nor byte evidence. This is a lifecycle boundary, not a runtime feature flag:
ordinary user execution under protocol 4 is always metered.

A **COMM** is one committed atomic RSpace match. It consumes one consensus fuel
unit. Its physical authority proof can draw more than one located cell when the
matched regions carry compound or separately located authority. Consequently,
the following quantities are distinct:

- `ProcessedDeploy.cost` is the number of committed COMM units plus the
  quantitative byte debit;
- the authority witness's physical settlement is the component-wise set of
  RevVault balances and located stack cells that authorize those COMMs;
- the RevVault burn is the physical authority draw plus the quantitative byte
  settlement; and
- the proposer fee is a separate conserving transfer.

Collapsing the physical settlement to the COMM count would weaken compound
authority. Replacing the COMM count with the sum of physical cells would change
the papers' one-interaction cost grade. The implementation records and verifies
both.

## Canonical tariff

For a byte-cost schedule $`S=(r_i,r_d,r_t)`$, let:

- $`I(e)`$ be the canonical encoded footprint of a produce or consume
  introduction;
- $`D(e)`$ be the sum of all payload bytes delivered by a committed COMM; and
- $`T(e)`$ be the committed consume/produce trace footprint of that COMM.

The quantitative charge for an event is:

```math
Q_S(e)=r_i I(e)+r_d D(e)+r_t T(e).
```

Schedule version 1 uses unit rates:

```math
S_1=(1,1,1).
```

The concrete footprints are:

```math
I(\operatorname{produce}(c,d))=
  |\operatorname{prost}(c)|+|\operatorname{prost}(d)|+2h,
```

```math
I(\operatorname{consume}(\vec c,\vec p,k))=
  \sum_i|\operatorname{prost}(c_i)|+
  \sum_i|\operatorname{prost}(p_i)|+
  |\operatorname{prost}(k)|+h+|\vec c|h,
```

```math
D(\operatorname{COMM})=\sum_i|\operatorname{prost}(d_i)|,
```

```math
T(\operatorname{COMM})=(h+nh)+n(h+h),
```

where $`h=32`$ is the Blake2b-256 hash width and $`n`$ is the join arity.
All additions, multiplications, platform conversions, and rate applications are
checked. Overflow rejects the event before mutation.

Let $`C(E)`$ be the number of committed COMMs in execution trace $`E`$. The
consensus execution cost carried by `ProcessedDeploy` is:

```math
K(E)=C(E)+\sum_{e\in E}Q_{S_1}(e).
```

Let $`A(E)`$ be the physical authority multiset allocated to the committed COMM
events, $`B(E)`$ its scalar sum after allocation, and $`F`$ the proposer fee.
The native custody debit is:

```math
\operatorname{VaultDebit}(E)=B(E)+\sum_{e\in E}Q_{S_1}(e)+F.
```

`K(E)` and `VaultDebit(E)` need not be numerically equal when one COMM is
authorized by multiple regions. Replay checks both equations against their own
wire evidence.

## Arrival-order independence

Both sides of a communication are introductions, regardless of which one waits.
The observer therefore executes at the following boundaries:

1. before any `produce` counter, store, or trace mutation, atomically charge a
   previously unseen persistent identity or charge the non-persistent produce
   occurrence;
2. before any `consume` lookup, store, or trace mutation, apply the same rule to
   the consume identity; and
3. after a complete match is selected but before its store or trace mutation,
   charge the COMM's delivered payload and trace footprint together with its one
   fuel unit and physical authority event.

For producer-first and consumer-first execution of the same match:

```math
I(P)+I(C)+Q(\operatorname{COMM}_P)
=I(C)+I(P)+Q(\operatorname{COMM}_C).
```

The COMM identity canonicalizes its producer set and excludes mutable telemetry.
The full equality also requires both introductions; proving only COMM identity
equality is insufficient. Charging only the side retained in RSpace would yield
either $`I(P)`$ or $`I(C)`$, which differ in general and make validator cost
depend on Tokio scheduling.

For a persistent produce or consume, its stable original identity is charged
once even if proposal and replay need different numbers of internal retries to
reach the same fixed point. The identity is recorded only after its reservation
succeeds, so an out-of-budget attempt cannot mark an unpaid introduction as
paid. Each later counterpart is a new introduction and is charged once; each
committed delivery is a new COMM and is charged once. Persistence, peeks,
removal, and settlement never issue negative byte credits. This preserves the
legacy safety purpose without its schedule-sensitive live refunds.

Peek restoration preserves the removed datum's stored interaction authority.
If that datum was authority-neutral, it remains neutral; the restoration is a
new physical introduction sponsored by the active deploy. This charges work
without granting authority and prevents a stored payer field from redirecting
the current deploy's byte debit.

## Located lollipop funding lifecycle

The lollipop operator uses two distinct located purses. The **outer purse** pays
for the rendezvous that crosses the outer authority layer. The **continuation
purse**, also called the funding-slot purse, pays for work performed after the
gateway receives the retained continuation capability. Their public addresses
allow deposits but confer no right to draw.

The complete lifecycle is:

```text
install:
    derive distinct outer and continuation purse addresses
    publish both deposit addresses
    retain the unforgeable continuation capability

fund:
    authenticate the sponsor wallet
    atomically debit the sponsor purse
    credit both located purses without minting REV

activate:
    require committed funding of both purses
    authenticate the gateway
    require the retained outer authority and continuation capability

admit and settle:
    certify each purse against its own bound in the authenticated pre-state
    debit each purse by its own realized cost
    transfer the gateway fee separately
    refund each purse's unused bound
    replay the same payer, bound, debit, fee, and post-state root
```

For outer purse $`o`$ and continuation purse $`s`$, admission is component-wise:

```math
R^A_o+R^Q_o\leq L_o
\qquad\text{and}\qquad
R^A_s+R^Q_s\leq L_s.
```

Surplus in one purse cannot rescue a deficit in the other. A stack or
capability created by the candidate deploy proves future linear authority but
does not create authenticated pre-state REV capacity. Therefore both purses
must be funded in a prior certified state before continuation activation. This
is the concrete native realization of the papers' local sufficiency rule.

The component-wise maximum is also the minimum safe reservation under this
isolation rule. A smaller first-branch or expected reservation can underfund a
reachable branch; a pooled scalar can hide an individual purse deficit; summing
every branch is safe but rejects executions that the component maxima fund.
The path-correlation and capital-feasible concurrency consequences are derived
in [Correctness-constrained reservation optimization](end-to-end-authority-settlement.md#correctness-constrained-reservation-optimization)
and exhaustively cross-checked by the licensed, opt-in Wolfram exploration.

## Reservation, top-up, and settlement

Admission reads the authenticated merged pre-state, derives physical authority
and the exact state-bound byte witness, and binds both to a versioned funding
certificate. For payer $`a`$, let $`L_a`$ be unreserved liquid REV,
$`R^A_a`$ the physical authority reservation, $`R^Q_a`$ the byte reservation,
and $`R^F_a`$ the fee reservation. Admission requires:

```math
R^A_a+R^Q_a+R^F_a\leq L_a.
```

The accepted execution receives the immutable snapshot:

```math
H_a=R^A_a+R^Q_a+R^F_a.
```

An authorized transaction may credit the same purse while the process runs.
That transition increases only unreserved liquid balance:

```math
L'_a=L_a+u,\qquad H'_a=H_a.
```

It cannot expand the certificate, alter the byte schedule, increase the running
budget, or rescue an execution that reaches its certified ceiling. The credit
becomes available to a later admission or other transaction after its own
canonical state boundary. This rule makes top-up commute with settlement while
preventing live balance observations from changing block validity.

On success, one `SystemVault.applyCost` transition burns the physical and byte
settlements, transfers the fee, and returns unused reserved custody. Located
stack pops occur in the same node checkpoint. On failure, the affected RSpace
mutation, stack pops, vault burn, and fee transfer remain uncommitted. Earlier
successful events and explicit failed-call evidence may remain in the errored
deploy's authenticated replay trace; they never authorize settlement above the
fixed certificate.

A located stack production needs a narrower rollback rule inside evaluation.
Its physical cells enter a pending reservation before the introduction byte
charge, because another concurrent operation must not reserve the same cells.
Pending cells are capacity-consuming but witness-invisible. The physical debit
and one stack birth become realized only after the complete RSpace produce and
any matched continuation succeed. Byte rejection, continuation failure,
unwinding, or cancellation drops the pending reservation and restores its exact
cells. Quantitative byte and ordinary reducer attempt costs retain the charging
semantics above; only the unrealized linear transfer is aborted.

When a produce immediately matches, its canonical identity is present in the
COMM's producer vector. Causal settlement extraction therefore visits every
nested producer before the COMM itself. Repeated appearances are harmless
because authority-event identities are consumed once. This closes the evidence
path without changing the COMM identity or arrival-order-independent tariff.

Only authenticated `SystemVault.protocolMint` execution may increase total REV
supply. Wallet transfers, purse top-ups, reservations, refunds, fee transfers,
located-stack funding, and byte settlement conserve existing value.

## Proposal, wire evidence, and replay

The authority protocol binds:

- the accounting protocol version;
- canonical program and pre-state roots;
- reservation and certificate identities;
- physical authority allocation and stack reservations;
- byte schedule version and digest;
- byte bound and component-wise byte allocation;
- fee allocation and recipient; and
- exact realized authority events, byte cost, byte settlement, physical draws,
  born stacks, and post-state root.

Protocol version 8 is the first authority evidence version carrying the byte
schedule and allocation. Node protocol version 4 is the corresponding block
validation boundary. Older or unknown evidence fails closed; there is no
runtime A/B switch.

Replay installs the certified capacity, rigs the committed RSpace trace,
recomputes every introduction and COMM charge, verifies the schedule digest,
reconstructs physical and quantitative allocation, applies the same settlement,
and requires exact cost and post-state-root equality. A proposer cannot choose
its own byte count or use a later purse top-up to validate an earlier execution.

## Relation to legacy `ChargingRSpace`

The legacy node charged serialized produce and consume inputs before invoking
RSpace. It then applied negative storage refunds after a match and charged event
storage. That design recognized the correct safety requirement—communication
bytes consume finite resources—but its refund path depended on the trigger side,
persistence, and removal decisions.

The native refinement retains the safe prefix and removes the unstable suffix:

| Legacy behavior | Native behavior |
| --- | --- |
| charge produce/consume inputs before RSpace | charge every stable semantic introduction before RSpace mutation; persistent fixed-point retries are idempotent |
| refund removed or trigger-side storage | no live byte refund or removal credit |
| charge event storage after result | precharge the complete committed COMM trace before mutation |
| mutable counter only | fixed RevVault certificate plus exact witness |
| runtime result determines accounting order | canonical event identities and commutative reconciliation |
| replay repeats wrapper arithmetic | replay recomputes versioned evidence and settlement |

This is compatible with lazy cost accounting: introductions charge work actually
submitted to RSpace, while payload delivery and committed trace charge only when
the interaction fires. An unselected branch creates no introduction and incurs
no byte charge.

## Implementation map

| Responsibility | Implementation |
| --- | --- |
| schedule, digest, canonical footprints, checked arithmetic | `rholang/src/rust/interpreter/accounting/byte_accounting.rs` |
| fixed budget, canonical event reconciliation, COMM/byte product | `rholang/src/rust/interpreter/accounting/mod.rs` |
| pending physical stack reservation and commit/abort ownership | `rholang/src/rust/interpreter/accounting/mod.rs`, `reduce.rs` |
| proposal and replay observer wiring | `rholang/src/rust/interpreter/rho_runtime.rs` |
| pre-mutation introduction and COMM hooks | `rspace++/src/rspace/rspace.rs`, `replay_rspace.rs`, `rspace_interface.rs` |
| certificate and witness algebra | `rholang/src/rust/interpreter/accounting/authority.rs` |
| protobuf evidence | `models/src/main/protobuf/CasperMessage.proto` |
| admission, allocation, refund, settlement validation | `casper/src/rust/util/rholang/acceptance.rs` |
| retained play and exact replay | `casper/src/rust/rholang/runtime.rs`, `replay_runtime.rs` |

## Verification and regression obligations

The proof boundary is intentionally end to end:

| Property | Formal evidence | Executable evidence |
| --- | --- | --- |
| introduction and COMM product decomposition | Rocq `trace_debit_is_product_sum` | runtime combined-cost regression |
| full producer/consumer arrival-order equality | Rocq `trigger_arrival_order_does_not_change_total_debit`; TLA+ `ExactCanonicalDebit` | RSpace either-trigger observer test |
| every join payload and trace participant is charged | Rocq `join_transfer_includes_every_participant`, `adding_join_participant_adds_exact_transfer_cost` | byte-accounting proptests |
| persistent introductions are charged once and deliveries repeatedly | Rocq `persistent_introduction_is_charged_once_and_each_delivery_is_charged`, `stable_persistent_identity_is_charged_once_across_retries`, and `nonpersistent_identity_preserves_attempt_multiplicity` | persistent produce/consume, concurrent retry, reset, and play/replay tests |
| checked hard ceiling and rejection atomicity | Rocq `accepted_byte_event_preserves_hard_ceiling`, `rejected_byte_event_is_atomic`; TLA+ `HardReservationCeiling`, `RejectedAttemptIsAtomic` | overflow unit tests, RSpace rejection tests, Loom reservation and persistent-identity races |
| top-up conservation and snapshot immutability | Rocq `top_up_is_a_conserving_transfer`, `top_up_does_not_expand_inflight_reservation`; TLA+ `CanonicalValueConserved`, `ReservationSnapshotImmutable` | Loom top-up/settlement interleaving |
| exact play/replay cost and root | Rocq `replay_byte_trace_accepts_iff_exact`, `replay_acceptance_binds_event_kind_and_amount`, and `replay_rejects_changed_event_kind`; TLA+ `ReplayMatchesPlay` | Casper state-bound settlement/replay tests |
| introduction sponsorship cannot mutate stored authority or race across payers | Rocq `authority_neutral_stack_keeps_storage_neutral_and_charges_sponsor`, `stored_interaction_authority_cannot_redirect_introduction_charge`, and the introduction-registry theorems; TLC and Apalache `IntroductionAuthorityRegistry` safe model | runtime neutral-stack restoration tests; 256-case registry property test; Loom atomic fallback/explicit registration and same-/different-payer registration interleavings; split-fallback unsafe model must violate `ResolvedMatchesCommittedRegistry` |
| candidate-created authority cannot create quantitative credit | Rocq `candidate_created_stack_cannot_supply_prestate_byte_capacity`; TLA+ `ContinuationActivationRequiresFunding` | same-deploy funded-stack replay regression and underfunded candidate rejection tests |
| wallet-funded lollipop settlement is staged, local, and conserving | Rocq `WalletFundedLollipop.v`; TLA+ and Apalache `WalletFundedLollipop` safe model | cross-deploy wallet-funded lollipop and same-deploy stack-transfer play/replay regressions |
| physical authority remains distinct from COMM count | existing n-ary authority proofs; Rocq `byte_trace_refines_single_comm_execution` and `single_cell_authority_settlement_matches_processed_trace`; TLA+ `PhysicalAuthorityIsTrackedSeparately` | matched/unmatched and multi-region cost regressions |
| stack introduction is failure-atomic across physical, RSpace, and enclosing-deployment effects | Rocq `StackIntroductionAtomicity.v`; TLA+ and Apalache `StackIntroductionAtomicity` plus six unsafe controls | RAII unit/proptest coverage, Loom rejection/pre-mutation-cancellation/competition/deployment-rollback interleavings, matched-produce extraction, deploy-abort, and Casper stack play/replay regressions |
| parser, reducer, play validation, replay validation, and derived-evidence publication share one state-and-witness transaction | Rocq `EvaluationTransactionIsolation.v`; TLA+ and Apalache `EvaluationTransactionIsolation` plus five unsafe controls | parser-after-paid-deploy, reducer-attempt-retention, deploy-stack-rollback, forged replay post-state active-root, and rejected-final-state no-evidence regressions |
| component-wise branch maximum is the minimum isolated-purse reservation and does not serialize disjoint purses | Wolfram `reservation_admission_regions.wl` is an optional optimization cross-witness over the authoritative local-sufficiency premises | `delta_sigma` alternative-maximum tests, state-bound exact-boundary properties, cross-group shared-component contention, and Loom independent-purse reservation interleavings |

The TLA+ unsafe controls independently refute mutation-before-charge,
arrival-side-only introduction charging, omitted join participants, persistent
recharge, peek credit, replay trace omission, live top-up expansion, and wrapping
overflow. Apalache cross-checks the safe bounded lifecycle and every negative
control. The Rocq module is axiom-free and included in the aggregate proof
hygiene gate.

## Operational consequences

- Wallets and purses are reusable and refillable.
- A running process can coexist with authorized credits to its purse, but its
  reservation remains fixed.
- Insufficient reserved REV rejects before the affected RSpace mutation.
- Genesis creates and funds initial custody through authorized protocol minting;
  it is not the ordinary per-process precharge operation.
- A later deploy may use a prior top-up or refund after observing the resulting
  canonical state.
- Validators accept a block only when byte tariff, physical authority,
  settlement, cost, trace, and post-state root all replay exactly.
