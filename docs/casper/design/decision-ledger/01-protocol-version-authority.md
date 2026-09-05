# D-01 Protocol-Version Authority and Activation

**Status.** Proposed. Pending maintainer ratification.

**Kind.** Protocol.

**Sources.**

- dev [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 10 and [`formal/tlaplus/deploy_recovery/`](../../../../formal/tlaplus/deploy_recovery).
- PR #216 DR-34, DR-47, `finalized-floor-specification.md` section 5.2, `CONSENSUS_PROTOCOL.md` "Protocol-Version Authority", `ProtocolVersionLifecycle.tla`, `ProtocolVersionLifecycle.v`.

## 1. Question

Does the Casper protocol have one version authority, and does a new protocol version activate only through a fresh genesis?

## 2. Position on dev

The protocol document lists genesis-locked parameters: the fault-tolerance threshold, the synchrony-constraint threshold, and the native token metadata. Any change needs a new genesis. No normative rule names a protocol version, a supported version set, or a version authority chain.

Two models exist on `dev` without a normative rule. Both files are present on `dev` and on this branch under the linked directory. PR #216 modifies both. `ProtocolActivationCoherence.tla` models the migration boundary between legacy and exact-occurrence records. `ProtocolVersionLifecycle.tla` models ceremony, approval, adoption, proposal, and reception with protocol 3 as the current version. Neither model is in the CI gate list.

## 3. Position on PR #216

Section 5.2 of the finalized-floor specification states six rules.

- **R-GENESIS-VERSION.** The ceremony master writes the configured version into the genesis candidate. Each approver compares it with its own configured version before signing.
- **R-APPROVED-SUPPORT.** Approved-block validation rejects every version outside the binary's supported set. The supported set is exactly protocol 6.
- **R-VERSION-ADOPTION.** Every node adopts the approved version into the running shard configuration.
- **R-VERSION-PROPOSAL.** Every proposal carries the running version. No compile-time default bypasses it.
- **R-VERSION-RECEPTION.** Peer-interest filtering and validation compare against the running version and no second source.
- **R-FRESH-GENESIS.** Protocol 6 activates through a fresh genesis. No node-local switch, A/B mode, or block-height window exists.

The protocol document assigns meaning to each version. Version 2 introduces the exact rejected-deploy format. Version 3 adds per-execution state-effect provenance. Version 4 adds vault-backed byte evidence.

Version 5 adds certified validator incarnation identity. Version 6 adds signed finalized-floor commitments with certificate sidecars. Versions 1 to 5 remain recognizable but a historical approved genesis is rejected before Casper starts.

DR-34 records the motivating defect: the ceremony hard-coded version 1 while proposal used version 2, so honest validators discarded each other's blocks. DR-34 rejects a block-height switch because it contradicts fresh-genesis deployment and makes replay depend on a removed engine. Genesis-locked parameters gain `max-cosigners-per-deploy`, `initial-phlogiston`, `epoch-phlogiston`, and `client-fuel-allocations`.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Version authority rule | None | One chain from ceremony to reception |
| Supported set | Not stated | Exactly protocol 6 |
| Activation | New genesis for locked parameters only | New genesis for every protocol version |
| Model gating | Models exist, not gated | `MC_ProtocolVersionLifecycle` and two rejected-version configurations gated |
| Decision record | None | DR-34 says protocol 3. Its historical note says DR-47 moved the value to 4. The specification says 6. |

## 5. Options

- **A. Adopt section 5.2 as written.** The supported set is a specification constant. Every future version change edits the specification and requires a fresh genesis.
- **B. Adopt the authority chain and keep the supported set as a release decision.** Rules R-GENESIS-VERSION to R-VERSION-RECEPTION become normative. R-FRESH-GENESIS and the supported set become a deployment decision with its own ratification row per version.
- **C. Reject fresh-genesis-only activation.** Require a height-activated upgrade path. DR-34 rejects this option because historical replay would need the removed engine.

## 6. Unification proposal

Adopt option B.

The authority chain follows principle P2. The running version derives from the approved block, which is on-chain data every validator sees. A second local source recreates the fork that DR-34 describes.

The supported set and the activation mode are deployment facts. They belong in a dated row per version, not in a rule that changes with each release. The first such row would state that protocol 6 is the sole supported version and activates through a fresh genesis.

## 7. Ratification checklist

- Confirm the protocol number. The DR file, the protocol document, and the specification must agree before the row flips.
- Confirm the operator consequence. Every running shard needs a new genesis to move to protocol 6. The row must say so.
- Add `MC_ProtocolVersionLifecycle` and its two rejected-version configurations to the TLA+ gate list on `dev` when the rules land.
- After ratification, edit the Consensus Protocol section 1 and the finalized-floor specification. Add the version meanings to the Casper glossary.

## 8. Open questions

1. Which version number is current on PR #216 at ratification time? The three sources disagree.
2. Does the read-only observer role need version adoption rules, or does it inherit them from approved-block validation?
