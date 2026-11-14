# FX-8 decisions: the FX fixes merged onto the P3 line

Branch `patch/FX-8`, lane A2, cut from `patch/P3-06b` (902d76197). Lane A1 re-audits the merge commits (F30).

## Merges

| Order | Branch | Head | Conflicts |
|---|---|---|---|
| 1 | `patch/FX-1` | 8886c8c9a | None |
| 2 | `patch/FX-3` | d3c5d383a | `executor.rs`, `gated_executor_spec.rs` |
| 3 | `patch/FX-4` | 3c68f8b2d | `gated_executor_spec.rs` |
| 4 | `patch/FX-5` | c0d48af7f | `reduce.rs` (F30), `region_spec.rs`, `gated_executor_spec.rs` |

FX-5 contains FX-6 (a978756ca), which contains FX-2 (4aed4d852). FX-3 and FX-4 bring lane B's P3-02b, P3-04 and P3-05 through their own history.

Not merged: lane B's `patch/P3-01` and lane C's region-aware refunds (`patch/FX-2-refunds`). Neither is in the ruling. The refunds merge after lane A1 re-audits this branch.

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | Base: `patch/P3-06b`, the newest phase-3 head (P3-07b, P3-06, Decision 65). | `patch/P3-05`. | P3-06b holds the executor that the FX fixes run in. P3-05 comes in through FX-3 and FX-4. | Yes |
| 2 | `executor.rs` (FX-3): both sides kept. Lane B's `overdrawn` outcome and this lane's footprints, timings and hot-store changes are independent fields. The record entry takes the overdrawn flag and the footprint. | — | The two sides add different things. | Yes |
| 3 | `check_region_signature` (F30): FX-4's hash-seal check first. The signature and the body are substituted once, and `hash_seal_admissible` runs on the closed forms (F18) with the runner-context rule (F20). Then FX-2's origin check: `region_signers` gives the origin's signers (F17) and refuses a seal inside data received at run time (FX-6). Each other seal must pass P3-05's `held_or_signed`, so a held unforgeable name still counts. The function returns FX-2's owner and kind. | Keep FX-2's side, as an automatic resolution would. | Lane A1's F30: FX-2's side reopens F18 and F20. FX-5's `admissible` would refuse an unforgeable seal that P3-05 admits. | Yes |
| 4 | Test files: each side kept whole where the other side only added tests, and the other side's new tests appended. In `gated_executor_spec` and `region_spec`, this lane's change from a run's post-root to `mid_root` (P3-06 step 3) is applied to the incoming tests. | — | The FX branches only added tests to these files, and no shared test changed. | Yes |
| 5 | `conservation.rs` (semantic conflict, no textual one): the wallet index adds the process wallet of a `Source::Share` transfer (P3-04), as it does the payer of `Source::Wallet`. | — | A refused share moves REV out of the process wallet. Without the case, the merge does not compile. | Yes |
| 6 | `rholang/tests/slot_spec.rs` (P3-05, semantic conflict): its `DeployData` gets `phlo_price: 0`, the field that FX-2 added. | — | The test does not use a price, and the other specs use 0 where the price is not under test. | Yes |

## Tests added on this branch

- `rholang/tests/hash_seal_spec.rs`: FX-4's F18 and F20 cases run again with seal records on, the path that FX-2's origin check shares with the hash-seal check:
  - `with_seal_records_a_hash_seal_is_still_checked_on_the_substituted_process` (F18)
  - `with_seal_records_each_instance_of_a_hash_sealed_template_still_has_its_own_wallet` (F18)
  - `with_seal_records_a_hash_sealed_body_still_may_not_read_the_running_deployment` (F20)
- `casper/tests/v6/region_spec.rs`: `a_sealed_stored_receive_that_sends_the_same_datum_on_each_firing_keeps_merging`, the sealed form of lane C's finding (FX-2 log, observation 3), fixed in P3-06b.

## Check of the F30 resolution

For this check only, the merged `check_region_signature` was replaced with FX-5's version (FX-2's side, unsubstituted `seals_itself`, no runner-context rule), and `hash_seal_spec` was run. Four tests fail:

- FX-4's `a_hash_seal_is_checked_on_the_substituted_process` (F18) and `a_hash_sealed_body_may_not_read_the_running_deployment` (F20)
- this branch's two record-mode versions of them.

So F30's statement that neither branch's tests fail on that resolution holds only for FX-2's tests. FX-4's two tests catch it, with records off. The new tests catch it with records on, the mode of a v6 shard. The third new test (one wallet for each instance) passes on both versions: it guards the wallet key, which both sides take from the substituted form, and not the check. With the merged function restored, all 12 tests in `hash_seal_spec` pass.

## Verification on the merge (release, stable 1.97 with `RUSTC_BOOTSTRAP=1`)

- rholang `hash_seal_spec` 12, `seal_record_spec` 10, `signed_region_spec` 13, `slot_spec` 3, `storage_left_in_space_spec` 5: all pass.
- casper `v6::`: 64 passed, 0 failed, 7 ignored (after the seal syntax of the new region test was corrected).
- Full suites at be79dc7c6: rholang lib 1,444 passed; casper lib 847; casper `--test mod` 1,237 passed, 0 failed, 20 ignored; node 466. All 0 failed.
- A first run of `casper --test mod` failed many tests when the disk filled with LMDB directories that earlier test runs left in `/tmp` (23 GB). After they were removed, the rerun passed.
