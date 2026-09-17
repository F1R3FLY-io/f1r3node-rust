# Three-paper cost-accounting claim audit

## Status and scope

This initial audit checks selected proof boundaries at commit `9e58cf211885b2cdb319e030efa2af0491d71471`.
It is not a completed conformance catalog or a release qualification.

The user deferred Casper changes on September 9, 2026, pending discussion with the upstream team.
The exception covers demonstrated blocking defects present in both this branch and `dev`.
This audit adds documentation and inspection tools.
It does not change consensus, funding policy, or the scope of the approved implementation.

## Specification inputs

The local specification files are in the sibling `publications` repository.
These SHA-256 digests identify the inspected revisions.

| Specification | Local path within `publications` | SHA-256 |
| --- | --- | --- |
| Cost-Accounted Rho | `cost-accounting/cost-accounted-rho.tex` | `71c01eb67e2d3447c5f8e6f90e494e7120587991165466382b53b0565dc0051c` |
| Continued Interactive GSLTs and the Cost Endofunctor | `cost-accounting-as-monad/continued-gslt-cost-v2.tex` | `c91a5f2f75ff960e78c415249945084583d493c31bf7b1d7b1a5088a898cc9b6` |
| Knotted topoi | `knotted-topoi/knotted-topoi.tex` | `02a5b1349e3d6a8d5157b7173e00e8d3e83fbfcae742136e20025497acbe96af` |

Use source labels when checking a claim. Line numbers can change between paper revisions.
The papers supply the specification. A proof filename or theorem name does not establish implementation conformance.

## Inspected proof boundaries

### Linear implication

The rho paper's `rem:tensor-hom` relates compound funding to staged funding.
The remark leaves the full adjunction development to a sequel.

[`LLIdentities.v`](../../../formal/rocq/cost_accounted_rho/theories/LLIdentities.v) defines a channel as a list of natural numbers.
It defines both `tensor_channel` and `lolly_channel` as list concatenation.
The theorem `lolly_curry_isomorphism` proves regrouping under list permutation through `app_assoc`.

This is an algebraic identity in that model.
It does not construct an internal hom, a natural hom-set correspondence, or an operational staged-funding simulation.
The historical audit incorrectly used this theorem as evidence for the full adjunction.
The corrected audit now states the narrower result.

The runtime also distinguishes capability connectives from funding signatures.
[`Sig::is_funding_former`](../../../rholang/src/rust/interpreter/accounting/mod.rs) excludes `Sig::Lolly` from the funding grammar.
This distinction does not establish that the separate linear-transfer syntax is missing or incorrect.
Its desugaring, authority transfer, and settlement require separate evidence.

#### Transfer syntax and explicit authority layers

The rho paper defines transfer syntax in `eq:sugar-lollipop` and uniform signing in `eq:sugar-uniform`.
[`desugar.rs`](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/desugar.rs) wraps the receive continuation with the destination authority.
[`recognize_signed_term`](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/recognize.rs) assigns the source authority to the outer receive.
The native output contains ordinary signed terms, not a funding `Sig::Lolly` constructor.

[`SyntacticSugar.v`](../../../formal/rocq/cost_accounted_rho/theories/SyntacticSugar.v) proves equality of the specified translation images through structural equivalence.
Its two images share the same construction by definition.
The proof does not execute the native normalizer or verify the resulting wallet settlement.
Fresh compilation and an independent kernel check passed on September 9, 2026.

The [generated normalization tests](../../../rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/sugar_properties.rs) compare shorthand with explicit native authority layers.
They include compound authorities, multiple receive bindings, persistent and peek receives, bound names, payload capture, and transfer chains.
These tests address native normalization correspondence without changing Casper or the production normalizer.
They do not establish complete operational bisimulation or end-to-end funding correctness.

The focused normalizer run passed all 24 tests with none ignored and 506 unrelated tests filtered.
Each of the four generated properties used 1,024 cases.
The generators cover one to eight compound members, one to eight receive bindings, and two to nine transfer stages.
These are test bounds, not language limits.
The test log is `target/verification/claims-audit-20260909/sugar-tests.log`.

#### Native runtime attribution

The [runtime transfer properties](../../../rholang/tests/accounting/located_authority_spec/transfer_properties.rs) execute multiple transfer chains in one Rholang process.
The Tokio executor has two worker threads.
The test oracle counts each expected authority occurrence independently of the emitted event log.

Each successful receive must charge its current authority exactly once.
Each completed chain must produce its expected terminal payload.
Every emitted authority event must pass the production authority check.
The total byte-event amount must equal the runtime's quantitative byte cost.

The fixed examples cover repeated and distinct payers across three chains.
They vary whether sends precede or follow receives in source order.
The generated property uses 32 cases with one to four chains and two to eight stages per chain.
Payer identities come from a sixteen-member set, which permits repeated and shared payers.
Source-order variation does not enumerate all thread schedules.

