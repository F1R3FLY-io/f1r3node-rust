# FX-15 follow-ups (lane C): F33 and F39 at the gate, the FX-3 store review, void blocks, and the known issues

Branch `patch/FX-15-followups`, cut from `patch/FX-8b` (7b60c3880). Design r21, plan r8.23. Names: V1…Vn validators, u1…um deployers, d1…di deployments, B1…Bj blocks, void reasons (a)–(d).

## 1. F33 and F39: the gate refuses every seal whose owner resolves only at run time

**Result: the guarantee holds. No term shape gets past the gate.**

The check is `GateLedger::decide` (`v6/gate.rs`): `input.named.iter().any(|s| !admissible(s, &signers))` refuses the deployment with `ForeignSignature`. `input.named` is `region::named_owners(term)`, applied by `executor::classify_envelope` to the term that the runtime also evaluates.

Why no shape gets past it:

1. **Every seal is named.** `named_owners` visits every place where a `Par` can hold a signed term: the bodies of signed terms, sends (channel and data), receives (bind sources and bodies, with the bind-level signature), `new` bodies, `match` targets and case bodies, bundles, conditionals, connectives, and every expression that holds a `Par` (all 26 `Expr` variants that have a `Par` field, including lists, tuples, sets, maps, pathmaps, zippers and method arguments). Each signature of each signed term goes to `name_signature`. A stack seal goes through `stack_payer`, so the payer of a kind stack is named too.
2. **A seal that the gate cannot resolve is never admissible.** `static_owner` resolves a seal only when the seal is a de Bruijn reference to a `new` binder whose URI injects a deployer identifier (`rho:system:deployerId`, `rho:system:cosignerId:N`), or a literal deployer identifier, or a compound of these. Every other binder (a fresh name, a receive variable, a `match` variable, a contract parameter) is in scope as `None`. An unresolved seal is named `WalletOwner::Unforgeable(signature bytes)`. `admissible` compares it with the signers. The signers are the deployer and the verified cosigners, which are all key-derived owners, so the comparison always fails.
3. **The gate reads what the runtime runs.** The gate and the runtime use the same `normalizer_env_from_deploy` and the same compiler. A term that the gate cannot read (`Unnormalized`) cannot run either. That is void reason (c), not a seal.
4. **Patterns do not run.** A signed term in a pattern is refused by the normalizer ("signed terms … cannot appear in pattern position"). A receive or a `match` binds data, never a seal from its pattern.
5. **Seals in received data.** A seal that arrives as data was in the term of its sender, and the gate checked it against the sender's signers. Under Decision 77, the receiver's membrane runs it. Decision 83 (F39) covers a seal that resolves at run time to a wallet that the gate did not commit, and FX-15's meter gates it.

**Test:** `region_spec::the_gate_refuses_every_seal_whose_owner_resolves_only_at_run_time`. It runs 34 term shapes through the executor:
- 13 shapes must be admitted and must run;
- 21 shapes must be refused with `ForeignSignature`.

The shapes cover:
- the order of fresh names and URI names in `new`;
- shadowing;
- receives with several binds and channels;
- persistent receives and peeks;
- `match`;
- contracts;
- seals in data, lists, maps, tuples, method arguments, `if`, and quoted channels;
- compound seals and nested seals.

Lane B's three shapes are cases of the variable seal in this table. During the work, three candidate shapes were found not to parse. The gate admits such a term as `Unnormalized`, and the run fails. They are reason (c), so they are not in the table.

**F33 and F39 can close.** Lane A1's general guarantee is items 1 to 4.

## 2. FX-3 after the `dev` merge (for lane A1)

**Result: no path reads a deployment back as valid when it lacks a named cosigner, within the store's trust model. A second encoding of a signature is still refused.**

