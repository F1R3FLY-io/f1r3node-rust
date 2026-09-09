---
doc_type: work_log
task: pr216-publication-local-handoff
status: in_progress
date: 2026-09-08
---

# Buffer publication: executable ownership evidence

## Current continuation

### September 9: checkpoint preparation and exact refusal reasons

The user requested one descriptive checkpoint commit after a stable local verification point.
`quarantine-result-red.wxm2Z4` reproduced two public API defects before the correction.
Local receipt and local recovery reported `AtCapacity` for inactive quarantine despite free capacity.
Both tests confirmed unchanged budget and provenance before the reporting assertion failed.

Plan review selected a typed result from the existing locked activation decision.
Existing active owners still take precedence. Inactive quarantine still precedes capacity, and storage errors remain errors.
The local API now maps ineligibility to `Quarantined`. Network and dependency-recovery wrappers retain their previous optional-result behavior.
Only capacity refusal increments the capacity metric. Both worker refusal outcomes retain the lease and existing retry delay.

`LocalRequestActivation.tla` passed 58,968 states and six exact unsafe controls in `run.gXQl6e` before production changed.
The model checks the classification boundary and delayed reporting, not subsequent receipt writes or complete worker execution.
Independent plan review found no checkpoint blocker, subject to native and lint results.

The older operation model no longer grants expired probes or adds probe exceptions to the reservation bound.
`run.Ce21VJ` passed 49,804 safe states, seven exact controls, sixteen Rocq theorem reports, and an independent kernel check.
The Rocq module needed no changes because it contained no probe rule.
This remains a single-budget-cycle operation check. Separate models cover retirement, renewal, deferred persistence, and sweep ordering.

`quarantine-result-green.HbZEbx` passed 61 retriever tests, 35 owner tests, 19 worker tests, and strict Casper/node library-and-test Clippy.
Both original reporting regressions pass. The node retains worker leases for capacity and quarantine refusals.
The checkpoint must not be reported as completed CI qualification, upstream protocol ratification, or a completed campaign.

### September 9: upstream assertions and two-stage model correspondence

`retirement-broader.HDfrjw` passed 32 owner tests and 19 worker tests. Its retriever run passed 55 tests and failed three assertions.
The wrapper lost its status variables during systemd expansion. The individual test logs, not the empty status fields, establish these results.
Later commands disable systemd environment expansion and use pipeline failure propagation.

The three failed assertions expected retained ordinary provenance or a retry probe after budget exhaustion.
Pinned dev removes ordinary request state on exhaustion and sweeps expired quarantine after active maintenance.
Its source locations are `block_retriever.rs:1041–1055`, `:1099`, `:1110–1125`, and `:378–404`.
The updated tests require transport retirement, retained quarantine and budget, no autonomous probe, and renewal at the deadline.
Durable pending provenance retains its separate positive coverage. No production code changed during this assertion correction.
`retirement-upstream.Y46dXy` passed all 58 retriever tests with matching captured source hashes.
The earlier strict Clippy run in `retirement-order-green.kcUJnz` also completed successfully.

Plan review identified an overstrong concurrent interpretation of the maintenance model.
The revised model permits same-owner recitation and separates due observation from current-policy application.
Both source branches permit this split. No new retry policy was introduced.
The initial budget now includes available states, so dispatch is reachable before the single expiry pass.
`run.AAVczp` passed 423,328 states at depth 21 and five exact unsafe controls.
Its unsupported-current-due configuration produced the expected counterexample with the safe transition relation.
The gate verified its source hashes and exited successfully.

Further review found that modeled dispatch retained an expired quarantine deadline, unlike production.
The model now clears that deadline on dispatch. `run.As2kQa` passed 425,572 states and the same six expected counterexamples.
The gate exited successfully with matching hashes. This correction changed no production behavior.

The new native correspondence test exercises publication and same-owner recitation with both available and exhausted budgets.
`maintenance-correspondence.fr3em4` passed all 59 retriever tests and strict Casper library-and-test Clippy.
Its captured source hashes matched after both commands. Focused diff checks and proof-script shell syntax checks passed.
Full publication-task qualification remains incomplete. This record does not claim all concurrency or restart obligations have passed.

The plan agent identified two narrower owner-composition gaps. New tests cover both without changing production code.
`failed_retired_budget_publication_preserves_budget_and_competing_row` injects a competing transaction before the retired-budget publication commits.
The conflict preserves both the retired record and the competing row. Successful retry preserves the budget and both captured-provenance cases.
This checks a confirmed conflict, not an unconfirmed storage failure.

`deferred_final_completion_survives_retirement_and_renewal_without_recharge` starts with a reachable reserved final allowance.
A failed completion write retains its observed charge. Retirement commits that charge, and renewal resets the budget.
Deferred drain releases its permit once without restoring the old charge. The test checks drain before and after renewal.
`final_completion_and_retirement_preserve_the_last_charge` races completion against retirement for volatile and durable owners.
It preserves the last charge and then renews without an extra dispatch or permit leak.
These native schedules complement the bounded models. Native thread scheduling alone does not exhaust all interleavings.

`retirement-composition.SXNuk5` passed all 35 owner tests and strict target Clippy with matching source hashes.
The complete publication formal gate passed in `run.B0cBlO`, including all configured TLA+ checks and five Rocq kernel checks.
The gate verified its captured input hashes and exited successfully under a 2 GiB memory limit with no swap.
This passes the existing gate. It does not replace the stated model-to-production correspondence limits.
The gate completed 29 safe TLA+ configurations, 122 expected counterexamples, 68 closed-context theorem reports, and five kernel checks.
One expected counterexample rejects the unsupported current-due claim. It is not an unsafe implementation variant.
The older operation model still permits expired probes. Its policy projection needs alignment with the current no-probe implementation.
`loom-components.8lFal4` passed eight focused tests with no ignored or filtered tests.
These checks cover the production commit helper, receipt helper, and guarded pending publication, including their expected unsafe controls.
They do not model the whole request-owner registry or the validator network.
The public quarantine-refusal diagnostic still needs correction: it can report `AtCapacity` even when active capacity remains available.
The worker retains its lease, so this finding does not demonstrate ownership loss or authorize a changed admission policy.

### September 9: production retirement and renewal integration

The current integration adds budget-only retired records and preserves pending policy separately from ordinary transport provenance.
Activation transfers the retained budget only when the new schedule is eligible and capacity permits it.
Retirement invalidates an ordinary owner. A surviving handle cannot update or republish that retired incarnation.
Pending publication inherits the retained total and deadline. A confirmed failed transfer preserves the retired record.
The expiry pass visits active records, retired budgets, and cold pending rows without activating transport.
Completion checks reject cancelled operations after confirmed renewal.

Plan review rejected unconditional retirement and a proposed retirement-history flag in the intermediate code.
Dev retires on an actual due retry or explicit recovery event, not merely because maintenance observes an exhausted active owner.
Both intermediate mechanisms were removed before qualification.
Recovery now distinguishes an existing exhausted schedule from an absent exhausted request.
The former can retire. The latter cannot create a new recovery schedule before renewal.

`retirement-integration.qMgaaH` passed 25 of 26 owner tests.
The remaining test expected an expired-budget probe. Its replacement checks no probe and explicit renewal under dev policy.
`retirement-native.JJJT7s` passed 31 expanded owner tests and 22 of 28 focused retriever tests.
The retriever failures exposed local receipt gating and fixture reactivation changes. They were not reported as successful qualification.
The generated cursor failure remains recorded in the regression corpus.

Further source review corrected an earlier mistaken statement that dev swept expiry before active processing.
Dev performs that sweep afterward at `block_retriever.rs:1088–1093` in the pinned source.
Two new native regressions then failed in `retirement-order-red.v7tWJ8` with unchanged source hashes.
One demonstrated a stale request clock through actual pending recitation. The other demonstrated an incorrect dispatch after early renewal.