All 28 focused located-authority tests passed on September 9, 2026.
The run ignored no tests and filtered 283 unrelated integration tests.
Strict Clippy for the integration target also passed.
Both commands used one build job, a 4 GiB memory cap, and disabled swap.
The logs are `transfer-runtime.log` and `transfer-runtime-clippy.log` under `target/verification/claims-audit-20260909/`.

These runtime tests establish observed cost attribution, not authenticated purse withdrawal or multi-validator settlement.
The existing tests also check checkpoint restoration of stored payload and continuation authority.
That check is not a process-crash or distributed-recovery test.

### Cryptographic naming

[`Translation.v`](../../../formal/rocq/cost_accounted_rho/theories/Translation.v) assumes an injective encoding from bit lists to abstract processes.
The rho paper's `app:sig-trans` requires distinct ground signatures to have distinct process encodings.
That encoding requirement differs from collision resistance for a fixed-size cryptographic digest.

[`Sig::lane_hash`](../../../rholang/src/rust/interpreter/accounting/mod.rs) hashes a domain-prefixed encoded channel with Blake2b-256.
An injective abstract encoder does not prove that this digest has no collisions.
The implementation correspondence must identify canonical encoding, domain separation, and cryptographic assumptions separately.
No digest collision or runtime naming defect was demonstrated by this inspection.

### Resource formulas

[`oslf.rs`](../../../rholang/src/rust/interpreter/accounting/oslf.rs) implements a finite, located resource formula checker.
Operational Semantics in Logical Form (OSLF) supplies the intended logical framework.
The checker distinguishes exact demand from a conservative upper bound.

[`CAOSLFSpatialModal.v`](../../../formal/rocq/cost_accounted_rho/theories/CAOSLFSpatialModal.v) models those resource observations and checks.
An upper bound can establish sufficient funding without establishing that a particular interaction occurred.
Existing Rust examples and a generated disjoint-spend property address parts of this boundary.
This inspection did not rerun those tests or establish complete runtime correspondence.

The zero-grade repair adds focused proof and runtime evidence.
It does not establish complete runtime correspondence or classifier adequacy.

Neither the finite checker nor these arithmetic lemmas alone establishes the knotted-topos classifier theorem.
That theorem requires the distinct correspondence described by `ob:classify`.

### Monetary fee and acceptance discovery

The rho paper's `eq:fee-extract` transfers one authority token to the fee collector.
It does not specify token-to-REV conversion for arbitrary monetary payer sets.
The user's one-total-fee requirement therefore needs an explicit mapping from authority obligations to monetary obligations.

The [native multi-wallet regression](../../../casper/tests/util/rholang/multi_payer_fee.rs) exposes two distinct gaps in the working tree based on the audited commit.
With `Nil`, two selected signers credit two monetary units and three selected signers credit three.
Replay agrees with those balances, so replay equality does not establish the required total fee.
This finding does not refute the paper's authority algebra.

The paper's `sec:acceptance-protocol` requires sufficient supply for a conservative bound over all reachable branches.
The native regression also checks one-COMM processes whose authenticated signer wallets have funds but whose compound wallet has none.
Before the initial-capacity repair, the multi-signer cases rejected with zero initial execution capacity.
The initial capacity inventory contained the compound signature but did not expand it into authenticated leaf custody.

