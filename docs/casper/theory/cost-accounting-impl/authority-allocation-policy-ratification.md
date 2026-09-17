# Authority-allocation policy ratification

## Decision

The [allocation policy](authority-allocation-policy-decisions.md) defines the selected funding allocation behavior.
The table below summarizes its requirements without replacing their exact domains or proof obligations.

## Approved choices

| Subject | Approved behavior |
| --- | --- |
| Restricted ties | Select a cyclic lexicographic maximum among equally ranked optimal contribution vectors. Preserve complete feasible assignment witnesses. |
| Fragment dispatch | Derive the mandatory fragment from the complete captured contribution domain. Do not let the proposer select a different rule. |
| Unrestricted cursor | Preserve existing all-to-all contributions and residual transitions under the specified refinement hypotheses. |
| Restricted cursor | Advance one physical-cohort position after each successful positive restricted settlement. Preserve the documented limits of the priority-cycle guarantee. |
| Zero new funding | Publish no contribution-cursor update. Preserve other validation and keep the separate deployment-fee policy unchanged. |
| Witness ties | Use canonical ordering only among semantically equivalent complete assignments with identical selected contributions. |
| Acquisition alternatives | Minimize new monetary cost among authorized equivalent acquisitions, then apply fixed-total lexicographic minimax. |
| Resource and fee composition | Select resources from the jointly feasible domain, with resource fairness first. Apply the separate fee rule and cursor afterward. |
| Fee sponsorship | Permit explicitly authorized fee sponsorship as an optional arrangement. Do not require a sponsor or debit an administrator implicitly. |
| Outcome families | Use canonical outcome priority over complete feasible families. Compare fairness ranks before allocation ties. Keep family-wide worst-case fairness as an alternative, not the selected policy. |

The approved document supplies the exact domains, boundaries, examples, and verification requirements for these choices.
The shorter table does not replace those requirements.

The [resource and fee composition](lexicographic-minimax-funding.md#approved-resource-and-fee-composition) defines the joint domain and objective precedence.
Its [alternatives comparison](lexicographic-minimax-funding.md#alternatives-considered) records the advantages and disadvantages of all six considered options.
Approval of this composition does not establish completed joint optimization or native settlement integration.
The [outcome-family policy](lexicographic-minimax-funding.md#selected-canonical-outcome-priority) states the selected ordering and its integration boundaries.
The [worst-case comparison](lexicographic-minimax-funding.md#alternative-family-wide-worst-case-fairness) explains when a different liability objective could be useful.

## Remaining work

Approval does not establish completed proofs, production optimization, native integration, or CI readiness.
The classifier and optimizer still need bounded-work guarantees and implementation correspondence.
The rejected cursor rule must remain a negative control and a native regression target.
Resource valuation, authority preservation, refund provenance, and concurrent publication remain mandatory constraints.

The [economic and activation policies](economic-activation-policy-ratification.md) separately define failure charges, prepaid compatibility, exposure, conversion, and activation.
No Casper architecture change, persistent deployment escrow, or global funding lock is authorized.