`RetryMaintenanceOrder.tla` passed 5,303 states at depth eleven and all five exact unsafe controls in `run.WHo7N5` before those production corrections.
The caller now sweeps after active examination, using its captured time and preserving error accumulation.
Fresh network activation commits the new request clock before active publication.
Local receipt retains an existing quarantined schedule without creating an absent quarantined schedule.
The fixture-only activation path preserves explicitly seeded transient data. Production network activation creates fresh schedule data.

`retirement-order-green.kcUJnz` passed all 32 owner tests and all 30 focused retriever tests.
These include the original retirement and expiry failures, the new ordering failures, generated histories, and a 129-row cold expiry pass.
Strict Clippy and broader qualification remain in progress at this entry.
The commands use a 4 GiB memory ceiling, zero swap, one build job, and scratch files under `target/verification/`.
No new validator, vote, fork-choice, or finality policy is part of this repair.

### September 9: retry-budget custody before production retirement

The scheduler still identifies this local ownership repair as approved and in progress.
The documentation review changed no Casper code and granted no new protocol authority.
The implementation target remains pinned dev's ordinary transport retirement and expiry, with the separately approved pending-publication repair.

The plan agent found gaps in the first `RetryBudgetCustody.tla` draft.
Its capacity guard could not refuse an inactive hash. Deadline transfers lacked an independent reference.
Its probe invariant tested the configuration string rather than a dispatch observation. Failed publication checked only the budget.
The revised model adds an independent capacity, reference deadlines, observed and authorized dispatch counts, and a failure-state comparison.
Fresh requests and completed retry actions permit bounded repeated renewal cycles.

`run.IcKAe3` failed initial-state enumeration before any state search. The initializer now enumerates a subset explicitly.
`run.iu7MvE` checked the safe model but failed the early-activation negative control because no invariant violation occurred.
The Boolean assignment lacked parentheses. TLC treated the remaining conjunction as an action guard and excluded the intended unsafe transition.
The activation and failed-publication observer assignments now parenthesize their complete Boolean expressions.
The invariant was not weakened.

`run.pF6pGv` passed 17,295 distinct states at depth 27 and all twelve exact unsafe controls.
The gate exited successfully and verified its captured hashes.
The run used a 2 GiB systemd memory ceiling, zero swap, one CPU, and a 1 GiB Java heap.
Evidence and scratch files remained under `target/verification/buffer-publication/`, not `/tmp`.

The plan agent found no remaining vacuity blocker in the strengthened model.
Its suggested structural checks now require active custody and nonzero retired deadlines.
`run.vNcEQc` passed the same 17,295 states and twelve exact unsafe controls with those additional checks.
The final gate exited successfully. Shell syntax and focused diff checks passed.

The model checks atomic custody transfers, not full production integration.
Reserved operations, stale owner handles, captured sweep timestamps, unconfirmed writes, and restart retain their separate model and native obligations.
The ordinary retirement regressions remain failing until implementation. No production source changed during this verification step.

### September 9: worker evidence transfer

`worker-first-capture-red.7Bk3sp` failed the first-publication provenance assertion.
The test removes request tracking during body storage, after the interest lookup and before successful pending publication.
The body, complete pending row, and queue cleanup assertions passed before the provenance assertion failed.
All three captured source hashes matched.
This case exercises tracking removal. It does not claim to implement ordinary budget retirement.

The model now includes the existing verified-stored restoration entry point.
`run.jY3uxq` passed 194,304 distinct states at depth fifteen and all ten exact unsafe controls before production changes.
The gate verified its hashes and exited successfully.
The incoming old-block capture invariant excludes the separate stored-restoration path.
Both paths retain verified identity, storage, custody, provenance, context, and terminal requirements.
The model does not establish cancellation or process-death recovery.

The implementation captures private-field evidence during the existing interest lookup.
Successful storage and valid content identity activate accepted evidence.
The worker keeps that evidence through publication errors and retries stored publication without repeating interest checks.
Initial publication, quarantine restoration, and validation deferrals pass captured provenance to the existing paired transaction.
The stored path prepares verified evidence before fallible dependency reconstruction or publication.
No validation, finality, or settled-admission decision uses this local evidence.

`worker-capture-green.S9dK4J` passed all fifteen worker publication tests, including the new failing case and retry exhaustion without manual redelivery.
Strict Clippy then rejected an unnecessary reference in the identity comparison. That expression is corrected.
The expanded rejection tests first failed compilation because they referenced a private storage encoder.
Those fixtures now use the public block-store writer and inspect its persisted bytes.
`worker-evidence-green.FY126B` checks the corrected fixtures, generated binding histories, worker regressions, and strict lint.
All nineteen tests and strict Casper/node library-and-test Clippy passed. All five captured source hashes matched.
The generated test enumerates all eight hash/context/storage-failure combinations before each of 128 generated histories.
The new tests also cover malformed signatures, invalid format, invalid content identity, missing bodies, corrupted bodies, mismatched stored keys, and replacement owners.
Formatter and diff checks passed.

The plan agent found no blocking implementation defect in the inspected worker integration.
Shutdown still aborts workers. The current evidence does not prove accepted-work custody across cancellation or process death before paired publication.
Ordinary transport retirement, budget renewal, and whole-pass expiry integration also remain incomplete.

The read-only restart audit found that buffer reconstruction and startup discovery use persisted dependency rows, not all stored block bodies.
Ordinary recovery also uses buffer candidates. Pinned dev uses the same buffer-first discovery approach.
Thus, an isolated stored body has no demonstrated restart discovery path after failed pending publication.
Captured request provenance also disappears with volatile worker state.
This is a source-supported gap, not a newly demonstrated native failure.
The diagnostic regression must compare failed publication against successful publication, then recreate recovery over the same stores without redelivery.
The audit does not authorize a new journal, shutdown policy, or Casper architecture.

### September 9: terminal membership and metadata availability

`terminal-publication-red.Pt3z5r` reproduced pending republication after interleaved certified admission.
The earlier `terminal-publication-red.gwGQBl` failed because its fixture supplied inconsistent authority evidence. That failure did not demonstrate the race.
The repair retains the existing DAG read lock through the terminal check and synchronous pending publication.
`terminal-publication-green.1emETR` passed fourteen worker tests, twelve atomic transition tests, and strict Clippy for block-storage, Casper, and node.

Independent review then found a predicate error in the new guard.
The readiness predicate returns false for a DAG member whose persisted metadata row is missing.
That result cannot authorize pending publication.
`publication-projection-red.JkP2Nk` failed the explicit missing-metadata exclusion assertion before the correction.

The updated model separates terminal membership from metadata availability.
`run.MEy3OS` passed 33,728 distinct states at depth fifteen before the production predicate changed.
Nine unsafe controls failed at their expected invariants. The gate verified its input hashes and exited successfully.
The new unsafe control reproduces publication based on metadata availability instead of terminal membership.

The publication predicate now validates an existing member's metadata or returns its storage error.
Both outcomes exclude the publication callback. The general readiness predicate remains unchanged.
The generated native test includes every membership, row-state, and publication-failure combination before its generated history.
Row states include valid, missing, malformed, and mismatched metadata.
`publication-projection-green.OiV0HN` has passed five metadata tests and the actual restoration race test.
Strict Clippy for block-storage, Casper, and node also passed. Both captured source hashes matched.
`publication-projection-loom.hSH2Lx` passed four Loom tests, four property tests, strict target Clippy, and its input-hash checks.
Loom exercises the production guard helper with a separate metadata predicate specification. Native histories test the actual storage predicate.
The formatter corrected only the two edited test files. Their formatter checks pass.

The related metadata-native gate stopped at an obsolete expected test count in `run.TIM2RI`.
All seven resolver tests passed, but the script still expected two.
Source inspection confirmed seven current tests. The gate expectation now requires all seven tests to pass.
The follow-up `run.cOfkCI` passed all 29 metadata-native tests, both strict Clippy checks, and all captured input hashes.
The gate exited successfully against the formatted source. Shell syntax, formatter, and diff checks also passed.

Pinned dev has no equivalent publication guard to adopt.
The correction uses dev's existing DAG-before-buffer locking order and preserves terminal membership semantics.
This finding repairs the branch's new local guard. It does not establish a separate upstream metadata-projection bug.