The [approved discovery repair](multi-wallet-funding-path-audit.md#approved-initial-capacity-repair) includes authenticated signer leaves and preserves distinct physical custody accounting.
All eight native cases now pass admission and replay checks.
Six cases still fail the independent one-total-fee assertion.
The repair has separate bounded TLA+ checks, abstract Rocq inventory proofs, and generated native membership and capacity tests.
These checks do not establish the paper's static all-branch acceptance guarantee.

[`StateBoundFrontierExpansion.tla`](../../../formal/tlaplus/cost_accounted_rho/StateBoundFrontierExpansion.tla) starts with positive capacity and supplies later discovery directly.
[`StateBoundFrontierExpansion.v`](../../../formal/rocq/cost_accounted_rho/theories/StateBoundFrontierExpansion.v) takes undiscovered backing as an input list.
Neither artifact proves that native execution can discover the required backing before an initial zero-capacity failure.
Their arithmetic and termination claims do not establish the paper's static all-branch acceptance guarantee.

The [funding path audit](multi-wallet-funding-path-audit.md#approved-test-implementation) records the exact cases, results, source hashes, and limits.
The [formal fee control](multi-wallet-funding-path-audit.md#checked-fee-obligation-boundary-control) demonstrates that existing conservation checks accept an excessive supplied fee.
Adding the independent monetary invariant rejects that trace.
This checks a proof boundary, not a production repair or complete native refinement.
Both catalog entries remain `source_inspected`.
The catalog check validates references, not the failing behavior or a future correction.

## Knotted-topoi obligation inventory

The paper states the following obligations explicitly.
Every row remains open in this audit until specific evidence establishes the required correspondence.
An open audit row does not assert that no relevant artifact exists elsewhere.

| Paper label | Required subject | Boundary that the audit must preserve |
| --- | --- | --- |
| `ob:exist` | Existence of the knotted topos | Finite syntax and resource maps do not construct the required categorical fixpoint. |
| `ob:cohere` | Swap and knot coherence | Constructor equalities alone do not establish the required triangle and pentagon identities. |
| `ob:opcorr` | Operational correspondence of desugaring | Match transitions in both directions, including persistence and location labels. |
| `ob:classify` | Classification of bisimilarity | Connect the internal modal theory to the stated subobject-classifier logic. |
| `ob:barbed` | Observational calibration | Connect context bisimulation to native Rholang observational equivalence. |
| `ob:functor` | Functoriality of encodings | Identify the actual source and target morphisms and their preserved structure. |
| `ob:size` | Finitary and infinitary calibration | Keep executable finite structures distinct from the full construction. |
| `ob:metric` | Internal ultrafilter metric | Resource arithmetic alone does not establish this metric construction. |
| `ob:place` | Relation to neighboring semantic models | Supply a mathematical comparison rather than an implementation-presence claim. |

This inventory does not add implementation tasks for the whole paper.
The remaining audit must map each obligation to the approved cost-accounting specialization and record any explicit scope boundary.
Do not treat unreviewed exclusions as approved decisions.

## Remaining work

1. Map the remaining paper claims to exact definitions, hypotheses, runtime paths, and tests.
2. Separate proof existence from checked proof execution and implementation refinement.
3. Record the invariants that require executable property tests.
4. Extend the initial claim catalog to cover all approved requirements.
5. Reconcile the final audit with integrated validation after the deferred Casper review.

The [executable conformance matrix](cost-accounting-executable-conformance-matrix.md) contains earlier implementation status claims.
This audit does not reconfirm all of those claims.
The [historical alignment audit](cost-accounting-spec-alignment-audit.md) retains the earlier rationale and now links to these narrower proof boundaries.

## Machine-readable inventory

The [TSV catalog](../../../formal/catalog/cost-accounted-rho-three-paper.tsv) records the seventeen claims inspected or inventoried above.
It does not yet cover every claim in the three papers.
Tab-separated values (TSV) keep each claim in one record with eight named fields.

| Field | Meaning |
| --- | --- |
| `claim_id` | Stable identifier for the claim. |
| `paper` | One of `rho`, `monad`, or `knotted`. |
| `label` | The exact LaTeX label in the pinned paper. |
| `claim_class` | The claim category, such as `adjunction` or `classifier`. |
| `status` | `open`, `source_inspected`, or `assumption`. |
| `boundary` | The precise kind of available evidence or unresolved obligation. |
| `source_refs` | Repository-relative `file#symbol` references, separated by semicolons, or `-` for an open row. |
| `limitation` | The explicit limit of the evidence. |

An `open` record uses the boundary `unmapped-obligation`.
A `source_inspected` record uses `abstract-algebra` or `runtime-source`.
An `assumption` record uses `encoding-premise` or `cryptographic-premise`.
The schema does not accept completion, verification, or exclusion claims.

Run the inventory check from the repository root:

```bash
bash scripts/check-cost-accounted-rho-claims.sh
```

The checker requires the sibling `publications` checkout.
Use `--publications-root PATH` when the checkout has another location.
A missing paper, changed digest, missing label, or missing referenced symbol fails the check.
The checker also rejects missing required records, duplicate identifiers, malformed fields, and unsupported status boundaries.

The checker confirms symbol occurrence, not a declaration, a proof, or semantic correspondence.
It cannot determine whether arbitrary prose makes an unsupported mathematical claim.
A semantic review must check each limitation and the actual definitions.
Passing this check must not satisfy a proof or release gate.
The aggregate CI workflow does not run this new inventory check yet.

The [checker tests](../../../scripts/claims/test_catalog.py) cover record mutations, all status-boundary combinations, input drift, and invalid references.
Generated tests use seeds `216` and `390`, with 200 cases for each generated property.
The suite also checks that a symbolic link cannot escape the selected input root.

On September 9, 2026, all 21 checker tests passed.
The latest catalog check passed with six inspected records, two assumptions, and nine open records.
All 21 checker tests passed after the fee-mapping and acceptance-discovery entries became mandatory.
The logs are `catalog-17-check.log` and `catalog-17-tests.log` under `target/verification/claims-audit-20260909/`.
Both checks ran under a 1 GiB systemd memory limit with swap disabled.
These results qualify the inventory checker only.
Logs are under `target/verification/claims-audit-20260909/`.
