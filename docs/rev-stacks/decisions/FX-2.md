# FX-2 decisions: the enclosing-seal record and the stored-origin wrapper

Fix FX-2 (A-6 findings F14, F15, F17), Decisions 51, 52, 56 and 59, rev-stacks
patch plan r8.3, lane C. Branch `patch/FX-2`, cut from `patch/P3-04`
(8095c53b0), with `patch/FX-1` (8886c8c9a) merged in. The merge had one
conflict, in `casper/tests/v6/gated_executor_spec.rs`: both sides added a test
at the end of the file, and both tests are kept.

The API was agreed in substance on 2026-11-01 (the FX-2 API proposal and
Decision 59). Lane B builds region metering on it. Lane A2 reviews the
dispatch-scope change.

## The record and its API

`rholang::rust::interpreter::accounting::rev_stack::seal`:

| Item | Use |
|---|---|
| `SealRecord { owner, kind, origin }` | The innermost enclosing seal. `owner` is the seal's wallet owner, and a compound seal gives a `Compound` owner. |
| `SealKind::{Deployment, Signed, Process}` | The implicit top-level seal, an explicit `{% S %}` region, or a `{% # P %}[P]` region. |
| `Origin { envelope, signers, phlo_price }` | The envelope that admitted the code: its deploy signature, its verified signers (deployer, then cosigners) and its `phloPrice`. |
| `seal::current()`, `seal::current_owner()` | Read the record of the running task. `None` outside a v6 deployment. |
| `seal::of_continuation(&TaggedContinuation)` | Read the record stored on a continuation, for example at the COMM charge in `charging_rspace`. |
| `seal::within`, `seal::inherit`, `SealRecord::deployment`, `SealRecord::enter` | `pub(crate)`. Only the reducer installs a record. |

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | A task-local `SEAL: Option<SealRecord>` holds the record. `spawn_in_dispatch_scope` carries it into every spawned task. | A field on the reducer. | Parallel branches can run in different regions. Lane B asked for this carrier. | Yes |
| 2 | `inj` installs `{owner: deployer, kind: Deployment, origin}` for a deployment. The rholang `DeployData` gains `phlo_price`, which `from_deploy` fills from the envelope. | — | Decision 51: the top level is `{% D %}[deployer]`. | Yes |
| 3 | Records exist only on a runtime with `set_seal_records(true)`. The v6 executor sets it for every run and replay. Another runtime (legacy, genesis, tests that do not set it) installs no record and keeps the P2-19 checks against the deploy data. | Records on every runtime. | Lane A2: the field is absent on legacy shards. Legacy terms do not use cost syntax. | Yes |
| 4 | `TaggedContinuation` gains `SealRecordProto seal = 4`. Only `consume` in the reducer writes it, from `seal::current()`, and only when records are on. The normalizer cannot produce a `TaggedContinuation`, so no term can write the field. | A field on `CostSignedTerm`. | Decision 52: a wrapper that only the reducer can build. | Until a shard holds sealed continuations |
| 5 | The binary form of `TaggedContinuation` keeps its legacy bytes when `seal` is `None`. The old `Option<Par>` tag of `guard` (the first field, because prost puts oneof fields last) becomes a tag byte: bit 0 means a guard follows, bit 1 means a seal follows. Then come the guard, the seal and `tagged_cont`. Unknown bits are refused. | Derived serde. | The derived form adds an `Option` byte to every continuation and changes every root. | Yes |
| 6 | `PartialEq` and `Hash` of `TaggedContinuation` include the seal. `Hash` writes nothing for `None`. | Ignore the seal. | Two continuations with the same body and different authority are different stored code. | Yes |
| 7 | A fired `ParBody` runs under the record stored on its continuation, or under no record when there is none. It never runs under the firing task's record. A deferred dispatch takes the record from the continuation when the dispatch is queued. | Inherit the firing task. | Decision 51: stored code keeps its author's seal. Lane A2's queue-time rule. | Yes |
| 8 | A system-process body (`ScalaBodyRef`) runs inline in the sending task, so `current()` in a native handler is the sender's record. | — | Lane B reads it in `rho:rev:wallet` and `rho:rev:bond`. | Yes |
| 9 | `check_region_signature` checks each non-hash component against the signers of `current().origin`, not of the running deploy data (F17). It returns the owner of the whole signature (a compound is given whole) and the kind (`Process` when every component is a `# P`, else `Signed`). The region body runs under `record.enter(owner, kind)`, with the origin unchanged. | — | Decision 52: a seal is checked against its origin. Decision 59 (2): a `# P` region pays at its installer's price, which is the origin's price. | Yes |
| 10 | A stack draws on `current_owner()` (F14). With records on and no record, a stack is refused. | Default to the deployer. | Decision 51. | Yes |
| 11 | The normalizer folds `{% {% cells %}[k] %}[A]` into one signed term with the compound signature `A (*) k`. A stack body whose signature has exactly one kind element is a stack of that kind. The other elements, if any, are the payer: they pass the region check and the stack draws on their owner. With no other element, the stack draws on the enclosing record (F15). More than one kind element is refused. A mint still checks the payer elements, but deposits from the kind name. | A new signature form. | The A-6 F15 nesting form, as the normalizer gives it. One rule for REV and every kind (P9). | Yes |
| 12 | A sealed region that arrives as data is checked against the origin of the code that evaluates it. | A reducer-built wrapper on data. | Decision 59 (4): the strict reading. | Yes |