The next worker step must transfer captured provenance on first publication as well as publication retry.
The plan agent identified the interval between interest capture and successful publication as another required regression boundary.
That interval is a planned test obligation, not a newly demonstrated runtime failure.
Use a private-field, exact-hash, context-bound evidence value and worker-local state.
Do not give that value validation, settled-admission, or finality authority.
The retirement regression must finish through the original worker without manual redelivery.

### September 9: accepted publication provenance

`retirement-red.mGwoqe` reproduced two differences from pinned upstream behavior.
Maintenance kept an exhausted ordinary transport schedule and its dependency provenance.
The budget and quarantine checks passed before both intended assertions failed.
All three source hashes matched. Production retirement remains unchanged.
The volatile expiry test now expects upstream provenance removal, not the earlier unsupported retention preference.

The plan review requires exact-hash and context-bound evidence for accepted local work.
The worker must capture dependency provenance with its original interest decision.
Only successful identity checks and storage can activate that evidence for pending-publication repair.
The bounded worker lease must retain that responsibility through publication failure and transport retirement.
The evidence must not authorize validation success or bypass ordinary consensus checks.

`AcceptedPublicationCustody.tla` models two block identities, two Casper contexts, provisional evidence, verified storage, publication failure, retirement, and terminal admission.
The first attempt used an invalid initial-state enumeration and did not run a state search.
The corrected `run.HjTmow` checked 16,896 distinct states at depth thirteen.
Eight unsafe controls produced their expected invariant failures. All captured hashes matched.
Publication is atomic in this model. Native integration must establish the corresponding terminal-publication boundary.
The model does not establish storage fault recovery, worker shutdown behavior, or liveness.

`captured-provenance-red.lzrGAY` demonstrated that publication ignored supplied dependency provenance when a request owner already existed.
The conflict fixture also failed, but that failure came from an incorrect assertion about empty-parent lookup.
The corrected fixture uses durable-row presence and pendant status for an empty dependency row.
That assertion correction is not a production bug fix.

Publication now joins captured provenance into the candidate policy under the owner lock.
The paired transaction publishes the dependency row and policy before the local owner changes.
An existing true provenance flag cannot become false.
`captured-provenance-green.p5EkHb` passed five publication tests, including generated histories and concurrent completion.
Strict Casper library and owner-test Clippy passed. Both captured source hashes matched.
The worker evidence, retirement, and renewal integration remain incomplete.

### September 9: independent expiry traversal

The plan review required separate recovery and expiry orders over the same candidate membership.
Shared cursor consumption can partition service between consumers, even when each consumer runs repeatedly.
The selected representation adds a second pair of links to each existing entry.
Insertion and removal update both orders under the existing membership lock.
Recovery retains its previous order and rotation behavior.

The new expiry batch method returns at most 64 candidates and respects the caller's remaining allowance.
It releases traversal locks before the caller performs policy or storage work.
Whole-pass serialization, fixed sweep time, renewal, and error handling remain integration requirements.
This component does not change finality, voting, or proposal rules.

The first formal attempt had a TLA+ expression precedence error.
The corrected model passed before production changes in `run.AbplEp`.
The safe configuration explored 4,430 distinct states at depth sixteen and checked pass-completion liveness.
Seven unsafe controls produced the expected failures for shared order, arrivals, duplicates, removal, early return, skipped work, and overlapping passes.
The gate checked all captured input hashes.

The model contains three identities, a two-entry batch, separate selection and processing, and arbitrary concurrent membership changes.
Its coverage claim concerns members that remain present throughout a pass.
Removal followed by reinsertion creates a new coverage obligation in a later pass.
Weak fairness of selection, processing, and return is required for pass completion.
The model does not prove storage I/O completion or application-level retry renewal.

`traversal-native.690mtH` passed seven native queue tests and ten tests in the Loom target.
Three of those ten tests use Loom scheduling. Seven are imported native helper tests.
Both strict Clippy checks passed, and all three captured source hashes matched.
The tests compare independent reference orders and check links after generated mixed operations.
The concurrent tests exercise the actual queue helper behind a Loom mutex.
`traversal-storage.kyKjL6` passed the storage batch and restart regression and strict block-storage Clippy.
The regression checks zero allowance, the 64-entry cap, the final partial batch, removal, insertion, and restored membership.
All three captured source hashes matched.

### September 9: actual worker at retry exhaustion

The new actual-worker fixture blocks pending publication through the existing storage interface.
It waits for the first failed write, then drives thirty-two completed public recovery actions with one connected peer.
The transport response delay is 501 ms. No test mutates counters or timestamps.
The next recovery call reaches the budget boundary without another transport request.

The assertion accepts the existing ownership union: a worker lease, complete pending ownership, or a current retry owner.
After storage becomes writable, the fixture joins the worker and supplies a response if pending publication still has no owner.
The final assertions require complete dependencies, old-block eligibility, and release of worker identity and bytes.
The complete fixture has a 120-second timeout. Each controlled worker wait has a ten-second timeout.

`worker-retirement.6S5NSL` passed all thirteen publication tests and strict node test Clippy.
All four captured source hashes matched.
Independent review found a test race despite that successful run.
The initial fixture decided redelivery from `worker.is_finished()` before joining the worker.
A later completion could make that decision stale and cause a false failure.

The corrected fixture joins the worker before checking final pending ownership.
It also checks the quarantine deadline when a request state remains.
`worker-retirement-final.fk8dio` passed the corrected regression in 16.60 seconds and passed strict node test Clippy.
All four captured source hashes matched.
Only the changed regression was rerun after this fixture-only correction.

These results qualify the current retained-owner baseline, not the proposed upstream-style retirement integration.
The fixture explicitly supplies eventual response delivery. It does not prove autonomous recovery.
Its timing assumes no backward system-clock adjustment during the response intervals.

The pinned upstream source has a related first-publication failure path.
`block_processor.rs:455–463` propagates buffer publication failure.
The upstream worker acknowledges that dependency error at `block_processor_instance.rs:596–607`.
`ack_processed` delegates to retriever acknowledgement, and the worker later releases its in-flight guard.
These source observations support possible accepted-work loss when no buffer row was written.
They are not a newly executed upstream regression.

The plan review recommends testing continued worker-lease ownership before introducing a separate retained-retriever lifecycle.
An active retained owner can resume automatic requests after expiry, even if its peer schedule was cleared.
Thus a provenance marker alone cannot establish upstream retry equivalence.
Ordinary transport retirement, accepted-work handoff, and the expiry traversal still require complete integration and regression evidence.

### September 9: upstream quarantine entry

`quarantine-entry-red.8682TV` checked three new owner regressions before production entry changes.
The outstanding final peer operation correctly prevented early quarantine entry.
Both durable and volatile entry tests failed to clear upstream peer state after completion.
The observed tuple was `(3, 7, Some(3), Some(4), Some(5))`.
Upstream requires `(0, 0, None, None, None)` for peer attempts, cursor, and the three cooldown fields.
Both captured source hashes matched.
The wrapper checked hashes after the expected test failure. Its final exit status is not a passing test verdict.

`run.YqPe6F` checked the initial entry model with two safe configurations and eight unsafe controls.
The failure monitor then expanded to compare every modeled state field.
`run.grx3J5` passed both safe configurations and twelve expected unsafe-control failures before the storage production changes.
The safe searches covered 732 and 7,956 distinct states, at depths ten and twelve.
All captured model and gate inputs matched at completion.

The entry model uses one hash, budgets one and two, and two or three immutable operation identities.
It permits concurrent reservation, completion, cancellation, schedule updates, persistence, and entry.
It abstracts cursor and cooldown values as untouched or touched.
It does not cover complete retirement, repeated cycles, physical corruption, or end-to-end node behavior.

The new storage transition validates a monotonic effective policy before applying the exact upstream entry projection.
It preserves total attempts, timestamps, and provenance. It clears peer attempts, cursor, and cooldowns, then sets the quarantine deadline.
Ordinary policy updates retain their previous monotonic checks.

