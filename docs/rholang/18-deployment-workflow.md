# Deployment workflow

This guide covers submission, proposal, replay, finalization, and result
inspection for a wallet-funded Rholang deployment. For the preceding custody
steps—creating a wallet purse, depositing REV, funding a persistent process
purse, refilling it, and delegating a lollipop capability—start with
[Wallet-funded process lifecycle](20-wallet-funded-processes.md).

## Before submitting

A deploy does not carry a client-selected phlogiston limit, phlogiston price,
or escrow. Its verified signer and any authenticated located capabilities name
the payer lanes. Validators derive a finite compute, quantitative-byte, and fee
bound from the merged pre-state. The relevant wallet or process purses must
therefore contain sufficient unreserved REV before the deploy's admission
boundary.

Keep private keys outside source files, shell history, logs, and API payloads.
The node accepts a private key only in the local CLI so that it can construct
and sign the canonical deploy. A remote HTTP client must submit the public key
and signature, never the private key.

## Lifecycle

1. Write and locally review the Rholang source.
2. Fund the wallet or persistent process purse that will pay.
3. Canonically serialize and sign the deploy envelope.
4. Submit it to a validator's pending-deploy pool.
5. Let a validator assemble a state-bound, fully funded candidate set.
6. Include the retained deploy, authority certificate, execution witness, and
   state roots in a proposed block.
7. Let every validator independently replay the same causal evidence and exact
   settlement.
8. Wait for the deploy effect—not merely an arbitrary containing block—to
   become canonical and finalized.
9. Query the result, resulting application state, and remaining purse balance.

## Node CLI

Generate a password-encrypted secp256k1 key pair into a controlled directory.
The command creates `rnode.key`, `rnode.pub.pem`, and `rnode.pub.hex` there:

```bash
cargo run -p node -- keygen ./keys
```

Submit a contract through a node's gRPC endpoint:

```bash
cargo run -p node -- \
  --grpc-host localhost \
  --grpc-port 40401 \
  deploy \
  --valid-after-block-number 10 \
  --private-key "$PRIVATE_KEY" \
  --shard-id root \
  contract.rho
```

Use `--private-key-path rnode.key` instead of `--private-key` for an encrypted
key file. The command requires exactly one key source.

The deploy subcommand deliberately has no `--phlo-limit` or `--phlo-price`
flags. Use the wallet and process-purse funding workflow to change available
capacity.

Force a proposal when the target network does not auto-propose:

```bash
cargo run -p node -- --grpc-host localhost --grpc-port 40401 propose
```

Find the containing block and inspect finality:

```bash
cargo run -p node -- --grpc-host localhost --grpc-port 40401 \
  find-deploy "$DEPLOY_ID"

cargo run -p node -- --grpc-host localhost --grpc-port 40401 \
  is-finalized "$BLOCK_HASH"
```

## HTTP submission

`POST /api/deploy` accepts an already signed envelope on a validator. The
signature must cover the node's canonical deploy preimage; hand-assembling a
JSON object and signing the displayed text is incorrect. Use `pyf1r3fly` or an
equivalent protocol-aware client.

The request shape is:

```json
{
  "data": {
    "term": "new stdout(`rho:io:stdout`) in { stdout!(42) }",
    "language": "rholang",
    "timestamp": 1700000000000,
    "validAfterBlockNumber": 10,
    "shardId": "root",
    "authorityPresentations": []
  },
  "deployer": "04...",
  "signature": "3044...",
  "sigAlgorithm": "secp256k1",
  "cosigners": []
}
```

For a multi-signature envelope, all signers authenticate the same canonical
message. Signers are ordered by raw public-key bytes, duplicate keys are
rejected, and empty threshold placeholders never become payer identities.

## Querying deploy status

Use the deploy-specific finalization endpoint for canonical effect status:

```bash
curl "http://localhost:40403/api/deploy-finalization-status/$DEPLOY_ID"
```

Use the deploy lookup endpoint for execution details:

```bash
curl "http://localhost:40403/api/deploy/$DEPLOY_ID"
curl "http://localhost:40403/api/deploy/$DEPLOY_ID?view=summary"
```

The summary response contains the deploy and block identifiers, block number,
timestamp, scalar `cost`, error flag, canonical finalization state, and
rejection count. The full view additionally exposes the deployer, term,
signature algorithm, valid-after height, system error, and extracted transfers
when available. It does not contain retired `phloPrice` or `phloLimit` fields.

`cost` is:

```math
\text{committed COMM count}+\text{canonical RSpace byte cost}.
```

It is not the physical REV debit. Compound and located authority can consume a
different number of physical cells, and proposer fees are separate. The gRPC
`DeployInfo` carries the protocol-v8 authority certificate and witness,
adjacent roots, and admission status; the HTTP deploy response is only the
scalar projection. See [Cost-accounted Rholang](13-cost-model.md).

## Querying resulting state

An exploratory deploy reads state without creating a block. Send this request
to a read-only node. A validator without dev mode returns `readonly_node_required`.

```bash
curl -X POST http://localhost:40403/api/explore-deploy \
  -H 'Content-Type: application/json' \
  -d '{"term":"new ret(`rho:io:stdout`) in { ret!(42) }"}'
```

An exploratory result is not consensus evidence and does not spend a user
purse. For a historical query, use `/api/explore-deploy-by-block-hash` or the
gRPC data-at-name service against a specific block state.

## Refilling and reusing custody

Wallets and process purses persist across deploys. An authorized transfer can
credit the same public deposit address while other processes execute. That
credit cannot expand a certificate already frozen against an earlier pre-state;
it becomes available to the next canonical admission that observes it.

Unused certified maximum remains in the purse after exact settlement. There is
no separately minted refund. A later deploy can reuse the remaining balance,
top it up, transfer it under the appropriate draw capability, or fund another
process slot. See [Vaults and Tokens](12-vaults-and-tokens.md) for the contract
API and [Wallet-funded process lifecycle](20-wallet-funded-processes.md) for the
complete custody flow.

## Failure interpretation

| Observation | Meaning |
| --- | --- |
| Signature or envelope rejection | Authentication failed before user execution; no cost evidence or settlement may publish |
| Insufficient authority | The authenticated pre-state cannot fund the complete physical, byte, and fee bound |
| Out of authority or bytes during candidate execution | The candidate is rejected and its speculative state cannot enter block evidence |
| Replay evidence mismatch | The proposed block is objectively invalid |
| Local unknown root or unavailable storage | Local validation/recovery failure; never evidence for slashing a peer |
| Containing block finalized but deploy effect rejected | Query the deploy-specific finalization state; block finality alone is not effect finality |

## Related interfaces

- `DeployService.doDeploy` submits a signed deploy.
- `DeployService.findDeploy` locates it.
- `ProposeService.propose` requests a proposal.
- `DeployService.getDataAtName` queries canonical state.
- `/ws/events` streams block-created, block-added, and block-finalized events.

For endpoint schemas and status codes, see the [Node API reference](../node/api-reference.md).
