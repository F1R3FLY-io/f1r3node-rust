# Offered-funded production integration proposal

**Status:** Proposal, 2026-10-01. No protocol activation is authorized by this document.

**Target:** `feature/casper-cost-accounting-completion`, derived from the cost-accounting work. The current `feature/cost-accounted-rho` milestone covers the Rholang, model, and native RSpace layers. Its [implementation guide](README.md) assigns Casper production bindings to the derivative branch. This proposal does not reopen unrelated Casper consensus, finality, or recovery repairs.

## Decision requested

The saved Casper code contains a working **body-only, state-bound** accounting route. It does not connect the newer **offered-funded** model to ordinary submission, proposal, block replay, and durable settlement. Full production activation needs that connection. It is not a missing requirement for the narrowed non-Casper milestone.

The first design decision is whether a certificate built from one privately executed, state-bound outcome can satisfy the approved **complete funding family** contract. The current [formal contract](signed-phlo-formal-contract.md) requires an up-front proof for every permitted execution in the certified scope. Checking one completed trace is explicitly insufficient under that contract. Until a formal refinement proves a narrower state-bound scope valid, implementation must retain the existing complete-family requirement and keep offered-funded production activation closed.

I recommend a bounded private-candidate design **only if** that refinement is approved and proved. Otherwise, build a conservative complete-family producer before execution. Both paths must pass the same independent-replay and atomic-publication gates below.

## Terms and boundary

- **Offered-funded envelope:** A signed deploy that commits to offered pricing, execution limits, and funding intent. The model type is `OfferedFundedDeploy`.
- **Funding family:** Checked source capacities and all certified outcome cases. Each case carries resource obligations, eligibility, and allocation.
- **Original funding root:** The state root at which payer wallets and prepaid resources are authenticated before execution.
- **Settlement runtime root:** The root after a successful user execution, or the restored root for a classified user failure. Wallet and receipt changes apply at this root.
- **Prepaid receipt:** A rooted record for an already acquired resource. A newly born retained resource may issue a receipt after settlement, but cannot fund its own birth.
- **Host-work budget:** A separate bound on machine work and allocation. Economic phlo charges do not substitute for it.
- **Publication:** The atomic transition that makes the user result, wallet debits, receipt changes, and evidence visible to later deploys.

The intended flow is [shown here](../diagrams/offered-funded-production-flow.svg). Blue steps authenticate, amber steps prepare private evidence, green steps publish or validate, and red steps reject without leaking a partial state.

## Verified shortcomings and proposed changes

The observations below are from the saved derivative commit `f9bd3895d` and the current cost-accounting worktree. Saved-path citations use `git show f9bd3895d:<path>` so they remain unambiguous while the dev merge is in progress.

| Boundary | Observed shortcoming | Proposed change |
| --- | --- | --- |
| Ordinary ingress | Saved `pending_deploy.rs:57-64` accepts only `BodyV61` at protocol 6 or above. `block_creator.rs:3645-3649` converts pending deploys to body envelopes. `casper_message.rs:1881-1891` rejects funded variants in `to_cosigned`. | Carry the authenticated offered envelope through API, pending storage, proposer, block body, and validator dispatch. Gate the new format using approved fresh-genesis policy. Reject it before activation. |
| Funding proof | [`PhloFundingIntentV1`](../../../../models/src/rust/phlo_intent.rs) signs controls, schedule commitment, exposure, and sources, but no outcome cases. [`CheckedPhloFundingFamily`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/funding.rs) requires cases. Saved `direct_wallet_funding/execution.rs:97-126` requires `CheckedDirectWalletPolicy` before evaluation. Saved `execution/settlement.rs:56-145` derives the observed case after evaluation. | Separate offer authentication from checked family construction. Resolve the complete-family decision below. Never fabricate a family from a caller-supplied test fixture in production. |
| Prepaid selection | Saved `prepaid_receipts/inventory/demand.rs:24-31,87-185` checks supplied draws and demand positions. No ordinary producer derives those choices from the original-root inventory. | Produce a deterministic, bounded selection from authenticated receipt and physical inventory. Reuse the existing binding as a verifier. Reject foreign-root, stale, duplicate, or partly backed selections. |
| Retained births | Saved `prepaid_receipts/births.rs` can bind authorized retained births to physical stacks. Its helper is not reached from ordinary offered-funded execution. | Derive births from the completed authority and physical-stack evidence. Issue exact receipts in the same publication as wallet settlement. Charge new acquisition on the current attempt. |
| Transaction ownership | Saved `prepaid_receipts/settlement.rs:66-170` checkpoints wallet and receipt mutation inside its helper. That does not cover the entire ordinary user execution and evidence publication. | Own one outer transaction over user effects, wallet debits, receipt replacement, evidence, and final root. Roll back every component on rejection or platform failure. |
| Replay evidence | Saved `execution/replay.rs:24-30,72-170` accepts budget recording, operation records, and events as helper arguments. The ordinary processed-deploy wire record carries older state-bound fields, not this complete native evidence. | Add a bounded canonical evidence field to `ProcessedDeployProto`, tentatively field 14 after existing fields 11-13. Specify exact encoding and commitments before assigning the tag. Validators independently reconstruct and compare the execution and settlement. |
| Native directives and host bounds | [Observed funding outcome](observed-funding-outcome.md) states that ordinary funded runtime directives are not emitted and funded ingress remains disabled. It also records remaining storage write and cleanup bounds. | Complete directive emission, source authentication, full journal consumption, and host reservations before enabling funded ingress. Preserve independent replay and fail closed on missing evidence. |
| Fork and block composition | Private candidate helpers do not establish safe ordinary scheduling across shared wallets or receipts. | Advance the accepted state root after each deploy. Recheck source old-values before publication. Retry a stale private candidate at the new root. Add targeted accounting-dependent merge handling only if cross-fork tests show dev behavior is insufficient. |