`quarantine-entry-storage.lzZ3yx` passed sixteen pending-policy tests, six policy tests, and strict block-storage Clippy.
All three captured source hashes matched.
The new example covers observed counts, transaction failure, unchanged rows, reopening, and stale expected state.
The generated histories cover ordinary updates, entry, expiry renewal, injected failures, and reopening across repeated cycles.
Full-width field properties check the exact projection and reject an unexhausted or zero budget.

The plan agent clarified volatile custody under the user's upstream-authority rule.
Ordinary transport requests must follow upstream retirement. Blanket volatile-provenance retention has no separate demonstrated justification.
However, failed first publication can leave an already stored block with no buffer row and a retriever retry owner.
The existing worker regression proves that handoff, not its behavior across later budget retirement.
That boundary requires investigation before any retention exception or blanket deletion rule.
No new approval question is needed merely to restore upstream behavior for ordinary transport requests.

The actual owner entry regressions remain unresolved. No production owner or retriever entry change has been applied in this step.
The plan agent found no blocking storage defect in the reviewed step.
The review requires current ownership, exhausted allowance, no reserved operations, and upstream deadline calculation at the owner boundary.
It also requires protection against repeated entry restarting an existing quarantine.
The entry model does not establish those integration preconditions for restored policies or legacy probe states.

### September 9: renewal storage and exact row comparison

The storage primitive now resets only the spent total and quarantine deadline, with a checked revision increment.
It preserves every other field, including scheduling state created by recitation before the expiry sweep.
Owner renewal and maintenance integration remain unfinished. The four complete-path quarantine regressions remain unresolved.

`run.LtsKYw` extends the earlier renewal model with repeated cycles and unconfirmed commit outcomes.
Seven safe configurations passed. Thirteen unsafe controls produced their expected invariant failures.
All captured model and gate inputs matched at completion.

| Safe configuration | Distinct states | Search depth |
|---|---|---|
| Durable | 31,567 | 17 |
| Volatile | 2,553 | 12 |
| Independent hashes | 186,590 | 22 |
| Concurrent hashes | 1,560,022 | 24 |
| Two cycles | 968,086 | 26 |
| Unconfirmed commit | 63,711 | 20 |
| Concurrent hashes with unconfirmed commit | 4,243,874 | 27 |

The repeated-cycle case uses one hash and three operation tokens.
The concurrent cases use two hashes. The largest search uses two operation tokens and one renewal per hash.
All cases use a normalized one-attempt budget and two maintenance actors.
Persistent cases permit one restart. Reopening assumes valid storage with the old or new paired policy.
These finite searches do not prove unbounded lifecycle correctness or recovery from arbitrary physical corruption.

The backend review found no fallible Rust operation after successful LMDB commit.
However, LMDB can return a fatal metadata-write error without confirming restoration of the prior metadata.
The model now separates this uncertain outcome from a confirmed comparison conflict.
Further native recovery, custody bounds, and traversal integration remain required.

The initial storage test run, `renewal-storage.fkg81t`, exposed a dependency-row comparison defect.
The old guard decoded a `HashSet`, then re-encoded it as the expected comparison bytes.
Decoding can change iteration order. An unchanged parent set can therefore produce a false conflict.
This defect belongs to the branch's pending-policy extension, not an upstream Casper protocol rule.

`row-guard-red.qd87wF` reproduced the failure independently for ordinary update and renewal.
The deterministic fixture uses duplicate-member storage bytes accepted by the existing decoder, not ordinary writer output.
The separate distinct-parent case exercises the ordinary publication path.
Both captured regression source hashes matched.

Before the production correction, `run.KJ6Znb` checked the row-guard model.
The safe search covered 942 distinct states at depth seven.
The re-encoding control failed `Inv_ExactRowGuard`, as expected. Captured inputs matched.
The plan agent confirmed the minimal repair and its required boundary tests.

The corrected guard retains exact stored bytes and validates them before the paired transaction.
It finishes the read callback before writing. It does not change the decoder, storage format, or policy comparison.
`renewal-storage.L1rULg` passed eleven persistence tests, five policy tests, and strict block-storage Clippy.
All three captured source hashes matched at completion.

`row-guard-coverage.SLknYX` passed all fourteen pending-policy tests and strict block-storage Clippy after additional coverage.
All three captured source hashes matched again.
Generated properties exercise serialization order, duplicates, empty sets, stale policy, and injected transaction failure.
Native tests cover malformed and missing rows, concurrent removal, changed parents, and an equivalent representation replacement.
Concurrent tests pause after the raw read and mutate storage before the paired transaction.
Rejected transactions preserve the replacement and the original policy.

The exact byte guard retains the existing same-byte deletion/reinsertion limitation.
Neither these native checks nor the finite model establish generation-based row ownership.
No new production Casper policy changed in this correction.

### September 9: checked quarantine renewal model

This subsection records the earlier model revision. The subsection above records its later extensions and storage implementation.

Four independent native regressions now separate volatile and durable ownership from the two upstream policy assertions.
`renewal-red.sstXYI` failed all four before production changes.
Both ownership modes sent one autonomous request and reported thirty-three attempts instead of a renewed zero.
All three captured source hashes matched.
The provenance assertions follow the failed budget assertions and do not yet establish post-renewal preservation.

The plan agent reviewed the renewal model and storage preconditions.
The review required a separate cancelled-token control, a complete atomic-failure monitor, and an independent durable reference.
The model now preserves old operation identities instead of reusing their slots for new callbacks.
The volatile configuration keeps revision one and uses its current deadline as the renewal guard.

The first two model runs stopped on TLC evaluation errors before a complete search.
`run.4DC4Tn` exposed unbound initial Boolean variables.
`run.GSvzhH` exposed a failure monitor that read next-state variables before TLC assigned them.
These were model implementation errors, not production counterexamples.
After correction, `run.WWOfWL` passed three safe configurations and eleven expected unsafe-control failures.

`run.0g5Kkw` added concurrent independent hashes and passed the complete renewal gate.
All captured model and gate inputs matched.

| Safe configuration | Hashes | Operation tokens | Distinct states | Search depth |
|---|---|---|---|---|
| Durable | 1 | 2 | 20,330 | 16 |
| Volatile | 1 | 2 | 2,553 | 12 |
| Independent hashes | 2 | 1 | 73,592 | 20 |
| Concurrent hashes | 2 | 2 | 597,506 | 23 |

Each configuration permits two maintenance actors, one restart where persistence applies, and one renewal per hash.
The normalized attempt budget is one. The clock covers three values.
These are exhaustive finite searches, not unbounded lifecycle proofs.

The backend audit distinguishes a confirmed comparison conflict from a fatal LMDB commit error.
Memory publication has no fallible step after validation.
The LMDB wrapper has no fallible Rust operation after a successful `writer.commit()`.
However, LMDB's metadata-write error path cannot guarantee successful restoration after every I/O error.
The current renewal model covers confirmed atomic failures only.
Unconfirmed commit recovery, repeated cycles, and custody/traversal bounds remain required before the production repair is complete.
No production renewal change has been applied.

### September 9: upstream retry selection

