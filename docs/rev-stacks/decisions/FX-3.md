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

## The deployer names its cosigners (Decision 63, A-7 F27)

Second commit on `patch/FX-3`.

### Finding

The deployer's signature did not cover the cosignature set. A relay or a
proposer could remove cosignatures. The envelope kept its identity (the
deployer's signature), the gate then refused it for a foreign signature, and
the complete envelope became a duplicate (A-7 F27).

### Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 6 | `DeployData` gains `required_cosigners` (proto field 16, `requiredCosigners`, JSON `requiredCosigners`): the public keys of the cosigners, in order. It is part of the message that the deployer signs, so the cosignature preimage covers it too. An empty list keeps the old message bytes. | Sign over the cosignatures. | Decision 63. The set is fixed before anyone cosigns, so cosigners still sign in any order. | Until a shard holds cosigned envelopes |
| 7 | `Cosignature::missing_cosigners` checks each cosignature (it verifies, its key is named, no key signs twice) and the list itself (no key twice, not the deployer), and returns the named keys that have not signed. `verified_cosigners` returns the named keys, in the deployer's order, only when none is missing. | — | One check for the gate, the decoder and the command. | Yes |
| 8 | `rho:system:cosignerId:<i>` is the i-th key that the deployer named. Before, it was the i-th cosignature in the envelope, which a relay could reorder. | — | The deployer fixes which party each index names. | Yes |
| 9 | `DeployData::from_proto` and the web API refuse an envelope whose named cosigners have not all signed (`with_complete_cosignatures`). A stripped envelope therefore does not decode, does not enter the pool and does not consume the identity. A block that holds one does not decode. The gate still refuses one with `InvalidTerms`. | Refuse at the gate only. | A-7 F27 test plan: a stripped envelope is malformed, and the complete one is admitted later. | Yes |
| 10 | `node cosign prepare` takes `--cosigner <public key>` (repeated, in order). `sign` decodes a partial envelope (the deployer's signature is checked, the cosignatures may be incomplete) and accepts only a named key that has not signed. `submit` needs every named cosignature. | — | The command follows the new format. | Yes |
| 11 | The deployer's signature has one encoding. `Secp256k1::verify` (k256 `verify_prehash` on strict DER) refuses a high-S value, a padded integer, a long-form length and a trailing byte. `secp256k1:eth` takes exactly 64 bytes `r ‖ s`, which map to one DER form, so a high-S value is refused there too. No code change was needed. | Normalize S before verifying. | Decision 63: confirm that one deploy has one identity. | — |

### Tests

- `casper_message` unit tests: `a_stripped_envelope_does_not_decode_and_the_complete_one_does`, `the_deployers_signature_covers_the_named_cosigners` (removing or adding a named key breaks the deployer's signature, and an unnamed cosignature is refused), `cosigners_follow_the_order_the_deployer_named`. The four earlier tests now name their cosigner.
- `secp256k1::tests::a_second_encoding_of_one_signature_does_not_verify`: the high-S, padded, long-form and trailing-byte encodings of a valid signature are refused, for `secp256k1` and for `secp256k1:eth`.
- `deploy_runtime::envelope_tests`: a prepared envelope that names a cosigner does not decode until the cosigner signs; only a named key signs, and only once; the deployer cannot name itself.
- `options`: `--cosigner` parses, repeated and in order.
- `casper/tests/v6/region_spec.rs`, `a_stripped_envelope_does_not_decode_and_the_complete_envelope_runs_after_it` (the A-7 F27 plan): the stripped envelope does not decode, and the complete envelope with the same identity decodes and moves 700 from Bob to Carol. A stripped envelope that reaches the executor directly is refused with `InvalidTerms`.