This table does not imply the saved body-only path is defective. It is a different, narrower path. The current dev-based Casper runtime still uses legacy precharge and refund in `casper/src/rust/rholang/runtime.rs:383-415`; that behavior must be preserved for formats accepted by the target network.

## Funding proof: the blocking design question

The intended offer authenticates an upper execution limit, a price schedule, owner consent, source exposure, and payer identities. Actual resource obligations depend on native execution. The saved helper's type order requires a checked family before executing, but only execution supplies its measured case. Moving the existing calls around cannot resolve this cycle.

**Preferred investigation: state-bound private candidate.** Authenticate a `VerifiedNativeOffer` at the original root, reserve host work, and execute inside an unpublished checkpoint. Bound execution by the signed `phloLimit`. Derive the exact observed case and canonical allocation from complete measurements. Certify it only if a new proof establishes that the certified scope contains exactly the replayable state-bound outcome. The proof must cover deterministic external inputs, failure classes, dependency order, rollback, and absence of effects before certification. If these conditions fail, discard the candidate without a wallet charge or partial publication.

**Conservative alternative: complete pre-execution family.** Generate a bounded family covering every outcome permitted by the signed controls and execution environment. Check source exposure and capacity before user execution. The current formal contract directly supports this route. The producer must either demonstrate finite enumeration within configured caps or reject an offer whose family cannot be proved. It cannot silently substitute one observed case.

The decision requires an explicit amendment or refinement of [the signed phlo contract](signed-phlo-formal-contract.md), a Rocq statement that connects the chosen producer to the family checker, and an executable regression that distinguishes a missing permitted case from a valid certificate. Until then, neither proposed path is ready to activate. The offer's signed price and policy commitments remain unchanged in either design.

## Proposed end-to-end implementation

The following pseudocode describes the **conditional** state-bound path. Each stage has an owned failure boundary. It is a design sketch, not a claim that these calls already exist.

```text
submit(envelope):
    verify canonical offered format, signatures, owner consent, and genesis policy
    authenticate wallets and prepaid inventory at original_root
    freeze signed controls, price, exposure, and execution context
    prepare a bounded private candidate at original_root

prepare(candidate):
    run native user execution under phloLimit and host-work limits
    capture complete operation journal, budget record, events, and byte observations
    derive prepaid draws from original-root receipts and measured demand
    derive retained births from actual authority and physical stacks
    prove the chosen funding-family contract and calculate exact settlement
    reject if any recording, match, backing, price, or bound is incomplete

publish(candidate):
    recheck original-root funding inputs and current settlement root
    under one owned checkpoint, apply user result, wallet debits, and receipt changes
    commit canonical evidence and the adjacent pre-state and post-state roots
    on any failure, restore the checkpoint and release private resources

validate(processed):
    load the historical rule selected by the authenticated envelope format
    independently replay from the submitted pre-state root and original funding root
    regenerate demand, prepaid selection, births, family, and settlement
    compare complete journal, costs, failures, receipts, and both resulting roots
    accept only after full evidence consumption and exact equality
```

The original funding root and settlement runtime root must remain distinct. This matters when successful user execution changes RSpace before wallet settlement. It also matters when a classified user failure restores the user checkpoint but retains only authorized billable work. Platform, certificate, and unclassified failures must not publish candidate charges, as required by [economic policy](economic-activation-policy-ratification.md).

The evidence field should encode a versioned, length-limited record. It needs commitments to the envelope, policy and schedule, original funding root, selected family case, budget recording, operation journal, prepaid input and output, wallet debits, failure classification, and resulting root. Existing deploy log, cost, and pre/post root fields remain authoritative where already defined. The wire design must specify canonical ordering, limits, unknown-field rejection, and historical decode behavior before protobuf changes. A digest alone is insufficient when replay must reconstruct operation directives.

