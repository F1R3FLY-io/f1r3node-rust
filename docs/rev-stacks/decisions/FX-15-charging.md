# FX-15 charging: lane C's meter on FX-8b, the caps, and F39 (lane B)

Decisions 83, 86, 90, 93. Plan r8.16. Branch `patch/FX-15-charging`, cut from lane C's `patch/FX-15` (fc3a6ca72), with `patch/FX-8b` (4a74b0e34) merged in.

Lane A2's `patch/FX-15-replay` and lane B's `patch/FX-15-settle` are not merged here. Their replay stop (`ReplayStops` in `CostManager` and `ChargingRSpace`) and lane C's replay in `MembraneMeter` overlap, and the merge conflicts in four files. That merge belongs to the FX-8b integration.

## 1. The meter meets FX-8b

Lane C cut `patch/FX-15` before FX-8b took FX-12's client limit (eab30238f). The merge has two conflicts.

- `CostManager::charge`: lane C's version is kept. A gated record (an owner that the gate did not commit, Decision 93) is admitted step by step against its wallet and does not use up the client's limit (Decision 90). Any other charge uses up the client's limit.
- **`FOREIGN_WORK_BUDGET` is removed** in this merge commit, with `set_own_origin`, the `own_origin` and `foreign` fields, and FX-12's unit test `own_limit_tests`. Gated work is bounded by its wallet. Ungated work, including foreign work of an owner that the gate committed, now uses up the client's limit. FX-12 had kept that work off the limit and gave it the constant bound.
- `region_spec`: lane C's assertion message is kept.

## 2. F39: a region whose owner the gate did not commit (Decision 83)

**Before:** `settle::own_payers` charged an own key to the client when the gate had not committed its owner. A seal that resolves at run time to another wallet (F33) therefore charged that region's cells and work to the deployer.

**Now:**
- `own_payers` returns `None` for such a key. A key with no record, or with an owner that names no wallet, still goes to the client (Decision 81).
- `charges` skips that key. It does not use the client's budget.
- `surface_charges` meters it with the foreign surfaces (`gated_own`). It is drawn from the owner's purse (`surface_purse`) at its origin price, like any surface draw. A compound owner whose components the gate committed is still charged to the components (Decision 80).
- This matches lane C's meter, which gates the same records (`EnvelopeFunding::gates`).

**Test:** `settlement_spec::an_own_region_whose_owner_the_gate_did_not_commit_is_charged_to_that_owner`. Alice's envelope has 2 cells of its own and a region owned by Carol, which the gate did not commit (4 cells, 10 work, price 3). Alice pays 6. Carol pays 42 as a surface draw.