| Path | Check |
|---|---|
| Write (`add`, `prepare`, `prepare_checked`, `add_prepared_if_absent`) | `deploy_codec::encode_checked` → `verify_signature`: `Signed::verify`, then `DeployData::with_complete_cosignatures`. |
| Read of a current (`F1R3DEP\x02`) record | Fingerprint only. The record was written by `encode_checked` or by the upgrade below. |
| Read of an older (`\x01` or legacy) record | `DeployRecordQuarantine::decode_verified` → `verify_signature`, then the record is upgraded. A record that fails goes to the quarantine. |
| Restore from the quarantine | `decode_verified`, as for a read. |
| Network input | `DeployData::from_proto`: signature, then `with_complete_cosignatures`. |
| Execution | `GateInput::from_deploy` uses `Cosignature::verified_cosigners`. An incomplete deployment is refused (`InvalidTerms`) whatever path brought it in. The runtime takes origin signers from the same function. |

Second encoding: every path above calls `Signed::verify` or `Cosignature::verify`, which use the algorithm's `verify`. `crypto` `secp256k1::tests::a_second_encoding_of_one_signature_does_not_verify` still passes.

New test: `deploy_record_quarantine::tests::a_deploy_that_lacks_a_named_cosigner_is_not_written_and_not_read_back`, for both pools. The codec refuses to write the deployment. An older-format record of it is not read back, and it goes to the quarantine.

**Known issue (Decision 88), not blocking:** a current-format record is trusted on read. A process that writes the database directly can store an incomplete deployment that the store then returns. The gate still refuses it at execution, so it moves nothing.

## 3. The meter and void blocks (Decision 109)

The meter keeps no state between blocks:
- `EnvelopeExecutor::execute`, `replay` and the close each make a new `BlockLedger` and new meters. They are dropped with the call.
- The runtime's meter slot is cleared after each run.
- The refusals exist only in the run's `metered` and in the deploy logs of that block's record.

So for a void block, the meter needs nothing:
- **Nothing drawn:** a void record applies no settlement.
- **No refusal events kept:** the record keeps no deploy logs.
- **Nothing carries over:** the next block's ledger is new.

**Lane A2's hook** must do two things:
- drop the block's `ExecutedBlock` (with its `metered` and its deploy logs) when the record is void;
- execute the deployments again in the block that runs them, from that block's pre-state.

Nothing else is needed from lane C.

Test: the FX-15 test now runs a later block on the same executor after the stopped one. The later block has no refusals, draws exactly what a fresh block draws, and gives the same root.

## 4. The FX-15 known issues

| Issue | State |
|---|---|
| The veto after the match | **Fixed.** `Matcher::admits_waiting` skips, in play only, a stored receive whose wallet has already stopped. The skip happens before the spatial match. Replay never skips: it follows the log. |
| The fixed identity of an unscoped charge | **Measured, not changed.** `membrane::unscoped_charges()` counts such charges. The FX-15 test asserts that the count is 0 after every region and close test. A real fix would need a step scope at the new site, and no site is known. |
| The compound-wallet tie-break | **Logged, not changed.** The refusal event does not name the binding component, and `accept_metered` takes the first component that fits. A fix changes the event encoding (lane A2). It does not affect authority, because each component is charged in full. |

**New finding, fixed here:** the reservation of Decision 104 depended on which side of a COMM fired. The trigger reserved the follow-up storage. Two executions of one block could therefore draw different gross amounts (147,129 and 136,425 in the test), and replay can fire a COMM from the other side. With a stopped wallet, replay would then refuse an honest block, because the drawn amount must be exact (r14).
- **Change:** a send or a receive now reserves, when it is issued, all the storage that it can cause: its event storage, the re-store of a persistent item, and for a receive the COMM's event storage. A COMM is admitted with its cells only.
- **Close path:** the COMM's event storage is charged to the continuation's record.
- **Label:** this was a divergence from r12 §6.3 (exact replay), so it is **blocking**. It is fixed on this branch.

## Verification (release, stable with `RUSTC_BOOTSTRAP=1`)

- block-storage lib: 173 passed. rholang (lib and tests): 2,463 passed. casper lib: 889 passed. node: 469 passed.
- casper `--test mod`: 1,296 passed, 0 failed, 22 ignored.
- Clippy with the lane's allow list: no warnings.
- **Known issue (Decision 88), not blocking:** the first full run of casper `--test mod` stopped after 221 tests with SIGSEGV in the test process. The log does not name the test, and the second run passed in full. This looks like a stack overflow in a test thread. It needs a run with `--test-threads=1` to find the test.