## For lane B (region metering)

- Decision 59 (1): work under a `Deployment` record is metered to the firing deployment. Only `Signed` and `Process` records meter to their owner, at `origin.phlo_price`. Authority still follows the record.
- Decision 59 (5): a charge with no record is the client's part.
- Lane A2: the per-region tallies must be the same in replay and sum to the run's COMM count. A COMM is counted in `charging_rspace`, where the matched continuation is at hand, so `seal::of_continuation` attributes each COMM to exactly one record (or to the client), the same in play and replay.

## Tests

- `models` `par_serde`: `a_continuation_without_a_seal_keeps_the_legacy_bytes` (byte-identical to the derived legacy layout, with and without a guard), `a_sealed_continuation_round_trips_and_differs_from_the_unsealed_one` (unknown tag bits refused).
- `rev_stack::seal` unit tests: the proto round trip, and scoping (nested, cleared, inherited by a spawned task, not inherited by a bare spawn).
- `rholang/tests/seal_record_spec.rs`:
  - `a_legacy_runtime_keeps_the_golden_root`: a legacy term with linear, persistent, contract, guarded and join continuations gives the root `e26a2350…` that the base (`e82192f70`) gives.
  - `a_v6_runtime_records_seals_and_changes_the_root`.
  - `a_stored_continuation_carries_its_authors_record` (`Deployment` and `Signed` records, origin envelope, signers and price).
  - `a_stack_pays_from_its_enclosing_seal_not_the_deployer` (F14).
  - `a_region_sealed_in_its_origin_runs_for_a_later_runner_and_pays_from_its_sealer` (F17, the A-6 three-step plan).
  - `a_fired_continuation_does_not_inherit_the_firing_region`.
  - `a_foreign_seal_in_a_later_runners_own_code_is_refused`.
  - `a_kind_transfer_draws_on_the_enclosing_seal` (F15: Bob's bare kind stack draws on Bob, Bob's `{% {% s :: () %}[kind] %}[a]` with Alice's id is refused, Alice's own nested form moves her units).
- `casper/tests/v6/region_spec.rs`, each block replayed to the same settled root:
  - `a_stack_under_a_cosigners_seal_draws_on_the_cosigner_not_the_deployer` (F14 test plan).
  - `a_region_sealed_in_its_origin_pays_from_its_sealer_for_any_later_runner` (F17 test plan, three blocks).
  - `a_fired_continuation_draws_on_its_author_not_on_the_firing_region`.

## Open items

1. The gate's static walk (`region.rs`) does not yet name the payer of the nested kind form, so an envelope that uses it is refused at the gate. This belongs with F19 in FX-5.
2. Lane B: `rho:rev:wallet`, `rho:rev:bond` and the gate's paying wallets read the record (F5, F6), and region metering builds on it.
3. Observed while writing the F17 test: in a three-block run, a persistent continuation whose body sends the same datum (`@"ran"!(1)`) on each firing is not merged in the third block (the v6 merge returns a merge error). The same happens with no seal syntax in the body. It looks independent of FX-2. It is reported to lane A2, and the test uses a body without the repeated send.