An end-to-end test needs a seal that the gate cannot resolve and that resolves at run time to a wallet that passes the region check. Three term shapes (a variable seal over a plain body, over a body with a stack, and over the cosigner test's stack) were all refused by the gate (`ForeignSignature`). The settlement test covers the charging rule. The run-time path waits for a term shape from lane A1's F33 reproducer.

## 3. The caps: removed, and two kept with a blocking finding

| Cap | State | Reason |
|---|---|---|
| `FOREIGN_WORK_BUDGET` | **Removed** | The meter bounds gated work. Ungated work uses up the client's limit. |
| Surface draw in `surface_charges` (`owed.min(free)`, D76 decision 7) | **Kept** | Finding below. Without the cap, an honest block fails to settle. |
| Close draw in `charge_recorded_work` (`owed.min(balance)`, FX-12 D8) | **Kept** | The close has no meter yet. Lane C's `CloseFunding` exists, but lane A2 has not installed it in `compute_state`. Without the meter, an underfunded installer's continuation fired in the close would make the close fail. Remove the cap in the commit that installs `CloseFunding`. |

**Finding — blocking: two runs can together overdraw a gated wallet.**
- **Cause:** the meter admits each run against the whole free balance of the wallet (`EnvelopeFunding::limit` = pre-state balance less the gate's holds). Runs execute in parallel, and no run sees what another run drew. `CloseFunding` does subtract the runs' draws for the close, but nothing does that between runs.
- **Evidence:** `region_spec::two_runs_that_each_fit_a_gated_wallet_but_not_together_pay_in_full` (ignored). A process wallet holds about 55,000. Two calls to its service each draw a gross 47,000 to 50,000, so each is admitted. Their net work is 29,542 each, 59,084 together.
  - With the cap removed, settlement fails: "a surface draw of 29542 exceeds the 25743 that its wallet can pay".
  - With the cap, the second draw is cut to 25,743, so 3,799 phlo of work under the membrane runs unpaid.
- **Label:** blocking. With the cap, work under t runs unpaid, which diverges from r12 §6.3. Without the cap, an ordered block cannot execute, which is a validator stall (High).
- **Proposed resolution (needs a ruling, lane C):**
  1. Decision 48's rule for gated draws: in canonical envelope order, a run whose gated draws pass what earlier runs left on the wallet becomes `Overdrawn` and does not merge. Settlement then never falls short.
  2. Or split each gated wallet's free balance between the admitted runs that can reach it. This needs the set of runs in advance and is not recommended.

## 4. The underfunded installer test

Lane C's `region_spec::work_under_an_underfunded_installer_stops_and_the_client_pays_nothing_for_it` is the test that this item asked for: the installer's continuation stops, the client's run does not fail, and the client pays what it pays when the service is fully funded. It passes on this branch.

## 5. Test change

`forms_spec::work_that_stored_code_pays_for_does_not_count_against_the_clients_limit` (FX-12) installed a continuation with 40 nested COMMs. Under the meter, its work passed the installer's wallet, so the installer stopped. The test's helper replays without the logged refusals, which lane A2's deploy-log event will carry. The continuation now has 12 nested COMMs. Its work still passes the client's limit of 1,000, which is what the test checks.

## Verification (release, stable with `RUSTC_BOOTSTRAP=1`)

- casper `v6::`: 90 passed, 0 failed, 8 ignored.
- casper `v6::region_spec::two_runs_…` (ignored): fails as the finding says.
- casper `genesis::`: 118 passed. casper `util::`: 134 passed. casper lib: 852 passed.
- rholang: 2,457 passed, 70 ignored. node lib: 427 passed.
- `cargo fmt --check`: not verified.

## Revision 2: Decision 103, the surface cap removed, and the F39 negative test

Lane C's e30313108 (Decisions 103 and 104) is merged in. It keeps one ledger per block: one gross drawn amount and one stop for each wallet, shared by all runs of the block, in play and replay. The overdraw finding of revision 1 is answered by Decision 103. Decision 48's rule is not adopted.

- **The surface cap is removed.** `surface_charges` draws the full amount owed. It fails only when a draw exceeds what the wallet can pay. Under one ledger per block, the net draws of a block are at most the gross drawn amount, which the meter keeps within the wallet's free balance. So an honest block never reaches that failure.
- `region_spec::two_runs_that_each_fit_a_gated_wallet_but_not_together_pay_in_full` is enabled and passes. The second run stops on the shared ledger, and every surface draw is paid in full.
- **The close cap stays.** `CloseFunding` is still not installed in the close (lane A2), so a stored continuation fired in the close is not gated yet.
- **F39, the run-time path:** `region_spec::the_gate_refuses_a_seal_that_resolves_only_at_run_time` asserts that the gate refuses (`ForeignSignature`) the three term shapes that revision 1 tried. If lane A2 confirms that the gate refuses every seal that resolves only at run time, this test closes F39's remaining path and F33. Until then it records what the gate does for these shapes.
- F48 (lane C): substitution into a fired body is charged to its installer.
- Verification (release): casper `v6::` 93 passed, 0 failed, 7 ignored. `genesis::` 118 passed. `util::` 134 passed. casper lib 852 passed. rholang 2,458 passed. node lib 427 passed. `cargo fmt --check` not verified.