Independent validation must never accept a helper's precomputed allocation or receipt list on trust. It must regenerate them from authenticated state and replayed demand. Each accepted deploy advances the block's root before the next deploy. Private evaluations may run independently, but shared-wallet or shared-receipt candidates need a root and old-value check at publication. A stale candidate retries without duplicate charges.

## Connected implementation batches and exit evidence

1. **Contract decision and authenticated ingress.** Prove or reject the state-bound exact-case refinement. Define the envelope activation rule and canonical evidence schema. Connect public submission through pending storage and proposal without body-only conversion. Evidence: wrong format, price, consent, root, and schedule reject before publication.
2. **Candidate producer and prepaid backing.** Implement the approved family producer, measured demand projection, deterministic prepaid selection, retained-birth capture, and host reservations. Evidence: fresh, prepaid, mixed, retained, malformed, and exhausted cases; payer counts through the configured cap; conservation and no same-attempt prepaid birth.
3. **One transaction and replay.** Connect ordinary execution to a single outer checkpoint and publish complete evidence. Connect independent validator replay and exact root comparison. Evidence: cold/warm parity, user failure, platform failure, duplicate settlement, foreign root, retry, cancellation, and concurrent independent sessions.
4. **Protocol qualification.** Test every accepted historical format under its historical rules and the new format under fresh-genesis policy. Run model/property, Rocq, TLA+/Apalache, Loom, integration, formatting, Clippy, documentation, and security gates on one pinned candidate. Multi-node results must show matching state, funding, and receipt roots.

The first batch is a genuine gate. A failed proof or unbounded complete-family producer blocks offered-funded activation. It does not invalidate the lower-layer milestone or justify unrelated Casper repairs. The remaining batch tests are acceptance criteria for this future production integration, not claims of current verification.

## Post-mortem: why the gap was missed

**Observed facts.** The [implementation guide](README.md) already separated the current non-Casper milestone from later Casper production bindings. The [verification catalog](../cost-accounted-rho-verification.md) states that native COMM fixtures supply an explicit funding case and prepared prepaid state. They do not prove complete family generation or production receipt issuance. [Observed funding outcome](observed-funding-outcome.md) states that ordinary funded directives and ingress remain disabled. The saved derivative branch has native helper APIs, while its ordinary proposer and replay path still convert to body-only envelopes. Rocq's [`state_bound_certificate_funds_committed_cost`](../../../../formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v) assumes a valid certificate and equal committed/replayed cost; it does not construct the certificate from a public deploy.

**Assessment failure.** My earlier completion claims treated a real body-only accounting path as evidence that the newer offered-funded path was production-connected. I also treated direct helper tests and conditional formal theorems as proof of ordinary ingress-to-replay reachability. That was wrong. I further described this gap as blocking the current narrowed milestone, despite the explicit branch split in the guide. The present review corrects both errors.

**Likely contributing factors, not independently established historical causes.** The large saved Casper diff mixed cost-accounting support with unrelated Casper work, which made route ownership harder to see. Several conformance statements describe a completed older state-bound route beside caveats for offered funding. Reviews focused on local invariants, fixtures, and proof premises rather than tracing a user submission through every production call site. These factors explain how a green local test or proof result could be overread, but they do not excuse the mistaken scope and completion statements.

**Prevention.** Future completion reviews must map each advertised format from public ingress to proposer, stored block, independent replay, settlement, and state publication. Each link needs a non-test production caller and a failure test. Verification reports must state theorem premises and supplied fixture data beside the claimed result. Release status must distinguish body-only state-bound accounting, lower-layer offered funding support, and actual offered-funded activation. A full public-submission-to-peer-replay test must be a gate before claiming the latter complete.

## Source map

- [Implementation scope](README.md), [signed formal contract](signed-phlo-formal-contract.md), [activation policy](economic-activation-policy-ratification.md), [native gap notes](observed-funding-outcome.md), and [verification catalog](../cost-accounted-rho-verification.md).
- [`PhloFundingIntentV1`](../../../../models/src/rust/phlo_intent.rs), [`CheckedPhloFundingFamily`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/funding.rs), and [formal state-bound certificate](../../../../formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v).
- Saved derivative commit `f9bd3895d`: `block-storage/src/rust/deploy/pending_deploy.rs`, `casper/src/rust/blocks/proposer/block_creator.rs`, `models/src/rust/casper/protocol/casper_message.rs`, `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution.rs`, `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/settlement.rs`, `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/replay.rs`, `casper/src/rust/util/rholang/costacc/prepaid_receipts/inventory/demand.rs`, `casper/src/rust/util/rholang/costacc/prepaid_receipts/births.rs`, `casper/src/rust/util/rholang/costacc/prepaid_receipts/settlement.rs`, and `models/src/main/protobuf/CasperMessage.proto`.