The subsequent quarantine review confirmed a remaining operational difference from upstream.
`renewal-red.ZUMFRJ` failed the new native no-autonomous-probe assertion before any renewal repair.
The retriever sent one request after expiry, where the upstream sweep sends none without recitation.
All three captured source hashes matched.
The failure occurs in the volatile case before the durable case or the reset assertion.
Those additional cases still need independent evidence.
The [renewal plan](../casper/theory/finalized-floor/buffer-publication-ownership.md#quarantine-renewal-repair-plan) records the reviewed transition and remaining custody bounds.
No renewal production change has been applied.

The plan review identified three scheduling differences from pinned `dev`.
Unresolved requests now use the fixed 500 ms retry ladder, independent of received-entry expiry.
Suppressed selections preserve upstream timestamp and cursor updates without charging an attempt.
Action metrics precede transport. Completion metrics count returned actions even when subsequent policy persistence fails.

`selection-red.EczqLs` passed fourteen existing tests and failed all three new scheduling regressions before the production correction.
Its three source hashes matched.
The wrapper subsequently failed because systemd expanded its shell status expression.
The captured test output establishes the regression failures independently of that wrapper error.

`run.CPzJ6s` checked the strengthened `RetrySelectionPolicy` model before the production correction.
Three safe configurations passed. Ten unsafe controls produced their expected invariant failures.
The safe configurations checked 4,680 durable, 4,632 volatile, and 2,340 no-peer states, each at depth ten.
These finite results do not establish unbounded protocol correctness.

`selection-green.IsyJGT` passed forty-eight retriever tests, twenty owner tests, and fourteen transport-policy tests.
Strict Clippy passed for Casper test targets. All five captured source hashes matched.
Generated properties cover the upstream clock ladder, wrapping cursors, suppressed policy preservation, and received-owner no-selection behavior.
Native cases also check cancellation and metric preservation across failed completion persistence.

The independent review found no blocking production defect in this patch.
It requested two further coverage checks: exact suppressed-action metric labels and failed suppression publication with no partial state.
Both checks are now present.
`selection-boundaries.TkOY8r` stopped during compilation because the new tests omitted the `Future` trait import.
After that test-only correction, `selection-boundaries.zYHtj0` passed all fifty retriever tests and strict Casper test lint.
Its three captured source hashes matched.
The new checks cover both suppressed paths, durable-policy conflicts, unchanged request data, no transport, and released operation capacity.

The model abstracts peer indices and permits prepared snapshots before publication.
Production uses wrapping integer cursors and holds ownership locks during selection publication.
The native properties check that representation difference.
Capacity saturation, pending handoff, deferred completion, and quarantine renewal require their separate checks.
Quarantine renewal remains incomplete. This patch does not change that policy or establish whole-branch qualification.

### September 9: pending worker integration

The worker now distinguishes paired pending ownership from terminal DAG admission.
Pending acknowledgement no longer deletes request provenance or retry policy.
The processor obtains its buffer from the retriever, which prevents separate buffer instances at that constructor boundary.

`worker-handoff.LBdNKo` passed all twelve node publication tests.
The ordinary, quarantined, and restarted-instance provenance cases passed after their earlier failures in `provenance-red.qDrAi4`.
The same run covered publication failure, tracker capacity, partial dependency rows, and conflicting incoming block identity.
Its captured inputs matched when that run completed.
Later fixture migration changed the retriever file. The older result does not qualify those later edits.

Production startup and worker callers now use the canonical owner and shared buffer.
Test fixtures read request snapshots instead of independent mutable request maps.
Tests seed explicit owner policy when they need counter or quarantine preconditions.
The generated handoff property checks active-slot release and exact cold-restored policy, rather than deletion of obsolete auxiliary maps.

`retriever-integration.Q3vgOd` stopped during compilation because seventeen test calls still referenced removed private helpers.
This was fixture migration failure, not a completed regression run.
The updated fixture helpers access the canonical policy. No production retry decision changed during this migration.

`retriever-integration.gzVI0u` passed forty of forty-two tests with matching captured source hashes.
All three original stale-completion regressions and the untracked-capacity-dispatch regression passed.
Two fixtures still needed migration. One reset counters when it changed a request timestamp.
The other attempted to inject an over-capacity state that the canonical owner now rejects at insertion.

The plan agent reviewed both fixture corrections.
The peer test changes only the existing owner's timestamp and checks two total attempts with one peer attempt.
The capacity test checks refusal of entry 2,049 and preservation of all 2,048 accepted identities after maintenance.
Neither correction changes production policy.

`retriever-integration.CB9ug5` passed all forty-two retriever tests, eighteen owner tests, and thirteen transport-policy tests.
Targeted strict Clippy passed, and all four captured source hashes matched.
Test-only owner convenience methods now reside in the owner test module, not in production source.
The run does not establish complete upstream policy alignment or whole-branch qualification.

`worker-integration.YoLE8j` passed all thirty-six node worker tests and compiled the Casper integration target.
All nine captured source hashes matched when checked after the run.
The integration check found two unused imports from fixture migration. Those imports were removed.
The following lint command did not run because it repeated the `--lib` option.
`publication-lint.08O9BZ` passed strict Clippy for all Casper and node test targets.
Its eight captured source hashes matched after completion.
The formatter subsequently expanded one test assertion without changing its tokens or production behavior.
Focused formatting and repository diff checks then passed.

`handoff-loom.ikbctZ` passed the two production-helper concurrency checks.
Their two unsafe controls produced the expected assertion failures.
All four captured source hashes matched.
These Loom checks cover receipt reopening and dependency promotion, not the complete owner registry or durable storage implementation.

Pending ownership at one observation does not prevent a separate pruning operation from deleting the obligation later.
The unresolved pruning architecture remains subject to upstream disposition.

### September 9: restore upstream retry-action counting

The user requires upstream Casper decisions unless a demonstrated upstream defect justifies a local repair.
The plan-agent review confirmed that pinned `dev` counts completed retry actions, including returned transport errors.
The earlier success-only interpretation was incorrect.
Cancellation before completion does not count an action.
Recovery can append peers to a nonempty waiting list without dispatch, then count that completed action.

Six native regressions failed against the actual retriever before this correction.
The tests exposed missing error-path counts, propagated initial transport errors, and an extra recovery broadcast.
The repair now logs block transport errors and preserves upstream fallback broadcasting.
It completes each operation against its original owner rather than looking up a replacement by hash.
Storage errors still propagate and retain deferred completion.
Certificate transport handling is unchanged.

| Evidence directory | Result |
|---|---|
| `run.GkTSM1` | Revised retry model checked 107,944 states at depth 25. Seven exact controls passed. Sixteen Rocq theorem reports and independent kernel checking passed. |
| `run.DakDXS` | Revised deferred model checked 2,538 states at depth 13. Seven exact controls passed. |
| `retry-policy-red.bn1cqA` | All six native policy regressions failed before the repair. Captured source hashes matched. |
| `retry-policy-green.KkUM51` | Seventeen owner tests and ten transport tests passed. Captured source hashes matched. |
| `retry-policy-boundaries.dE4g5D` | Thirty tests passed. Strict Clippy found an unused method in the independently compiled owner test module. |
| `retry-policy-final.4UaWgg` | Eighteen owner tests and thirteen transport tests passed. Focused strict Clippy and the node build check passed. All captured source hashes matched. |

Both models now distinguish returned errors from cancellation.
The new negative controls reject success-only counting.
The generated owner histories include returned success, returned error, and cancellation across overlapping operations and request replacement.
Native transport cases cover both cancellation points in a waiting-peer action and cancellation of known-peer, broadcast, and recovery actions.
Further cases cover pending publication, replacement, failed policy persistence, and continued independent request maintenance after transport errors.
These checks do not establish complete node or cross-validator correctness.

Startup now creates the retriever after opening Casper buffer storage and passes that same storage instance to the retriever.
The earlier raw request map is removed from startup.
Fixture migration and pending-versus-terminal worker acknowledgement remain incomplete.
Quarantine-expiry policy remains a separate upstream-alignment issue. This repair does not silently select the feature's probe policy.

The independent plan agent reviewed the focused source changes and found no blocking defect in this correction.
The review confirmed that certificate errors remain visible.
It also identified remaining upstream differences in retry timing, cursor changes during cooldown suppression, and action-metric timing.
Those differences still need explicit upstream alignment within this repair.
The thirty-one passing tests do not qualify those remaining paths or the complete publication repair.

All new heavy checks used systemd memory caps, disabled swap, and one CPU per scope.
The two earlier formal scopes remained active with 2 GiB caps when inspected.
The new combined verification cap stayed at or below 8 GiB, including those existing scopes.
No Git commit was created.

### Canonical owner component and native correspondence

The candidate request-owner component now has native tests against the actual Casper buffer storage API.
The existing formal models preceded this component implementation.
Production retriever and worker integration remain incomplete.

| Evidence directory | Result |
|---|---|
| `request-owners.AHrnwt` | Nine initial component tests passed. Captured source hashes matched. |
| `request-owners.yv6k5x` | Fifteen tests passed. Strict Clippy rejected one nonminimal Boolean expression. The source-hash check passed separately. |
| `request-owners.pCg7BU` | Seventeen tests and strict Clippy passed after the equivalent Boolean simplification. Captured source hashes matched. |

Two properties each run 128 generated histories.
The second property retains multiple outstanding operations across pending publication, cancellation, terminal disposal, and replacement.
Its independent counter oracle charges only successful operations from the current incarnation.
Every step checks operation permits, active capacity, and the committed policy when present.

Native thread cases exercise competing reservations, concurrent cold lookup, and completion during pending publication.
Additional cases cover missing-row errors, revision conflicts, failed publication, queue rotation, saturated counters, and restored probe deadlines.
These tests use the candidate component directly.
They do not show that the seven original worker and retry failures are repaired.

All new checks used a 4 GiB memory cap, disabled swap, one CPU, and on-disk temporary files.
The two existing 2 GiB formal scopes remained active when inspected.
No Git commit was created.

### Paired storage and completion-persistence boundary

The storage layer now commits pending request policy with its complete dependency row.
The new namespace shares the existing Casper buffer database environment.
Constructors and fixtures require both stores.
The production paths for pending acknowledgement and operation ownership still need integration.

| Evidence directory | Result |
|---|---|
| `policy-storage.JgjoAx` | Five new storage tests passed, including generated histories and concurrent revision conflicts. |
| `policy-integration.Rkvjrk` | Six policy tests passed, including the added LMDB process-crash test. The broad `durable_` filter stopped at the known pruning regression. |
| `policy-lint.UQK8lm` | Strict storage, Casper, and node Clippy passed for library and test targets. Captured source hashes matched. |
| `run.VqFVUz` | Deferred completion checked 2,538 states at depth 13. Six exact unsafe controls passed. Captured source hashes matched. |

The storage tests verify atomic dependency unions, policy updates, terminal row deletion, failed commits, stale revisions, and database reopen.
The crash fixture exits a child process before or after the actual LMDB commit.
The reopened database contains either the complete old pair or the complete new pair.
An unrelated policy record remains unchanged.

The authorized plan review identified a required failure boundary in the proposed owner integration.
Successful transport followed by failed policy persistence must retain the completion, owner, and operation permit.
A later write can include several completions without charging them again.
The model now distinguishes these transitions before their production implementation.
It explicitly permits loss of uncommitted completion information on process death.
It does not claim exact network-send accounting across restart.

Seven original worker and retry regression cases remain unresolved.
The independently repaired promotion case remains qualified.
The separate pruning architecture still awaits upstream disposition.
No Git commit was created.

### Approved persistent-provenance continuation

The latest user approval requires a failing regression before each fix and the same regression passing afterward.
Scheduler revision 704 retained this task as the incumbent.
The separate pruning architecture remains subject to upstream review.

| Evidence directory under `target/verification/buffer-publication` | Result |
|---|---|
| `provenance-red.qDrAi4` | Announcement promotion failed. Three worker cases failed old-block eligibility after pending handoff, including reconstructed request and buffer instances. |
| `promotion-proof.fQ3ZvO` | Safe promotion model and four exact unsafe controls passed. Six Rocq theorem checks and independent kernel validation passed. |
| `promotion-green.PGYmVv` | The promotion regression, all 38 retriever tests, both Loom tests, and strict Casper Clippy passed. Input hashes matched. |
| `run.fDNKEy` | The first durable model run rejected an incomplete successor state in a negative control. This run did not qualify. |
| `run.DyfmoT` | The corrected durable model checked 106,856 states at depth 19. Six exact unsafe controls passed. |
| `delayed-retry-red.lEj4kI` | All three delayed retry regressions failed. Each old operation incremented a replacement request's counter from seven to eight. Input hashes matched. |
| `run.7VXmuD` | The retry-operation model checked 107,944 states at depth 25. Six exact unsafe controls passed. |
| `retry-capacity-red.HziWEH` | Recovery dispatched an unowned request after the active tracker reached its 2,048-entry bound. Input hashes matched. |
| `run.iKtfl2` | The retry-operation model and all controls passed again. Thirteen closed Rocq theorem reports and independent kernel validation passed. |
| `run.jphWUk` | Canonical registry ownership checked 129,259 states at depth 16. Four exact unsafe controls passed. |
| `policy-codec.BIqiba` | Four bounded policy-codec tests and strict storage Clippy passed. Input hashes matched. |

The promotion fix changes only the dependency fact under the existing request mutex.
It does not reset counters, age, quarantine, or receipt state.
The three worker provenance cases and three delayed-completion cases still fail before their corresponding repairs.
No test assertion was relaxed to conceal those failures.

The durable model checks parallel capture, update, commit, retirement, terminal disposal, eviction, and one restart.
It uses two keys, two workers, one active slot, and two revisions.
The first draft needed parentheses around a Boolean assignment.
The negative-control gate detected that error before durable implementation began.

The retry model uses one hash, two request incarnations, two operations, one active slot, one ordinary allowance, and one clock round.
It covers preparation, transport, successful completion, failure, cancellation, pending transfer, activation, receipt, replacement, quarantine, and restart.
Six controls detect hash-only completion, premature charging, lost pending completion, double charging, unreserved budget, and duplicate quarantine probes.
These are bounded component checks, not proof of the complete Rust program.
The separate registry model now checks canonical-owner loading and resident-owner bounds.
Its assumptions still require concrete implementation correspondence and concurrency tests.

The authorized plan agent recommends a shared per-request policy owner and a bounded weak registry.
Every activation must reuse an existing live owner before loading a durable record.
Outstanding operations retain their exact owner across pending handoff.
Terminal cleanup closes that owner before another incarnation can replace it.
Dead weak entries need removal, and operation permits must bound outstanding old owners.
No lock may cross network transport.

The review rejected unconditional pre-transport counter increments.
The earlier review attributed success-only counting to pinned `dev`. That attribution was incorrect.
The September 8 evening comparison refresh checked the upstream transport-error branches directly.
Pinned `dev` logs failed retry dispatches and still increments attempts. The feature currently propagates those errors before completion increments.
The checked owner design currently preserves the feature's success-only policy, not equivalence with upstream counting.
Request ownership remains necessary under either policy. Upstream alignment needs an explicit counting-policy decision and corresponding model and regression updates.
Restart cannot distinguish every delivered request from an uncommitted success counter.
The documentation must state that uncertainty rather than claim exact network-send accounting across crashes.

The capacity regression adds a fourth unrepaired retry case.
Three pending worker cases and four retry cases therefore remain red.
The promotion regression is fixed and verified.
The next production change must connect the checked owner and operation contracts to the durable policy store and existing callers.
This requires explicit acknowledgement types, canonical owner reuse, atomic pending publication, and terminal cleanup that cannot target a replacement.
The remaining native matrix includes cancellation, transport failure, concurrent last-allowance requests, durable completion, transaction failure, and actual database reopen.

No durable schema or Casper protocol rule changed in this continuation.
The new policy codec defines version-one record bytes but does not yet open or mutate a durable namespace.
Its generated tests cover all record fields and arbitrary byte inputs with 256 cases per property.
The encoded record has a 75-byte maximum and contains no variable-length peer collection or block body.
No Git commit was created.
All new heavy checks used systemd memory limits, disabled swap, and one CPU.
The aggregate configured cap stayed at or below 8 GiB, including the two older verification scopes.

### Earlier implementation record

The local implementation task is `pr216-publication-local-handoff`.
Scheduler revision 692 selected and claimed this task.
The separate persistent-pruning architecture review remains open and requires upstream disposition.
This repair does not satisfy that review gate.

The implementation now includes these changes:

1. The queue reserves identity, bytes, and channel capacity before synchronous receipt recording.
2. Network, startup, and ordinary recovery use the same receipt-before-publication boundary.
3. Receipt and local failure handoff preserve retry policy and report tracker capacity explicitly.
4. A worker retains its existing lease when neither storage nor the tracker accepts ownership.
5. One existing-schema transaction publishes the complete block and certificate dependency union.
6. Durable-row lookup includes explicit empty rows and excludes implicit missing parents.
7. Quarantine repairs partial rows from the stored block after identity checks.
8. Maintenance rechecks current receipt state and preserves initial age and retry policy.
9. Durable handoff currently retires request tracking after another retry owner exists. The provenance review below demonstrates why this remains incorrect.

The source review confirmed that batch union preserves upstream successful set semantics.
The patch does not change validity, voting, finality, fork choice, or wire data.
Concurrent dependency resolution can still leave a conservative stale edge.
Existing metadata and certificate reconciliation remain necessary.

### Additional failing evidence

| Evidence directory under `target/verification/buffer-publication` | Result |
|---|---|
| `review-red.q0gW9o` | Maintenance reset the original request timestamp. Four repaired receipt-policy tests passed. |
| `quarantine-red.oG2gM0` | A later relation-write failure plus active quarantine released a row with an omitted certificate dependency. |
| `canonical-red.UK53DV` | The first quarantine repair accepted dependencies from a conflicting incoming payload. |
| `canonical-check.tpAZZU` | Stored-identity examples and generated mutations passed. The new request-retirement case failed. |

Each run captured source hashes and confirmed unchanged inputs.
The unquarantined later-write fixture passed because the worker retried publication.
The quarantine fixture supplied the missing interleaving and reproduced the partial-row failure.

The conflicting-payload defect belonged to this repair.
The earlier formal abstraction assumed correct dependency-source identity without enforcing that source boundary.
The additional model, proof, and native tests now distinguish the stored block from the incoming payload.

### Boundary proof evidence

`run.N0P0BS` completed the first boundary specification and 11 closed theorem reports.
`run.lUbSAL` added stored-identity selection and completed 13 closed theorem reports.
Both Rocq runs passed an independent kernel check.
The first attempted proof run, `run.lZUqA8`, failed during a proof tactic and did not qualify.
The corrected proof retained its original completeness obligation.

The latest boundary run checked 80 states at depth eight with one tracker slot.
It checked nine states at depth five with zero tracker slots.
All five unsafe controls violated their exact required invariants.
These checks preceded their corresponding production edits.
They supplement, rather than replace, the earlier concurrent ownership models below.

### Native and concurrency evidence

`green.5pPJwP` passed the first implementation batch:

- Eight storage publication test matches, including crash-boundary and metadata tests selected by the filter.
- One generated storage-history test, extended with mixed dependency batches.
- Thirty-six request-retriever tests.
- Four queue receipt-boundary tests.
- Thirty worker, dispatcher, and recovery tests.
- Four Loom tests, including two unsafe controls.

Those 79 native tests and four Loom tests used unchanged captured inputs.
The later stored-identity correction requires the new qualification run.
`final-native.fmmUkp` passed 33 worker, dispatcher, and recovery tests, 36 retriever tests, and four queue tests.
Strict Clippy then rejected one unnecessary reference in the stored-identity comparison.
That expression was corrected after the run ended.
`lint-and-network.ZYrvvD` contains the subsequent lint and actual network-producer checks.
Strict Clippy passed for `block-storage`, `casper`, and `node`, including their test targets.
The two network-producer tests passed in 30.47 and 31.31 seconds.
The gate returned zero, and captured source hashes passed their post-run checks.
These results do not resolve the pending-provenance failures below.

### Pending provenance regression

The local `dev` cache remains clean at `cdf447ac18710d9702a27379bce6c946f421be46`.
The plan agent identified premature pending acknowledgement in both that source and this branch.
The feature worker adds another affected cleanup after quarantine repair.

`solicited-history-red.KuO2Z6` compiled successfully and ran two actual-worker regressions.
Both failed the same old-block eligibility assertion after successful complete buffer publication.
The tests completed in 0.06 seconds, and Cargo returned 101.
Captured source hashes passed their post-run checks.
These failures remain unresolved. No production correction followed them in this continuation.

`provenance-model.WzOFpG` checked the new `BufferPendingProvenance` diagnostic model.
Premature deletion failed `Inv_PendingEligibility` at depth three.
Retention with insufficient tracker capacity failed `Inv_DependencyProgressAvailable` at depth six.
Restart failed `Inv_PendingEligibility` at depth four.
The restricted no-restart, sufficient-capacity configuration passed 11 states at depth 11.
The combined expected-outcome gate returned zero, and captured input hashes passed.
These expected failures expose limitations. They do not qualify the implementation.

`run.nDK3w6` then passed the complete updated publication gate.
Seven positive configurations and 23 exact negative controls produced their required outcomes.
All 46 theorem reports closed under the global context.
Independent kernel checks accepted all three Rocq modules.
The gate returned zero, and all captured input hashes passed.
The two actual-worker provenance regressions still fail, so this formal gate does not close the repair task.

The final plan review accepted the diagnostic model and its stated limits.
It corrected the capacity example: only the pending chain's tail needs an unrequested ancestor.
A 2,049-node chain fits below the default 16,384-node buffer limit while exceeding the 2,048-entry request limit.
The document now states the required absence of a settled-history shortcut, helpful unsolicited delivery, and pruning during that prefix.
This remains a source-supported objection to retention alone, not a native large-chain reproduction.

The [publication specification](../casper/theory/finalized-floor/buffer-publication-ownership.md#pending-provenance-unresolved-admission-boundary) records the caller distinction and required upstream design decision.

All new heavy commands used systemd memory limits, zero swap, and a 100 percent CPU quota.
Native checks used at most 4 GiB.
Formal checks used at most 2 GiB.
The two earlier 2 GiB verification scopes remained active and were not stopped or restarted.
The configured aggregate limit stayed at or below 8 GiB.
All new temporary files used on-disk `target/verification`, not the RAM-backed `/tmp` directory.
No commit, reset, stash, push, or worktree mutation occurred during this implementation continuation.

## Scope

The previous goal turn updated the local `dev` cache through a fast-forward.
This turn resumed the requested pruning-repair decision review through scheduler revision 679.
It added formal evidence and a worker regression before production changes.
It did not start another state-import task or change Casper consensus behavior.

## TLA+ results

The first publication model separates receipt, durable commit, cache publication, acknowledgement, worker release, eviction, resolution, and restart.
It includes two workers and two block identities.
Both workers can hold the same identity in this abstraction.

The safe one-slot configuration exhausted 23,306 distinct states at depth 21.
The safe zero-slot configuration exhausted 13,118 distinct states at depth 19.
Each run completed without an invariant violation.
Five unsafe controls violated their intended invariants.

The evidence directory is `target/verification/buffer-publication/run.yDN24l`.
Its source-hash file records the exact model and original checker-script revision.
The scope used `MemoryMax=2G`, `MemorySwapMax=0`, and `CPUQuota=100%`.
Each Java process used a 1 GiB heap and one TLC worker.
The gate returned zero after verifying all seven expected outcomes.

The checker script later gained a Rocq mode.
The earlier result does not claim execution of that later script revision.

## Rocq results

`BufferPublicationOwnership.v` compiled successfully.
All 14 printed theorem assumptions reported closure under the global context.
A separate `coqchk` execution accepted the compiled module.

The evidence directory is `target/verification/buffer-publication/run.6UB6ed`.
The scope used `MemoryMax=1G`, `MemorySwapMax=0`, and `CPUQuota=100%`.
The gate returned zero.
The proof is parameterized over keys, cache capacity, and finite operation histories.
It remains conditional on its operation preconditions and ownership abstraction.

## Actual worker regression

The regression invokes the existing `process_owned_block` function.
It uses a current-protocol block with a valid signature and content hash.
The block waits for a missing parent and certificate.
The fault store rejects the first actual `parents-map` transaction.

The fixture established these preconditions before the failing assertion:

- The request tracker owned the received block.
- The worker accepted the block's format, signature, version, and shard.
- The block body reached BlockStore.
- Exactly one injected publication failure occurred.
- The block did not enter the DAG.
- The worker released its queue identity and byte reservation.
- No durable retry row existed.

The assertion then failed because the request tracker no longer contained the block.
The successful-publication control passed with a durable row and no remaining request owner.

Command:

```sh
cargo test --offline -p node --lib publication_tests::publication_ -- --test-threads=1
```

The build completed in 6 minutes and 16 seconds.
The two tests completed in 0.05 seconds.
The result was one expected ownership failure and one passing control.
Cargo returned 101 after successful compilation.
This was not a build failure, timeout, or resource termination.

The evidence directory is `target/verification/buffer-publication/native.ZTX17f`.
Its input hashes passed the post-run identity check.
The scope used `MemoryMax=4G`, `MemorySwapMax=0`, and `CPUQuota=100%`.
The command used one build job and one test thread.

## Upstream attribution

The refreshed sibling `dev` worktree is at `cdf447ac18710d9702a27379bce6c946f421be46`.
Its dependency-error branch also calls `ack_processed`.
Its outer error handler records validation failures without publishing a missing retry row.
These observations establish source correspondence, not execution of the feature fixture against the complete upstream node.

The earlier controlled upstream pruning reproduction remains separate evidence.
Do not combine these two results into a claimed full-dev network reproduction.

## Independent review and next refinement

The authorized plan agent reviewed the specific retry-handoff boundary after the regression failed.
It rejected unchanged reuse of `BlockRetriever::defer_for_admission`.
That method assigns dependency authority to new entries and clears auxiliary retry state.

The recommended local handoff must preserve provenance, attempt counts, cooldown records, and quarantine.
It must distinguish successful tracking from capacity refusal.
The worker must retain its existing identity and byte lease if no other owner accepts the block.
A capacity-refused worker must be able to retry publication, rather than wait only for tracker space.

The current formal model assumes that receipt inserts a tracker entry.
It does not yet model tracker capacity or received-versus-retry-eligible state.
It also does not model dependency authority or retry budgets.
These are explicit refinement requirements before this local handoff enters production.

Required follow-up checks within this repair:

1. Model capacity refusal while a worker retains the only owner.
2. Model retry eligibility, provenance, retry budgets, and quarantine preservation.
3. Prove that failed handoff cannot release the last worker lease.
4. Test publication recovery without another network announcement.
5. Test duplicate delivery, cancellation, and concurrent successful publication.
6. Extract each additional invariant into an implementation property test.

The larger pruning-storage architecture decision remains separate.
No persistent namespace, disk-backed capture implementation, or production error-handler change was introduced in this turn.

## Resource accounting

The resource check found two existing 2 GiB verification scopes still running.
This turn did not stop or restart those scopes.
The new TLC check completed before the 4 GiB native test build started.
The 1 GiB Rocq check started after the native test process exited.
The combined configured memory limits did not exceed 8 GiB during these new checks.
All new temporary files reside under the repository's on-disk `target/verification` directory.

## Capacity and receipt refinement

The next continuation added `BufferRetryHandoff.tla` and `BufferRetryHandoff.v`.
The refined model exposes queue reservation, receipt, publication, capacity refusal, retry readiness, policy preservation, and worker release.
It permits independent workers and enforces the existing per-hash lease exclusion.
Cancellation can release a reservation before receipt changes.
The receipt-to-publication section has no cooperative cancellation point.

The initial refined run completed in `target/verification/buffer-publication/run.rNSS5Y`.
It checked 131,477 states with one tracker slot and 3,823 states with zero slots.
All ten negative controls produced their expected violations.

The first new Rocq compile found a tactic error in the initial-state proof.
No production code used that unchecked proof.
The corrected proof passed compilation and kernel checking in `target/verification/buffer-publication/run.q8qbLh`.

The final refinement separated reservation from accepted work and added cancellation before receipt.
It also separated the policy-preserving local-history theorem from restart with a fresh policy.
The complete gate passed in `target/verification/buffer-publication/run.utZSIT`.
That run checked both model families, all 15 negative controls, and both Rocq modules.
The refined one-slot result was 132,021 distinct states at depth 41.
The zero-slot result was 3,823 distinct states at depth 22.
All 19 refined Rocq theorem reports were closed under the global context.
Both separate kernel checks passed.

The checks used at most 2 GiB per new scope, no swap, and one CPU.
The new native tests ran after each model scope completed, with a 4 GiB cap and one build job.
The two older 2 GiB scopes remained live when inspected.
The aggregate configured cap for overlapping campaign checks did not exceed 8 GiB.

## Additional actual-worker and property results

The worker fixture now fills the real request tracker before delivering an untracked candidate.
Receipt confirms that the candidate did not obtain a tracker entry.
The first buffer transaction fails exactly once.
The current worker then releases its lease without retry publication.

`target/verification/buffer-publication/native.N8pJpY` records the four-test result:

- Tracked failed publication: failed the retry-owner assertion.
- Full-tracker failed publication: failed the durable-owner assertion after worker release.
- Tracked successful publication: passed.
- Full-tracker successful publication: passed.

Compilation completed in 27.09 seconds.
The four tests completed in 0.09 seconds.
Cargo returned 101 with exactly two test failures.
The source-hash check passed.

Three generated receipt properties call the real `BlockRetriever::ack_receive` method.
`target/verification/buffer-publication/policy.YE3uot` records one passing property and two failing properties.
The retry-counter failure shrank to one attempt, one peer attempt, and one receipt.
The quarantine failure shrank to deadline one, cooldown one, and one receipt.
The authority and original-timestamp property passed.
Proptest retained a regression seed under `casper/proptest-regressions/rust/engine/block_retriever/`.
Compilation completed in 49.33 seconds, followed by 0.03 seconds of tests.
The source-hash check passed.

These tests establish current behavior and the required local repair contract.
They do not yet test a repaired production handoff API.

A final example verifies an exhausted production retry budget under an active quarantine before receipt.
It then fails because receipt replenishes that exhausted budget.
This distinguishes a live budget bypass from the generated example's expired minimum deadline.
`target/verification/buffer-publication/policy.nwLscT` records the latest four-test receipt result.
Three assertions failed as expected, and the authority/timestamp control passed.
Compilation completed in 21.91 seconds, followed by 0.03 seconds of tests.
The source-hash check passed.

The shell syntax check, focused test formatting check, and repository diff check also passed.
Their logs reside in the final formal evidence directory.

## Producer race and upstream distinction

The authorized plan agent confirmed a receipt-after-publication race in three feature paths:

1. Network delivery in `casper/src/rust/engine/running.rs`.
2. Startup recovery in `node/src/rust/instances/block_processor_instance/recovery_driver.rs`.
3. Ordinary recovery in the same recovery driver.

The worker can complete before the producer marks receipt.
A late receipt can overwrite retry readiness or recreate obsolete request tracking.
The proposed repair reserves identity, bytes, and a channel slot before a synchronous receipt callback and `permit.send`.
All three producers must use this boundary.
The existing proposal-demand and receipt-error tests must retain their behavioral assertions.

Current upstream `dev` does not call `ack_receive` in its network `Running::handle` delivery path.
Thus, the feature's network receipt callback is not identical to that upstream path.
Upstream startup recovery does call `ack_receive` after queue publication in `casper_launch.rs`.
The same delayed-receipt pattern therefore exists upstream for startup recovery.
Upstream receipt also clears auxiliary retry tracking.
These are source comparisons at `cdf447ac18710d9702a27379bce6c946f421be46`, not a full upstream runtime reproduction.

## Next implementation boundary

The new tests and models require these connected local changes:

1. Record receipt synchronously after successful queue reservation and before item visibility.
2. Preserve policy during receipt and local failure handoff.
3. Return explicit tracker-capacity refusal.
4. Retain the worker lease and retry publication when no successor owner exists.
5. Keep local storage retry separate from validation quarantine.
6. Add producer, callback-failure, duplicate, rejection, cancellation, and generated handoff-history tests.
7. Preserve all successful and authoritative terminal acknowledgement paths.

The larger pruning-storage decision remains separate.
This continuation changed formal sources, test sources, documentation, and the formal runner only.
It did not change production behavior, consensus rules, durable schema, or Git history.
