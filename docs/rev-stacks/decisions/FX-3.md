# FX-3 decisions: a cosignature covers the whole envelope

Fix FX-3 (A-6 finding F4, Decision 53), rev-stacks patch plan r8.3, lane C.
Branch `patch/FX-3`, cut from `patch/P3-04` (8095c53b0), with
`patch/P3-02b` (1186cee1d, the `node cosign` command) merged in, because
the command signs and checks cosignatures.

## Finding

A cosignature signed `DeployData::to_message()` only. That message leaves out
the deployer's public key and the deployer's signature. A party that saw a
cosigned envelope could sign the same data as deployer, attach the
cosignature, and get the cosigner's authority in its own envelope (A-6 F4).

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | A cosigner signs `Cosignature::preimage(envelope)`: `"f1r3fly/cosignature/v1"`, then four length-prefixed parts (8-byte big-endian lengths): the deploy data message, the deployer's public key, the deployer's signature, and the name of the deployer's signature algorithm. The hash is `Signed::signature_hash` of the cosigner's algorithm, as before. | The hash of the whole signed envelope less the cosignatures. | Decision 53 names the three parts. The length prefixes make the parts unambiguous. The domain keeps a cosignature apart from every other signature. | Until a shard holds cosigned envelopes |
| 2 | The preimage leaves out the other cosignatures. | Sign over earlier cosignatures. | Every cosigner signs the same preimage, so cosigners can sign in any order and in parallel. A cosignature cannot be lifted, because the deployer's signature already fixes the envelope. | Yes |
| 3 | `Cosignature::sign`, `verify` and `verified_cosigners` take the signed envelope (`Signed<DeployData>`), not the deploy data. Every caller (the gate, `normalizer_env_from_deploy`, the rholang `DeployData::from_deploy`, `node cosign`) passes the envelope. | Keep the data-only functions beside the new ones. | No caller can then check a cosignature against data alone. | Yes |
| 4 | `node cosign sign` is unchanged in use: `prepare` signs as the deployer first, so every cosigner signs an envelope that already holds the deployer's signature. | — | The order already matched the new rule. | Yes |
| 5 | A lifted cosignature makes the envelope's cosignatures invalid, and the gate refuses it with `InvalidTerms` (P3-02). The cosigner's wallet moves only for the envelope it signed. | Drop the bad cosignature and admit the rest. | A partly valid set of signers would change who pays. | Yes |

## Tests

- `casper_message` unit tests: `a_cosignature_signs_the_deployers_preimage_and_survives_the_proto`, `a_tampered_cosignature_does_not_verify`, `a_cosignature_does_not_verify_under_another_deployer`, `a_cosignature_covers_the_deployers_signature`.
- `deploy_runtime::envelope_tests` (3): `node cosign` prepares, cosigns and decodes with the new preimage.
- `casper/tests/v6/region_spec.rs`: `a_lifted_cosignature_carries_no_authority_under_another_deployer`, the A-6 F4 test plan. Alice's envelope, cosigned by Bob, moves 700 from Bob to Carol. Mallory signs the same data as deployer and attaches Bob's cosignature. In one block, Alice's envelope is admitted and Mallory's is refused (`InvalidTerms`). Carol receives 700 once, Bob pays 700 once, and the block replays to the same settled root.
