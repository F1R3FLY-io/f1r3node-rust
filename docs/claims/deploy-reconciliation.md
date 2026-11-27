# Never-observed deploy reconciliation

## Scope

This claim covers the read-only verdict in [deploy reconciliation](../../casper/src/rust/api/deploy_reconciliation.rs).
The [block API](../../casper/src/rust/api/block_api.rs) checks the signature match and shard before it calls this verdict path.
The transport decoders verify the signed deploy. A signature-only request uses the register lookup.

## Verdict conditions

1. A terminal or open lifecycle record supplies the answer.
2. A request without a record can return `Expired` only after the resolved floor passes the reconciliation bound.
3. A request can return `Expired` only when the carrier index proves absence. A held carrier always keeps the verdict `Pending`.
4. Missing history keeps the verdict `Pending`. Storage errors propagate to the caller.

The bound adds the deploy lifespan, merge floor-distance allowance, and citability horizon to the signed valid-after height.
The lifecycle computes the bound with saturating arithmetic. A bound at `i64::MAX` cannot pass the strict height comparison.
An unlimited horizon prevents an expiry verdict.

[Transaction expiration](../../casper/src/rust/validate.rs) checks the carrier's frozen floor.
A live carrier's floor is less than the validity window end.
On the full merge path, the highest parent can be at most 256 blocks above that floor.
The carrier can sit one block above that parent.
The [merge code](../../casper/src/rust/util/rholang/interpreter_util.rs) also has fast paths that bypass the merge cap.
The signature probe checks the entire indexed range, without a height limit.

## History coverage

The first possible carrier height is `max(0, valid_after + 1)`.
The restore anchor must precede that height on a restored node.
The [carrier index](repeat-deploy-carrier-index-equivalence.md) must cover the height.
The index records each carrier before the DAG publishes its block, including held equivocations.
Equivocation validation can add a lifecycle record later.

The prune cutoff is inclusive for retained rows.
The node checks the cutoff before and after the signature probe because pruning can run during the query.
The finalizer retains carrier entries for two deploy lifespan windows plus the reconciliation horizon.
This allows an expiry query after the bound passes when the basis lags the adopted last finalized block.
The cutoff still marks a gap if a previous prune removed needed rows.

## Verification

The [binding record](../cbc-evidence/deploy-reconciliation-pr-1180.md) covers the API, the expiry calculation, and the revised cutoff.
The history claim relies on the index completeness contract and the ordering of its cutoff publication.
The construction-tier result for terminality across all admissible histories and maintainer acceptance are pending.
