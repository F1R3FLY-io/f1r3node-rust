--------------------------- MODULE SpaceReplayOrder ---------------------------
\* Decision 92 (ordinary events): the replay order of the space transactions
\* of one Reified RSpace in racing native replay.
\*
\* Play serializes the transactions of a space sid, because each of them
\* locks the config channel C(sid). Racing native replay runs the jobs of sid
\* as concurrent tasks. A job stages its operations from the state of sid
\* that it sees. A job that commits in another state than the state of play
\* stages other operations, and replay then diverges, fails closed or stalls.
\*
\* The mechanism of decision 92:
\*   - Every row of a transaction of sid names the space counter S(sid) in
\*     its footprint. The predecessors of a row are the last row on each
\*     channel of its footprint, so the rows of sid form one chain in play
\*     order.
\*   - The first row of each committed transaction is unique to its job. A
\*     transaction whose first operation is shared stages an anchor first.
\*     The re-offer of a persistent record is shared, because it keeps the
\*     identity of the record (`spaces/settle.rs`). The anchor is a consume of
\*     the persistent config record on C(sid) with the random state of the
\*     job, so the record stays in place.
\*   - A transaction that stages no operation commits the anchor alone, and
\*     then fails or ends. A positional failure (CellOccupied) and a step that
\*     changes nothing are such transactions.
\*   - Before its first operation, under its locks, a transaction checks only
\*     its first staged row: the first open row of its source exists, and the
\*     predecessors of that row are complete. Otherwise the transaction is
\*     NotReady. It applies nothing, releases its locks, waits for a change
\*     and stages again. At commit, a later row that is not ready and a row
\*     whose recorded candidate differs fail closed.
\*
\* Why the first row is enough: when the first row is ready, every earlier
\* row of sid is complete and no later row is complete, because S(sid) chains
\* the rows. So sid is in the state that play gave the transaction, and the
\* staging repeats the staging of play. Every later row is then ready and
\* fits at commit. The first row is unique to its job, so the selected row is
\* the row of the job.
\*
\* Changed by decision 102 (plan v4, M3): play picks at random at some sites
\* (CR-1), so the state alone does not give the staging of play. The staging
\* follows the recorded rows. Operation 0 uses its first open row, and a later
\* operation uses the S(sid) successor rule: it takes the row of its source
\* whose S(sid) predecessor is the row of the operation before it. A pick site
\* puts the candidate that the followed row names first. When the first row is
\* ready, the followed staging repeats the staging of play. Changed by
\* decision 122 (plan v4, M3): a deviating transaction ends with the mark-free
\* finish. The Tail of its last committed row decides the finish, in play and
\* in replay. The premise of decision 92 above stays.
\*
\* Added by decision 122 (plan v4, M3): the model assumes D-OBL-1. Play and
\* replay read the same `TailOf` of each deviating job. T-C4-21 tests D-OBL-1.
\* A child job is absent until the finish of its parent creates it. Play runs
\* a parent before its children.
\*
\* Added by plan v4 erratum V4-12 (the anchor-only rule): the anchor that a
\* job stages first is operation 0 (CR-5). It is not a followed row for the
\* record end (CR-6) and for a staging error after following (CR-32). While
\* replay has followed no row after that anchor, two rules apply:
\*   - The error rule: a staging error keeps the path of `prepare_failure`.
\*   - The miss rule: a stage call without a successor row answers None. The
\*     rest of the transaction then stages in DR-75 mode.
\* In both cases play committed the anchor alone and failed, so replay repeats
\* the failure of play. Without the error rule, TLC finds a counterexample in
\* MC_SpaceReplayOrder_RaFirst.cfg and in MC_SpaceReplayOrder_ErrStop.cfg.
\* Without the miss rule, TLC finds one in MC_SpaceReplayOrder_ErrStop.cfg. In
\* each of them, replay ends the staging after the anchor, and the anchor
\* never satisfies CanStop.
\*
\* Changed by decision 124 (L2): play fires the cascade waiters in their DR-75
\* order, as the base fires them, so replay needs no pin. The pin of the least
\* journal index, the successor probes and the stage call when every probe
\* misses are retired.
\* Added by decision 102 (plan v4, rule CR-30), changed by decision 124 (L2):
\* the cascade. A cascade round fires a parked waiter of a dirty collection.
\* The first row of the round is the lift K_lift of the lead record, or the
\* re-offer of the lead when the lead is persistent. Play is lazy and adds no
\* row. The window of the cascade search and the waiter order both take the
\* DR-75 order (F3). Init chooses the DR-75 waiter order
\* (cascadeCase.waiterOrder), as it chooses canonFirst. So play and replay
\* read one order in each behavior, and both orders are reachable. Replay
\* checks the round cap first. Then it qualifies every waiter in DR-75 order
\* without a lease. The qualifying waiters form C, and replay fires the first
\* waiter C[1] through its own stage calls (CR-4 to CR-6). When that round
\* starts the job and the lead of C[1] is persistent, the chain anchor goes
\* first (C7.22). After a followed row, a stage call that finds no successor
\* row answers RecordEnded, so the staging ends there.
\*
\* Added by decision 124 (L3 and F2): the Stored check of an offer (base
\* E3-a, selection.rs:747-751). On a recorded Stored row of the offer, the
\* replay staging of a send skips each waiter whose commit may veto. A
\* PathMap offer records each fire under the vehicle of the element on the
\* fired prefix, and its store under the element itself. So the offer is
\* Stored only when the Stored row of the element comes before every open
\* row of its vehicles, as the one row source of a base produce gives the
\* play order (replay_rspace.rs:1058-1082). The rule reads the rows that
\* the stage calls read at the cursor, so it probes no successor and pins
\* no pick (L1 and L2 stay closed).
\*
\* Invariants:
\*   - NoStall: the replay of an honest journal never declares a stall.
\*   - NoFailClosed: the replay of an honest journal never fails closed.
\*   - Agreement: each job ends with its verdict of play. When replay is quiet
\*     and every row is complete, the space is in its final state of play.
\*     Changed by decision 122 (plan v4, V3-20).
\*   - ExactRows: when replay is quiet, every row is complete. Changed by
\*     decision 122 (plan v4, M3). Replay is quiet when every job has ended or
\*     is absent.
\*
\* Controls, each with one part of the mechanism removed:
\*   - NoFootprintKeyUnsafe (FootprintKey = FALSE): S(sid) is not in the
\*     footprints, so the anchor of a positional failure has no order with
\*     the rows of the cell. The failing put can then see an empty cell. It
\*     stages a store that has no row, and it waits forever: NoStall.
\*   - NoAnchorRowlessUnsafe (AnchorRowless = FALSE): a positional failure
\*     commits no row, so nothing orders it. It can see an empty cell and wait
\*     forever, as above: NoStall.
\*   - AnchorAppendedUnsafe (AnchorFirst = FALSE, the counterexample CE1): the
\*     anchor goes after a shared first operation. Another job then takes the
\*     row of the shared operation. Changed by decision 102 (plan v4, M3): the
\*     anchor of that job has no successor row, so the record end cuts the
\*     staging after the shared row, and CanStop fails at commit:
\*     NoFailClosed.
\*   - UnknownSourceFailsUnsafe (UnknownSourceWaits = FALSE): a first row
\*     without an open recorded row fails closed instead of NotReady. A job
\*     that stages from a stale state then rejects an honest journal:
\*     NoFailClosed.
\*   Added by decisions 102 and 122 (plan v4, M3):
\*   - CanonicalStagingUnsafe (pick, FollowRecord = FALSE): replay stages in
\*     DR-75 order. The put "pp" fires w1 where play fired w2: NoFailClosed.
\*   - EmptyFilterFailsUnsafe (pick, StaleWaits = FALSE): a stale "pp" names w2
\*     before w2 exists, and the job fails instead of NotReady: NoFailClosed.
\*   - DeviationRowsUnsafe (deviate, RecordEnd = FALSE): the staging of "td"
\*     goes on after its last recorded row, and CanContinue fails at commit:
\*     NoFailClosed.
\*   - ReissueOwnFirstUnsafe (k8, ReissueAnchorFirst = FALSE): the stop
\*     re-issue "rk8" stages no anchor. The replay of "tk8" takes the row of
\*     "rk8", and CanContinue fails: NoFailClosed.
\*   - NoCanonicalRestageUnsafe (capfail, CanonicalRestage = FALSE): play
\*     commits the anchor of "pc" after the failed pick t1. Replay stages t2,
\*     finds no row and waits forever: NoStall.
\*   - RestageDropsAnchorUnsafe (rafirst, RestageAnchorFirst = FALSE): the R-A
\*     restage of "rj" drops its anchor. The replay of "rt" takes the own row of
\*     "rj", and CanContinue fails: NoFailClosed.
\*   - MarkedFinishUnsafe (tailret, MarkFree = FALSE): the marked finish of play
\*     gives "tq2" an anchor. Replay creates "tq2" without an anchor, and "tq2"
\*     waits forever for the row of that anchor: NoStall.
\*   - MarkedSettleUnsafe (tailquiet, MarkFree = FALSE): play journals the
\*     anchor of the settle job "sj". Replay never creates "sj", so replay is
\*     quiet while a row is not complete: ExactRows.
\*   - ReofferOwnFirstUnsafe (reoffer0, ReofferAnchor = FALSE): the own re-offer
\*     of "j0" stages no anchor. The replay of "tr" takes the row of "j0", and
\*     CanContinue fails: NoFailClosed.
\*   - SeqPickAbortUnsafe (seqpick, SeqRestage = FALSE): play aborts "sq" with
\*     no row. Replay stages t2, finds no row and waits forever: NoStall.
\*   - FollowErrorFailsUnsafe (errstop, FollowErrorEnds = FALSE): replay of "te"
\*     discards its followed row after the staging error and stages an anchor
\*     that has no row, so "te" waits forever: NoStall.
\*   Added by plan v4 erratum V4-12 (the anchor-only rule):
\*   - AnchorOnlyErrorCutUnsafe (rafirst, AnchorOnlyErrorPath = FALSE): with
\*     the DR-75 order t1 first, play of "rj" fails twice and commits its
\*     anchor alone. In replay the pick takes t1, whose error after the
\*     followed anchor ends the staging. The anchor never satisfies CanStop,
\*     so the commit fails: NoFailClosed.
\*   - AnchorOnlyRecordEndUnsafe (errstop, AnchorOnlyMissDr75 = FALSE): in
\*     replay the counter take of "tej" after its followed anchor has no
\*     successor row. So the record end cuts the staging after the anchor,
\*     and CanStop fails: NoFailClosed.
\*   Added by decision 102 (plan v4, rule CR-30):
\*   Retired by decision 124 (L2): play and replay take the DR-75 waiter
\*   order, so replay pins no source, and CascadeLeastIndex is retired.
\*   - CascadeRandomWindowUnsafe (cascadeveto, CascadeCanonWindows = FALSE):
\*     the window of play takes either record, and replay takes the DR-75
\*     window. When t2 is first in DR-75 order and play takes t1, play fires
\*     nothing and commits the anchor alone. Replay qualifies wg and stages
\*     the lift of t2, which has no row. In the reverse case play fires wg,
\*     and replay stages the anchor alone, which has no row. In both cases
\*     "cv" waits forever: NoStall.
\*   Retired by decision 124 (L2): replay fires the first qualifying waiter
\*   with no probe, so no probe misses, and CascadeMissStages is retired.
\*   Added by decision 124 (L3 and F2):
\*   - OfferStoreSourceUnsafe (offersource, the Stored check that reads the
\*     element alone): play of "o" fires wg under the vehicle of x and then
\*     stores x. Replay finds the Stored row of x first, skips wg and stages
\*     the store. The store row waits on the fire row of its own job, so "o"
\*     waits forever: NoStall.
\* Witnesses: replay completes an honest journal (NoHonestCompletion), and
\* replay repeats a positional failure through its anchor (NoFailureVerdict).
\* Added by decisions 102 and 122 (plan v4, M3): replay ends a job whose play
\* committed after the R-A restage (NoRestagedCommit), and replay ends a
\* deviating job by the Return branch of its Tail (NoReturnAfterDeviation).
\* Added by decision 102 (plan v4, rule CR-30): replay completes an honest
\* journal of the scenario "cascade" (SpaceReplayCascadeCompletionReachable,
\* NoHonestCompletion).
\*
\* The code of decision 92 that this model checks:
\*   - The footprint key: `SpaceEnv::begin_phase_a` (rholang,
\*     `spaces/agent.rs`) sets S(sid). `apply_produce` and `apply_consume`
\*     (rspace++, `rspace/space_play.rs`) record it as one more join group,
\*     and native replay authenticates the same footprint (rspace++,
\*     `replay_rspace/native_session/space.rs`).
\*   - The anchor and its place: `spaces/anchor.rs`, `run_with` and
\*     `run_plan` (`spaces/generic.rs`), and the guard of the own first row in
\*     `commit_staged` (rspace++, `space_store_handle.rs`).
\*   - First-row readiness: `check_ready` (`native_session/space.rs`) and
\*     `check_space_transaction` (rholang,
\*     `trace/replay/session/execution/space_readiness.rs`). Replay reaches
\*     the check through `NativeOperationEpoch::space_transaction_ready`
\*     (rspace++, `replay_rspace/native_epoch.rs`, decision 95). The
\*     fail-closed check of a later row is `begin_row`
\*     (`native_session/space.rs`).
\*   - The invariants on the code: the racing tests `space_racing_replay_*`
\*     (rholang, `tests/reified_native_spaces.rs`), one test for each
\*     invariant.
\*   - Added by decision 124 (L3 and F2): the Stored check of an offer: the
\*     E3-a arm of `Engine::offer` and `JoinSource::offer_stored` (rholang,
\*     `spaces/join.rs`), its PathMap override (`spaces/pathmap/join.rs`),
\*     `Cx::recorded_first` (`spaces/generic.rs`) and
\*     `SpaceTxn::recorded_row_index` (rspace++, `space_store_handle.rs`).
\*     The test on the code is `space_theory_pathmap_peek_then_send_certifies`
\*     (rholang, `tests/reified_theory.rs`).
\*
\* Abstractions:
\*   - One space sid. Every transaction of sid locks C(sid), so each attempt
\*     of a job in replay is atomic: it stages from the current state and then
\*     commits, fails closed or parks. JoinScopeLock.tla checks the locks.
\*   - The state of sid is a cell on the channel D and the waiters that a
\*     persistent record R on the channel E can fire. The jobs "pa" and "pb"
\*     put a record into the cell, and the job "tk" takes one. A settle job
\*     ("sx", "sy" or "sz") re-offers R to the head waiter. The anchor is on
\*     the channel C.
\*   - A source is a pair <<owner, kind>>. An own source names its job, and a
\*     re-offer names the record R.
\*   - The candidate of a row is the other side of its COMM: the stored
\*     record that the take fires, the waiter that a put or a re-offer fires,
\*     or the config record of an anchor. A store or a park has no candidate.
\*   - The wait is the counted wait of the base. NativeReplayStall.tla checks
\*     its protocol. A commit that completes a row wakes every parked job.
\*     Replay declares a stall when every job that has not ended is parked.
\*   - Play runs the jobs in every order. The controls on the cell use one
\*     order, so that each of them breaks exactly one invariant.
\*   Added by decisions 102 and 122 (plan v4, M3):
\*   - The bag channel F holds waiters. A put on F fires one of them, and play
\*     picks it at random. The collection V holds the records t1 and t2, and a
\*     pick on V takes one of them. Its DR-75 order is fixed, except in the
\*     scenarios "capfail" and "rafirst", where Init chooses it. Changed by
\*     plan v4 erratum V4-12 (the anchor-only rule): "rafirst" also has both
\*     orders.
\*   - A row carries its stop value. "no": the transaction did not deviate
\*     there, so only CanContinue holds. "must": a deviation that replay sees,
\*     so only CanStop holds. "may": the ambiguous case, so both hold.
\*   - A deviating own operation runs on the channel G. Its state change does
\*     not matter here, so it changes nothing.
\*   - A stop re-issue keeps the random state of its parent. Its first own
\*     operation is the operation that the stopped transaction would stage
\*     next, so it has the source of that operation (CR-19 case 2).
\*   - The R-A restage runs in play only (CR-14). Replay never restages.
\*   - The scenario "seqpick" has one play order, because "sc" commits first.
\*   Added by decision 102 (plan v4, rule CR-30):
\*   - In the scenarios "cascade" and "cascadeveto", the collection V is a
\*     set collection. The field cw of the state holds its parked waiters. A
\*     round removes the waiter that it fires. The space state has no other
\*     change.
\*   Changed by decision 124 (L2): cascadeCase also gives the DR-75 order of
\*   the waiters.
\*   - "cascade": the waiter wi has the lead record vi. The lift of vi has the
\*     source <<"cs", "lifti">>, and the chain anchor has the source
\*     <<"cs", "anchor">>. The jobs "cs", "cs2c" and "cs2s" share these
\*     sources. The re-offer of vi has the source <<"vi", "prod">>. The
\*     variable cascadeCase gives the round cap (capOne: one round, otherwise
\*     two), the persistence of v2 (persist2) and the DR-75 order of the
\*     waiters w1 and w2 (waiterOrder). Init chooses each of them.
\*   - "cascadeveto": V holds the records t1 and t2, and the guard of the
\*     waiter wg admits only t2. Init chooses the DR-75 order of V.
\*   Changed by decision 124 (L2): replay pins no source.
\*   - Each lead has one waiter, so a round fires one candidate. The model does
\*     not check how replay names one waiter of a lead with two or more
\*     waiters (the names_waiter rule).
\*   Added by decision 124 (L3 and F2): the offer scenarios.
\*   - One job "o" sends the element x on a path of a PathMap space. The
\*     waiter wg waits on the index channel I of a prefix of the path, and
\*     cw holds it. The commit of wg may veto, through its guard or the
\*     theory of the space.
\*   - Play fires wg, or the guard of wg refuses x. Both verdicts are
\*     reachable. A fire stages the vehicle of x on I (the source
\*     <<"o", "veh">>) and then the store of x on D (the source
\*     <<"o", "prod">>), because wg peeks. A refusal stages the store alone.
\*   - The guard is pure, so replay fires wg when a row of the vehicle names
\*     it. In a bag or a set space the markers and the token bump come after
\*     the store (`land` and `finish`), so the model leaves them out.
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    \* Changed by decisions 102 and 122 (plan v4, M3): ten scenarios more.
    \* "mixed", "reoffers" and the scenarios of decisions 102 and 122 for the
    \* safe configurations, "cellstall" and "settle" for the controls of
    \* decision 92.
    \* Added by decision 102 (plan v4, rule CR-30): the scenarios "cascade"
    \* and "cascadeveto" of the cascade lookahead.
    \* Added by decision 124 (L3 and F2): the scenarios "offerstore" (safe)
    \* and "offersource" (control) of the Stored check of an offer.
    Scenario,
    \* Every row of the space names S(sid) in its footprint.
    FootprintKey,
    \* A transaction that stages no operation commits the anchor.
    AnchorRowless,
    \* The anchor goes before a shared first operation. Otherwise it goes
    \* after the last operation.
    AnchorFirst,
    \* A first row without an open recorded row is NotReady. Otherwise it
    \* fails closed.
    UnknownSourceWaits,
    \* Added by decision 102 (plan v4, M3): the staging follows the recorded
    \* rows (CR-4, CR-5). Otherwise replay always stages in DR-75 order.
    FollowRecord,
    \* Added by decision 102 (plan v4, M3): a pick site whose followed row
    \* names a candidate that is not there takes the first allowed candidate
    \* in DR-75 order. Otherwise the job fails closed.
    StaleWaits,
    \* Added by decision 102 (plan v4, M3): a stage call whose source has no
    \* successor row ends the staging (CR-6). Otherwise the staging goes on.
    RecordEnd,
    \* Added by decision 122 (plan v4, M3): a stop re-issue stages its anchor
    \* first (CR-19 case 2).
    ReissueAnchorFirst,
    \* Added by decision 102 (plan v4, M3): the R-A restage of play (CR-14).
    CanonicalRestage,
    \* Added by decision 102 (plan v4, F6): the R-A restage stages the anchor
    \* first again when the first staging did (CR-14 step 7).
    RestageAnchorFirst,
    \* Added by decision 122 (plan v4, U4 (B)): the finish of play is mark-free
    \* (CR-31). Otherwise play builds the job of C4v2 B.6 with an anchor first
    \* for every deviation.
    MarkFree,
    \* Added by decision 122 (plan v4, F22): a job whose first operation is its
    \* own re-offer of a stored persistent record stages its anchor first
    \* (CR-19 case 3).
    ReofferAnchor,
    \* Added by decision 122 (plan v4, U5): the R-A restage also runs after
    \* `SeqConcurrent` (CR-14).
    SeqRestage,
    \* Added by decision 122 (plan v4, CR-32): a staging error after a followed
    \* row ends the staging. Otherwise replay keeps the path of
    \* `prepare_failure`. Changed by plan v4 erratum V4-12: the text names the
    \* path of `prepare_failure`.
    FollowErrorEnds,
    \* Added by plan v4 erratum V4-12 (the error rule): while replay has
    \* followed no row after the leading anchor of the job, a staging error
    \* keeps the path of `prepare_failure`. Otherwise it ends the staging
    \* (CR-32).
    AnchorOnlyErrorPath,
    \* Added by plan v4 erratum V4-12 (the miss rule): while replay has
    \* followed no row after the leading anchor of the job, a stage call
    \* without a successor row answers None and puts the rest of the
    \* transaction in DR-75 mode (CR-5). Otherwise it answers RecordEnded
    \* (CR-6).
    \* Changed by decision 102 (plan v4, rule CR-30): more constants follow,
    \* so this constant takes a comma.
    \* AnchorOnlyMissDr75
    AnchorOnlyMissDr75,
    \* Added by decision 102 (plan v4, rule CR-30): at an empty staging, the
    \* cascade pin is the source of U with the least journal index (F1).
    \* Otherwise replay pins the first source of U with an open row, in the
    \* fixed order <<1, 2>>.
    \* Retired by decision 124 (L2): play and replay take the DR-75 waiter
    \* order (cascadeCase.waiterOrder), so replay pins no source.
    \* CascadeLeastIndex,
    \* Added by decision 102 (plan v4, rule CR-30): the window of the cascade
    \* search in play takes the DR-75 order (F3). Otherwise play picks either
    \* record of the window.
    \* Changed by decision 124 (L2): CascadeMissStages is retired, so this
    \* constant is the last one and takes no comma.
    \* CascadeCanonWindows,
    CascadeCanonWindows
    \* Added by decision 102 (plan v4, rule CR-30): when every cascade probe
    \* misses, replay fires the first qualifying waiter through its own stage
    \* call. Otherwise the miss skips the cascade ("continue").
    \* Retired by decision 124 (L2): replay fires the first qualifying waiter
    \* with no probe, so no probe misses.
    \* CascadeMissStages

\* Changed by decisions 102 and 122 (plan v4, M3): the new scenarios and flags.
ASSUME
    \* Changed by decision 102 (plan v4, rule CR-30): the scenarios "cascade"
    \* and "cascadeveto".
    \* Added by decision 124 (L3 and F2): the scenarios "offerstore" and "offersource".
    /\ Scenario \in {"mixed", "reoffers", "cellstall", "settle", "pick", "deviate",
                     "capfail", "k8", "rafirst", "tailret", "tailquiet", "reoffer0",
                     "seqpick", "errstop", "cascade", "cascadeveto", "offerstore",
                     "offersource"}
    /\ FootprintKey \in BOOLEAN
    /\ AnchorRowless \in BOOLEAN
    /\ AnchorFirst \in BOOLEAN
    /\ UnknownSourceWaits \in BOOLEAN
    /\ FollowRecord \in BOOLEAN
    /\ StaleWaits \in BOOLEAN
    /\ RecordEnd \in BOOLEAN
    /\ ReissueAnchorFirst \in BOOLEAN
    /\ CanonicalRestage \in BOOLEAN
    /\ RestageAnchorFirst \in BOOLEAN
    /\ MarkFree \in BOOLEAN
    /\ ReofferAnchor \in BOOLEAN
    /\ SeqRestage \in BOOLEAN
    /\ FollowErrorEnds \in BOOLEAN
    \* Added by plan v4 erratum V4-12 (the anchor-only rule).
    /\ AnchorOnlyErrorPath \in BOOLEAN
    /\ AnchorOnlyMissDr75 \in BOOLEAN
    \* Added by decision 102 (plan v4, rule CR-30): the cascade lookahead.
    \* Retired by decision 124 (L2): replay pins no source, so the constant
    \* CascadeLeastIndex is retired.
    /\ CascadeCanonWindows \in BOOLEAN
    \* Retired by decision 124 (L2): replay makes no probe, so the constant
    \* CascadeMissStages is retired.

VARIABLES
    \* The rows that play recorded, in journal order.
    journal,
    \* The verdict of each job in play.
    pverdict,
    \* The state of the space after play.
    pfinal,
    \* Added by decision 102 (plan v4, M3): the choice of each job in play.
    pchoice,
    \* Added by decision 102 (plan v4, M3): the first record of V in DR-75
    \* order.
    canonFirst,
    \* Added by decision 102 (plan v4, rule CR-30): the case of the scenario
    \* "cascade". capOne: the round cap is one round. persist2: the lead v2
    \* is persistent.
    \* Added by decision 124 (L2): waiterOrder: the DR-75 order of the
    \* waiters w1 and w2 (the candidates 1 and 2), <<1, 2>> or <<2, 1>>. Play
    \* fires the waiters in this order, and replay qualifies them in it.
    cascadeCase,
    \* The state of the space in replay.
    rsigma,
    \* The complete rows.
    done,
    \* Changed by decision 122 (plan v4, M3): each job in replay: "run",
    \* "parked", "ended" or "absent".
    status,
    \* The verdict of each job that ended in replay.
    rverdict,
    \* Replay declared a stall.
    stalled,
    \* Replay failed closed.
    failed

\* Changed by decision 102 (plan v4, V3-20): vars names pchoice and canonFirst.
\* Changed by decision 102 (plan v4, rule CR-30): vars names cascadeCase.
vars ==
    <<journal, pverdict, pfinal, pchoice, canonFirst, cascadeCase, rsigma, done, status,
      rverdict, stalled, failed>>

\* ---------------------------------------------------------------------
\* The jobs
\* ---------------------------------------------------------------------

Puts == {"pa", "pb"}
Waiters == {"w1", "w2"}

\* Added by decision 102 (plan v4, M3): the jobs of decision 92, which stage
\* with `Stage`.
OldJobs == {"pa", "pb", "tk", "sx", "sy", "sz"}

\* Changed by decisions 102 and 122 (plan v4, M3): the jobs of the new
\* scenarios.
Jobs ==
    CASE Scenario = "mixed" -> {"pa", "pb", "tk", "sx", "sy"}
      [] Scenario = "reoffers" -> {"sx", "sy", "sz"}
      [] Scenario = "cellstall" -> {"pa", "pb", "tk"}
      [] Scenario = "settle" -> {"sx", "sy"}
      [] Scenario = "pick" -> {"k2", "pp"}
      [] Scenario = "deviate" -> {"td", "rd"}
      [] Scenario = "capfail" -> {"pc"}
      [] Scenario = "k8" -> {"tk8", "rk8"}
      [] Scenario = "rafirst" -> {"rt", "rj"}
      [] Scenario = "tailret" -> {"tq", "tq2"}
      [] Scenario = "tailquiet" -> {"td2", "sj"}
      [] Scenario = "reoffer0" -> {"tr", "j0", "trj"}
      [] Scenario = "seqpick" -> {"sc", "sq"}
      [] Scenario = "errstop" -> {"te", "tej"}
      \* Added by decision 102 (plan v4, rule CR-30): the jobs of the cascade
      \* scenarios.
      [] Scenario = "cascade" -> {"cs", "cs2c", "cs2s"}
      [] Scenario = "cascadeveto" -> {"cv"}
      \* Added by decision 124 (L3 and F2): the job of the offer scenarios.
      [] Scenario \in {"offerstore", "offersource"} -> {"o"}

\* Changed by decision 122 (plan v4, M3): R has one waiter in "k8" and in
\* "reoffer0", and none in the other new scenarios.
\* The waiters that R can fire at the start, in head order.
InitHeads ==
    CASE Scenario = "reoffers" -> <<"w1", "w2">>
      [] Scenario \in {"mixed", "settle", "k8", "reoffer0"} -> <<"w1">>
      [] OTHER -> <<>>

\* Added by decision 102 (plan v4, M3): the waiters on the bag channel F at the
\* start.
InitBag == IF Scenario = "pick" THEN {"w1"} ELSE {}

\* Added by decision 102 (plan v4, rule CR-30): the parked waiters of the set
\* collection V that a cascade can fire, at the start. w1 and w2 wait in
\* "cascade". The guarded waiter wg waits in "cascadeveto".
\* Added by decision 124 (L3 and F2): wg also waits in "offerstore" and
\* "offersource", on the index channel I of a prefix.
CascadeWaiters == {"w1", "w2", "wg"}
InitCw ==
    CASE Scenario = "cascade" -> {"w1", "w2"}
      [] Scenario = "cascadeveto" -> {"wg"}
      \* Added by decision 124 (L3 and F2): the offer scenarios.
      [] Scenario \in {"offerstore", "offersource"} -> {"wg"}
      [] OTHER -> {}

Kind(j) == IF j \in Puts THEN "put" ELSE IF j = "tk" THEN "take" ELSE "settle"

\* Added by decision 122 (plan v4, V3-20): the parent that creates each child
\* job at its finish. "j0" belongs to another chain and has no parent.
ParentOf(j) ==
    CASE j = "rd" -> "td"
      [] j = "rk8" -> "tk8"
      [] j = "rj" -> "rt"
      [] j = "tq2" -> "tq"
      [] j = "sj" -> "td2"
      [] j = "trj" -> "tr"
      [] j = "tej" -> "te"
      \* Added by decision 102 (plan v4, rule CR-30): the cap job "cs2c" and
      \* the stop re-issue "cs2s" of "cs".
      [] j = "cs2c" -> "cs"
      [] j = "cs2s" -> "cs"
      [] OTHER -> "none"

Children == {j \in Jobs : ParentOf(j) /= "none"}

\* The play orders.
JobCount == Cardinality(Jobs)
EveryOrder ==
    {f \in [1..JobCount -> Jobs] : \A a, b \in 1..JobCount : a /= b => f[a] /= f[b]}

\* Added by decision 122 (plan v4, V3-20): a play order runs each parent
\* before its child.
ParentFirst(f) == \A a, b \in 1..JobCount : ParentOf(f[b]) = f[a] => a < b

\* Changed by decision 122 (plan v4, V3-20): play runs a parent first. The
\* scenario "seqpick" has one order, because "sc" commits first.
Orders ==
    CASE Scenario = "cellstall" -> {<<"pa", "pb", "tk">>}
      [] Scenario = "seqpick" -> {<<"sc", "sq">>}
      [] OTHER -> {f \in EveryOrder : ParentFirst(f)}

\* ---------------------------------------------------------------------
\* Staging
\* ---------------------------------------------------------------------

\* The candidate of a store or a park.
None == <<"none", "none">>
\* The persistent config record on C(sid).
ConfigRecord == <<"config", "prod">>
\* The identity of the persistent record R on E.
RecordR == <<"R", "prod">>

\* Changed by decision 122 (plan v4, V3-20): an operation carries its stop
\* value.
Op(src, chan, cand) == [src |-> src, chan |-> chan, cand |-> cand, stop |-> "no"]

\* Added by decision 122 (plan v4, M3): the operation op deviates in play with
\* the stop value stop.
Deviated(op, stop) == [op EXCEPT !.stop = stop]

\* The anchor of the job j: a consume of the config record with the random
\* state of j.
AnchorOp(j) == Op(<<j, "anchor">>, "C", ConfigRecord)

\* Added by decision 122 (plan v4, M3): the operations ops of the job j, with
\* the anchor of j first when anchored holds.
Anchored(anchored, j, ops) == IF anchored THEN <<AnchorOp(j)>> \o ops ELSE ops

\* Changed by decisions 102 and 122 (plan v4, M3): the state also holds the
\* waiters on F, the record of V that a pick took, and the stored claim of a
\* seq space.
\* Changed by decision 102 (plan v4, rule CR-30): the state also holds the
\* parked waiters of the cascade.
InitSigma ==
    [cell |-> "empty", heads |-> InitHeads, bag |-> InitBag, v |-> "open", claim |-> FALSE,
     cw |-> InitCw]

\* Changed by decision 122 (plan v4, V3-20): a staging names the child job
\* that its finish creates, or "none", and the choice of play, or "none".
Staging(ops, s, verdict) ==
    [ops |-> ops, sigma |-> s, verdict |-> verdict, child |-> "none", choice |-> "none"]

\* Added by decision 122 (plan v4, M3): the staging st with the play choice c,
\* and the staging st whose finish creates the child job k.
WithChoice(st, c) == [st EXCEPT !.choice = c]
WithChild(st, k) == [st EXCEPT !.child = k]

\* A transaction that stages no operation: a positional failure, or a step
\* that changes nothing. It leaves the state unchanged.
Rowless(j, s, verdict) ==
    Staging(IF AnchorRowless THEN <<AnchorOp(j)>> ELSE <<>>, s, verdict)

\* A transaction whose first operation is shared.
SharedFirst(j, ops, s, verdict) ==
    Staging(IF AnchorFirst THEN <<AnchorOp(j)>> \o ops ELSE ops \o <<AnchorOp(j)>>,
            s, verdict)

\* The staging of the job j in the state s: its operations, the next state
\* and its verdict.
\*   - A put stores its record in an empty cell, fires the waiting take, or
\*     fails with CellOccupied (a positional failure).
\*   - The take fires the stored record, or it parks in an empty cell.
\*   - A settle job re-offers R to the head waiter, or it changes nothing.
Stage(j, s) ==
    IF Kind(j) = "put"
      THEN IF s.cell = "empty"
             THEN Staging(<<Op(<<j, "prod">>, "D", None)>>, [s EXCEPT !.cell = j], "stored")
           ELSE IF s.cell = "waiter"
             THEN Staging(<<Op(<<j, "prod">>, "D", <<"tk", "cons">>)>>,
                          [s EXCEPT !.cell = "empty"], "fired")
           ELSE Rowless(j, s, "occupied")
    ELSE IF Kind(j) = "take"
      THEN IF s.cell \in Puts
             THEN Staging(<<Op(<<j, "cons">>, "D", <<s.cell, "prod">>)>>,
                          [s EXCEPT !.cell = "empty"], s.cell)
           ELSE Staging(<<Op(<<j, "cons">>, "D", None)>>,
                        [s EXCEPT !.cell = "waiter"], "parked")
    ELSE IF s.heads /= <<>>
      THEN SharedFirst(j, <<Op(RecordR, "E", <<Head(s.heads), "cons">>)>>,
                       [s EXCEPT !.heads = Tail(s.heads)], Head(s.heads))
    ELSE Rowless(j, s, "quiescent")

\* ---------------------------------------------------------------------
\* The operations of the new scenarios (decisions 102 and 122, plan v4, M3)
\* ---------------------------------------------------------------------

\* The first waiter of the set b in DR-75 order.
FirstWaiter(b) == IF "w1" \in b THEN "w1" ELSE "w2"

\* "k2" parks w2 on F. "pp" puts a datum on F that fires a waiter there.
ParkW2 == Op(<<"k2", "cons">>, "F", None)
PutBag(w) == Op(<<"pp", "prod">>, "F", <<w, "cons">>)

\* The own operations a and b of a job on G.
OwnA(j) == Op(<<j, "a">>, "G", None)
OwnB(j) == Op(<<j, "b">>, "G", None)

\* Added by plan v4 erratum V4-12: the counter take of the job j on the
\* counter channel N. A discipline with a counter stages it before the plan
\* of each operation (`stage_op` in generic.rs).
CountOp(j) == Op(<<j, "count">>, "N", None)

\* An operation with the source src that takes the record t of V.
TakeV(src, t) == Op(src, "V", <<t, "prod">>)

\* The re-offer of R to the head waiter of the state s.
ReofferR(s) == Op(RecordR, "E", <<Head(s.heads), "cons">>)

\* The persistent send of "tq" fires w1 on G, and its re-issue "tq2" stores.
SendTq == Op(<<"tq", "prod">>, "G", <<"w1", "cons">>)
StoreTq2 == Op(<<"tq2", "prod">>, "G", None)

\* The store of the claim of "sc" in the seq space.
ClaimSc == Op(<<"sc", "op">>, "V", None)

\* A stop re-issue of play has an anchor first (CR-19 case 2). The marked
\* finish gives every job of C4v2 B.6 an anchor first.
ReissueAnchor == ReissueAnchorFirst \/ ~MarkFree

\* The child job of a Return branch: none with the mark-free finish, and the
\* job k of C4v2 B.6 with the marked finish.
MarkedChild(k) == IF MarkFree THEN "none" ELSE k

\* The staging of the job j whose first own operation re-offers R to the head
\* waiter, with the anchor first when anchored holds. With no waiter it
\* stages nothing.
ReofferStaging(j, anchored, s) ==
    IF s.heads /= <<>>
      THEN Staging(Anchored(anchored, j, <<ReofferR(s)>>),
                   [s EXCEPT !.heads = Tail(s.heads)], Head(s.heads))
      ELSE Rowless(j, s, "quiescent")

\* The pick of "pc" on V. t1 fails with CapacityExceeded, and t2 fires. After
\* t1, the R-A restage stages in DR-75 order. A second failure commits the
\* anchor alone.
PcStagings(s) ==
    {WithChoice(Staging(<<TakeV(<<"pc", "op">>, "t2")>>, [s EXCEPT !.v = "t2"], "fired"), "t2"),
     IF CanonicalRestage /\ canonFirst = "t2"
       THEN WithChoice(Staging(<<TakeV(<<"pc", "op">>, "t2")>>, [s EXCEPT !.v = "t2"], "fired"),
                       "restaged")
       ELSE WithChoice(Rowless("pc", s, "capacity"), "t1")}

\* The stop re-issue "rj" of "rt": an anchor, then the next operation of "rt",
\* which picks a record of V. t1 fails with CapacityExceeded, and t2 fires.
RjStagings(s) ==
    {WithChoice(Staging(Anchored(ReissueAnchor, "rj", <<TakeV(<<"rt", "b">>, "t2")>>),
                        [s EXCEPT !.v = "t2"], "fired"), "t2"),
     IF CanonicalRestage /\ canonFirst = "t2"
       THEN WithChoice(Staging(Anchored(ReissueAnchor /\ RestageAnchorFirst, "rj",
                                        <<TakeV(<<"rt", "b">>, "t2")>>),
                               [s EXCEPT !.v = "t2"], "fired"), "restaged")
       ELSE WithChoice(Rowless("rj", s, "capacity"), "t1")}

\* The pick of "sq" on V in a seq space. t1 conflicts with the stored claim of
\* "sc" (`SeqConcurrent`). The R-A restage then stages in DR-75 order.
\* Without it, the transaction aborts and applies nothing.
SqStagings(s) ==
    {WithChoice(Staging(<<TakeV(<<"sq", "op">>, "t2")>>, [s EXCEPT !.v = "t2"], "fired"), "t2"),
     IF ~s.claim
       THEN WithChoice(Staging(<<TakeV(<<"sq", "op">>, "t1")>>, [s EXCEPT !.v = "t1"], "fired"),
                       "t1")
     ELSE IF SeqRestage /\ canonFirst = "t2"
       THEN WithChoice(Staging(<<TakeV(<<"sq", "op">>, "t2")>>, [s EXCEPT !.v = "t2"], "fired"),
                       "restaged")
     ELSE WithChoice(Staging(<<>>, s, "aborted"), "t1")}

\* ---------------------------------------------------------------------
\* The cascade (decision 102, plan v4, rule CR-30)
\* ---------------------------------------------------------------------

\* Added by decision 102 (plan v4, rule CR-30): the chain of the job j. The
\* jobs of one chain share the random state, so they share the lift sources
\* and the anchor (`anchor.rs`).
ChainOf(j) == IF j = "cv" THEN "cv" ELSE "cs"

\* The anchor of the chain of the job j.
ChainAnchor(j) == Op(<<ChainOf(j), "anchor">>, "C", ConfigRecord)

\* The lead record, the waiter and the persistence of the candidate i of the
\* job j. In "cascade" the waiter wi has the lead vi. In "cascadeveto" the
\* candidate 2 is the waiter wg with the lead t2.
LeadRecord(j, i) == IF ChainOf(j) = "cv" THEN "t2" ELSE IF i = 1 THEN "v1" ELSE "v2"
LeadWaiter(j, i) == IF ChainOf(j) = "cv" THEN "wg" ELSE IF i = 1 THEN "w1" ELSE "w2"
LeadPersistent(j, i) == ChainOf(j) = "cs" /\ i = 2 /\ cascadeCase.persist2

\* The lift K_lift of the lead of the candidate i: a consume of the lead with
\* the lift random state of the chain.
LiftOp(j, i) ==
    Op(<<ChainOf(j), IF i = 1 THEN "lift1" ELSE "lift2">>, "V", <<LeadRecord(j, i), "prod">>)

\* The re-offer of the lead of the candidate i. It fires the waiter of the
\* candidate, and it keeps the identity of the lead.
ReofferOp(j, i) == Op(<<LeadRecord(j, i), "prod">>, "V", <<LeadWaiter(j, i), "cons">>)

\* The operations of the round that fires the candidate i. A persistent lead
\* stays in place, so its round has no lift.
RoundOps(j, i) ==
    IF LeadPersistent(j, i) THEN <<ReofferOp(j, i)>> ELSE <<LiftOp(j, i), ReofferOp(j, i)>>

\* The first source of the round of the candidate i.
\* Retired by decision 124 (L2): no use after the pin retired.

\* The anchor that goes before the round of the candidate i when the round
\* starts the job. The re-offer of a stored persistent record is not unique
\* to the job (CR-19 case 3, C7.22).
LeadAnchor(j, i) == IF ReofferAnchor /\ LeadPersistent(j, i) THEN <<ChainAnchor(j)>> ELSE <<>>

\* The operations ops with the stop value stop on the last operation.
DeviateLast(ops, stop) == [ops EXCEPT ![Len(ops)] = Deviated(ops[Len(ops)], stop)]

\* The state s after the round of the candidate i fired its waiter.
FireCw(s, j, i) == [s EXCEPT !.cw = s.cw \ {LeadWaiter(j, i)}]

\* The candidate of "cs2c" and of "cs2s": the waiter that "cs" did not fire.
RestIndex(s) == IF "w1" \in s.cw THEN 1 ELSE 2

\* A cascade job that fires nothing commits the chain anchor alone.
CascadeRowless(j, s) == Staging(IF AnchorRowless THEN <<ChainAnchor(j)>> ELSE <<>>, s, "quiescent")

\* The stagings of the cascade jobs in play. Play is lazy and adds no row.
\* Changed by decision 124 (L2): "cs" takes the DR-75 waiter order that Init
\* chose, not a random order, as the base cascade does.
\*   - "cs" takes the DR-75 waiter order o = cascadeCase.waiterOrder, and it
\*     fires o[1] first. With capOne the cap ends "cs" after one round, and
\*     its finish creates the cap job "cs2c". Otherwise "cs" fires both
\*     waiters, or the re-offer of round 1 deviates with the Tail Stop and
\*     the finish creates the stop re-issue "cs2s".
\*   - "cs2c" fires the other waiter. It has no anchor unless its lead is
\*     persistent. "cs2s" fires the other waiter with the chain anchor first
\*     (CR-19 case 2).
\*   - The guard of wg admits only t2. With CascadeCanonWindows, the window of
\*     "cv" takes canonFirst. Otherwise it takes either record. A t1 pick
\*     fires nothing, so "cv" commits the chain anchor alone.
CascadePlay(j, s) ==
    CASE j = "cs" ->
           UNION {LET pre == LeadAnchor(j, o[1])
                      s1 == FireCw(s, j, o[1])
                  IN IF cascadeCase.capOne
                       THEN {WithChoice(WithChild(Staging(pre \o RoundOps(j, o[1]), s1, "capped"),
                                                  "cs2c"),
                                        "cap")}
                       ELSE {WithChoice(Staging(pre \o RoundOps(j, o[1]) \o RoundOps(j, o[2]),
                                                FireCw(s1, j, o[2]), "done"),
                                        "all"),
                             WithChoice(WithChild(Staging(pre \o DeviateLast(RoundOps(j, o[1]), "must"),
                                                          s1, "stopped"),
                                                  "cs2s"),
                                        "stop")}
                  \* Changed by decision 124 (L2): the DR-75 waiter order that Init
                  \* chose, as replay reads it in CascadeCands.
                  \* : o \in {<<1, 2>>, <<2, 1>>}}
                  : o \in {cascadeCase.waiterOrder}}
      [] j = "cs2c" ->
           LET k == RestIndex(s)
           IN {Staging(LeadAnchor(j, k) \o RoundOps(j, k), FireCw(s, j, k), "done")}
      [] j = "cs2s" ->
           LET k == RestIndex(s)
               pre == IF ReissueAnchor THEN <<ChainAnchor(j)>> ELSE LeadAnchor(j, k)
           IN {Staging(pre \o RoundOps(j, k), FireCw(s, j, k), "done")}
      [] j = "cv" ->
           {IF t = "t2"
              THEN WithChoice(Staging(RoundOps(j, 2), FireCw(s, j, 2), "done"), "t2")
              ELSE WithChoice(CascadeRowless(j, s), "t1")
            : t \in (IF CascadeCanonWindows THEN {canonFirst} ELSE {"t1", "t2"})}

\* ---------------------------------------------------------------------
\* The Stored check of an offer (decision 124, L3 and F2)
\* ---------------------------------------------------------------------

\* Added by decision 124 (L3 and F2): the rule of the offer staging. In
\* "offerstore" an offer is Stored only when the Stored row of the element
\* comes before every open row of its vehicle (`JoinSource::offer_stored`,
\* `Cx::recorded_first`). In the control "offersource" it reads the row of
\* the element alone, as the L3 arm did before F2.
OfferStoredFirst == Scenario = "offerstore"

\* The fire of the guarded waiter wg by the vehicle of x on the index
\* channel I, and the store of x on its data channel D.
VehFire == Op(<<"o", "veh">>, "I", <<"wg", "cons">>)
StoreX == Op(<<"o", "prod">>, "D", None)

\* The stagings of "o" in play. wg peeks, so after the fire x goes on and
\* is stored. A refusal by the guard of wg stores x alone.
OfferFire(s) == Staging(<<VehFire, StoreX>>, [s EXCEPT !.cw = s.cw \ {"wg"}], "fired")
OfferStore(s) == Staging(<<StoreX>>, s, "stored")

\* Play reads no row, so it never skips. The guard of wg admits x or
\* refuses it, and both verdicts are reachable.
OfferPlay(s) == IF "wg" \in s.cw THEN {OfferFire(s), OfferStore(s)} ELSE {OfferStore(s)}

\* Added by decisions 102 and 122 (plan v4, M3): the stagings of the job j in
\* the state s in play. A job of decision 92 has one staging.
\*   - "td" commits both own operations, or it deviates at row 0 with the
\*     Tail Stop (re-issue "rd") or Return.
\*   - "tk8", "rt", "tr" and "te" deviate at row 0 with the Tail Stop. Their
\*     stop re-issues are "rk8", "rj", "trj" and "tej".
\*   - "tq" deviates at row 0 with stop "may". Its Tail is Return of the
\*     DivergentPair re-issue "tq2", which has no anchor.
\*   - "td2" deviates at row 0 with stop "may". Its Tail is Return(None).
\*   - "j0" re-offers R as its own first operation.
\*   - The next operation of "te" is a store that fails with
\*     CapacityExceeded before its stage call, so "tej" fails at the same
\*     place and commits its anchor alone. Added by plan v4 erratum V4-12:
\*     "tej" stages its counter take before the store, and `prepare_failure`
\*     drops the take, so the anchor stays alone.
\*   - Added by decision 124 (L3 and F2): the offer job "o" of the scenarios
\*     "offerstore" and "offersource" fires wg and then stores x, or it stores
\*     x alone (OfferPlay).
PlayStagings(j, s) ==
    CASE j \in OldJobs -> {Stage(j, s)}
      [] j = "k2" -> {Staging(<<ParkW2>>, [s EXCEPT !.bag = s.bag \cup {"w2"}], "parked")}
      [] j = "pp" ->
           IF s.bag = {}
             THEN {Staging(<<Op(<<"pp", "prod">>, "F", None)>>, s, "stored")}
             ELSE {WithChoice(Staging(<<PutBag(w)>>, [s EXCEPT !.bag = s.bag \ {w}], w), w)
                     : w \in s.bag}
      [] j = "td" ->
           {WithChoice(Staging(<<OwnA("td"), OwnB("td")>>, s, "done"), "staged"),
            WithChoice(WithChild(Staging(<<Deviated(OwnA("td"), "must")>>, s, "stopped"), "rd"),
                       "stop"),
            WithChoice(WithChild(Staging(<<Deviated(OwnA("td"), "must")>>, s, "ret"),
                                 MarkedChild("rd")), "return")}
      [] j = "rd" -> {Staging(Anchored(ReissueAnchor, "rd", <<OwnB("td")>>), s, "done")}
      [] j = "pc" -> PcStagings(s)
      [] j = "tk8" -> {WithChild(Staging(<<Deviated(OwnA("tk8"), "must")>>, s, "stopped"), "rk8")}
      [] j = "rk8" -> {ReofferStaging("rk8", ReissueAnchor, s)}
      [] j = "rt" -> {WithChild(Staging(<<Deviated(OwnA("rt"), "must")>>, s, "stopped"), "rj")}
      [] j = "rj" -> RjStagings(s)
      [] j = "tq" -> {WithChild(Staging(<<Deviated(SendTq, "may")>>, s, "ret"), "tq2")}
      [] j = "tq2" -> {Staging(Anchored(~MarkFree, "tq2", <<StoreTq2>>), s, "stored")}
      [] j = "td2" ->
           {WithChild(Staging(<<Deviated(OwnA("td2"), "may")>>, s, "ret"), MarkedChild("sj"))}
      [] j = "sj" -> {Staging(<<AnchorOp("sj")>>, s, "quiescent")}
      [] j = "tr" -> {WithChild(Staging(<<Deviated(OwnA("tr"), "must")>>, s, "stopped"), "trj")}
      [] j = "j0" -> {ReofferStaging("j0", ReofferAnchor, s)}
      [] j = "trj" -> {ReofferStaging("trj", ReissueAnchor, s)}
      [] j = "sc" -> {Staging(<<ClaimSc>>, [s EXCEPT !.claim = TRUE], "stored")}
      [] j = "sq" -> SqStagings(s)
      [] j = "te" -> {WithChild(Staging(<<Deviated(OwnA("te"), "must")>>, s, "stopped"), "tej")}
      [] j = "tej" -> {Rowless("tej", s, "capacity")}
      \* Added by decision 102 (plan v4, rule CR-30): the cascade jobs.
      [] j \in {"cs", "cs2c", "cs2s", "cv"} -> CascadePlay(j, s)
      \* Added by decision 124 (L3 and F2): the offer job.
      [] j = "o" -> OfferPlay(s)

\* ---------------------------------------------------------------------
\* Play
\* ---------------------------------------------------------------------

\* Added by decision 122 (plan v4, V3-20): the rows that the staging st of the
\* job j appends.
Added(j, st) ==
    [i \in 1..Len(st.ops) |->
        [job |-> j, src |-> st.ops[i].src, chan |-> st.ops[i].chan,
         cand |-> st.ops[i].cand, stop |-> st.ops[i].stop]]

\* Changed by decision 102 (plan v3) and decision 122 (plan v4, V3-20): play
\* is set-valued over the play choices, and it skips a child job that its
\* parent did not create.
\* Play runs the jobs of `order` one transaction at a time from the state s,
\* and it appends one row for each staged operation.
RECURSIVE PlayFrom(_, _, _, _, _, _)
PlayFrom(order, s, rows, verdicts, choices, created) ==
    IF order = <<>>
      THEN {[rows |-> rows, sigma |-> s, verdicts |-> verdicts, choices |-> choices]}
      ELSE LET j == Head(order) IN
           IF j \in Children /\ j \notin created
             THEN PlayFrom(Tail(order), s, rows, verdicts, choices, created)
             ELSE UNION {PlayFrom(Tail(order), st.sigma, rows \o Added(j, st),
                                  [verdicts EXCEPT ![j] = st.verdict],
                                  [choices EXCEPT ![j] = st.choice],
                                  IF st.child = "none" THEN created
                                  ELSE created \cup {st.child})
                         : st \in PlayStagings(j, s)}

Rows == 1..Len(journal)

\* The channels of the footprint of the row i.
Footprint(i) == IF FootprintKey THEN {journal[i].chan, "S"} ELSE {journal[i].chan}

\* The last row before the row i with the channel c in its footprint, or 0.
LastOn(c, i) ==
    IF \E k \in 1..(i - 1) : c \in Footprint(k)
      THEN CHOOSE k \in 1..(i - 1) :
             /\ c \in Footprint(k)
             /\ \A m \in (k + 1)..(i - 1) : c \notin Footprint(m)
      ELSE 0

\* The recorded predecessors of the row i: the last row on each channel of
\* its footprint (`accounting/native_runtime/operations.rs`).
Pred(i) == {LastOn(c, i) : c \in Footprint(i)} \ {0}

\* ---------------------------------------------------------------------
\* Replay
\* ---------------------------------------------------------------------

\* `select_slot`: the first row of the source src outside the rows D, in
\* journal order, or 0.
SelectOpen(src, D) ==
    IF \E i \in Rows : journal[i].src = src /\ i \notin D
      THEN CHOOSE i \in Rows :
             /\ journal[i].src = src
             /\ i \notin D
             /\ \A k \in 1..(i - 1) : ~(journal[k].src = src /\ k \notin D)
      ELSE 0

\* The readiness check of the first staged row (`check_ready`).
FirstReady(src) ==
    LET r == SelectOpen(src, done) IN r /= 0 /\ Pred(r) \subseteq done

\* Added by decision 102 (plan v4, M3): the S(sid) successor rule (CR-5). The
\* row of the source src whose S(sid) predecessor is the row p, or 0.
Succ(src, p) ==
    IF \E r \in Rows : journal[r].src = src /\ LastOn("S", r) = p
      THEN CHOOSE r \in Rows : journal[r].src = src /\ LastOn("S", r) = p
      ELSE 0

\* Added by decision 102 (plan v4, M3): a replay staging. It has the fields of
\* a staging and two more: `cut` holds when the record end or a staging error
\* after a followed row ended the staging (CR-6, CR-32), and `bad` holds when
\* the job fails closed at its staging.
Plain(st) ==
    [ops |-> st.ops, sigma |-> st.sigma, verdict |-> st.verdict, child |-> st.child,
     cut |-> FALSE, bad |-> FALSE]
Cut(ops, s, verdict, child) ==
    [ops |-> ops, sigma |-> s, verdict |-> verdict, child |-> child, cut |-> TRUE, bad |-> FALSE]
Bad ==
    [ops |-> <<>>, sigma |-> InitSigma, verdict |-> "none", child |-> "none",
     cut |-> FALSE, bad |-> TRUE]

\* Added by decision 102 (plan v4, M3): replay follows the rows of the staging
\* st of one or two operations. The second operation takes the row of its
\* source whose S(sid) predecessor is the row of the first (CR-5). When it has
\* no such row, the record ends (CR-6): the staging keeps the first operation,
\* with the state s1, the verdict v1 and the child c1 of the Tail of that row.
\* With RecordEnd FALSE the staging goes on without a row.
\* Changed by plan v4 erratum V4-12 (the anchor-only rule): when the first
\* operation is the leading anchor of the job, a miss puts the second
\* operation in DR-75 mode, so the staging goes on without a row.
Follow(st, s1, v1, c1) ==
    LET r1 == IF st.ops = <<>> THEN 0 ELSE SelectOpen(st.ops[1].src, done)
        miss == /\ FollowRecord
                /\ Len(st.ops) = 2
                /\ r1 /= 0
                /\ Succ(st.ops[2].src, r1) = 0
        anchorOnly == miss /\ st.ops[1].src[2] = "anchor"
    IN IF miss /\ RecordEnd /\ ~(anchorOnly /\ AnchorOnlyMissDr75)
         THEN Cut(<<st.ops[1]>>, s1, v1, c1)
         ELSE Plain(st)

\* The replay staging of "pp": the followed row names the waiter to fire
\* (CR-4). A waiter that is not there gives the first waiter in DR-75 order
\* when StaleWaits holds, and the first-row check then waits.
PpReplay(s) ==
    LET r == SelectOpen(<<"pp", "prod">>, done)
        follow == FollowRecord /\ r /= 0
        named == IF follow THEN journal[r].cand[1] ELSE "none"
        Fire(w) == Plain(Staging(<<PutBag(w)>>, [s EXCEPT !.bag = s.bag \ {w}], w))
    IN IF follow /\ named \in s.bag THEN Fire(named)
       ELSE IF follow /\ ~StaleWaits THEN Bad
       ELSE IF s.bag = {} THEN Plain(Staging(<<Op(<<"pp", "prod">>, "F", None)>>, s, "stored"))
       ELSE Fire(FirstWaiter(s.bag))

\* The replay staging of a job whose own operation picks a record of V. The
\* followed row r names the record. Without a row the pick takes the first
\* record in DR-75 order. t1 fails with CapacityExceeded before any followed
\* row, so the transaction commits its anchor alone (`prepare_failure`).
PickReplay(j, src, r, s) ==
    LET pick == IF FollowRecord /\ r /= 0 THEN journal[r].cand[1] ELSE canonFirst
    IN IF pick = "t2"
         THEN Plain(Staging(<<TakeV(src, "t2")>>, [s EXCEPT !.v = "t2"], "fired"))
         ELSE Plain(Rowless(j, s, "capacity"))

\* The replay staging of "sq": t1 conflicts with the stored claim of "sc", and
\* replay never restages, so the transaction aborts.
SqReplay(s) ==
    LET r == SelectOpen(<<"sq", "op">>, done)
        pick == IF FollowRecord /\ r /= 0 THEN journal[r].cand[1] ELSE canonFirst
    IN IF pick = "t1" /\ s.claim
         THEN Plain(Staging(<<>>, s, "aborted"))
         ELSE Plain(Staging(<<TakeV(<<"sq", "op">>, pick)>>, [s EXCEPT !.v = pick], "fired"))

\* The replay staging of "rt": its own row a, then the pick of its next
\* operation b, whose followed row names the record.
RtReplay(s) ==
    LET r1 == SelectOpen(<<"rt", "a">>, done)
        r2 == IF FollowRecord /\ r1 /= 0 THEN Succ(<<"rt", "b">>, r1) ELSE 0
        pick == IF FollowRecord /\ r2 /= 0 THEN journal[r2].cand[1] ELSE canonFirst
    IN Follow(Staging(<<OwnA("rt"), TakeV(<<"rt", "b">>, pick)>>, [s EXCEPT !.v = pick], "done"),
              s, "stopped", "rj")

\* The replay staging of the stop re-issue "rj": the anchor, then the pick of
\* the next operation of "rt".
\* Changed by plan v4 erratum V4-12 (the error rule): t1 fails with
\* CapacityExceeded before its stage call, after the followed anchor. Replay
\* has followed no row after the anchor, so the error keeps the path of
\* `prepare_failure`. The pick lookup never answers RecordEnded (CR-6), so a
\* pick without a successor row only takes the DR-75 order.
RjReplay(s) ==
    IF ReissueAnchorFirst
      THEN LET r1 == SelectOpen(<<"rj", "anchor">>, done)
               r2 == IF FollowRecord /\ r1 /= 0 THEN Succ(<<"rt", "b">>, r1) ELSE 0
               pick == IF FollowRecord /\ r2 /= 0 THEN journal[r2].cand[1] ELSE canonFirst
           IN IF pick = "t2"
                THEN Follow(Staging(<<AnchorOp("rj"), TakeV(<<"rt", "b">>, "t2")>>,
                                    [s EXCEPT !.v = "t2"], "fired"),
                            s, "fired", "none")
              ELSE IF FollowRecord /\ r1 /= 0 /\ FollowErrorEnds /\ ~AnchorOnlyErrorPath
                THEN Cut(<<AnchorOp("rj")>>, s, "capacity", "none")
              ELSE Plain(Rowless("rj", s, "capacity"))
      ELSE PickReplay("rj", <<"rt", "b">>, SelectOpen(<<"rt", "b">>, done), s)

\* The replay staging of "te": its own row a, then the next step of its
\* continuation, a store that fails with CapacityExceeded before its stage
\* call. The error comes after the followed row a (CR-32).
TeReplay(s) ==
    LET r == SelectOpen(<<"te", "a">>, done)
    IN IF FollowRecord /\ r /= 0 /\ FollowErrorEnds
         THEN Cut(<<OwnA("te")>>, s, "stopped", "tej")
         ELSE Plain(Rowless("te", s, "capacity"))

\* The replay staging of the stop re-issue "tej": the anchor, then the store
\* that fails with CapacityExceeded before its stage call. With the anchor
\* first, the error comes after the followed anchor (CR-32).
\* Changed by plan v4 erratum V4-12: "tej" stages its counter take after the
\* anchor, as a discipline with a counter does. Play committed the anchor
\* alone, so the take has no successor row. The miss rule stages the take in
\* DR-75 mode, and the error rule then keeps the path of `prepare_failure`.
\* Without the miss rule the take answers RecordEnded. Without the error rule
\* the error ends the staging (CR-32). Both cut the staging after the anchor,
\* which never satisfies CanStop. A followed take is a row after the anchor,
\* so CR-32 applies to it as before.
TejReplay(s) ==
    LET r == SelectOpen(<<"tej", "anchor">>, done)
        follow == ReissueAnchorFirst /\ FollowRecord /\ r /= 0
        \* The counter take after the followed anchor has no successor row.
        miss == follow /\ Succ(CountOp("tej").src, r) = 0
    IN IF miss /\ RecordEnd /\ ~AnchorOnlyMissDr75
         THEN Cut(<<AnchorOp("tej")>>, s, "none", "none")
       ELSE IF follow /\ ~miss /\ FollowErrorEnds
         THEN Cut(<<AnchorOp("tej"), CountOp("tej")>>, s, "capacity", "none")
       ELSE IF follow /\ FollowErrorEnds /\ ~AnchorOnlyErrorPath
         THEN Cut(<<AnchorOp("tej")>>, s, "capacity", "none")
       ELSE Plain(Rowless("tej", s, "capacity"))

\* Added by decision 102 (plan v4, rule CR-30): the cursor of a cascade
\* staging in replay. The mode is "unread" (no row followed yet), "anchored"
\* (at the row of the leading anchor), "following" (at a followed row),
\* "free" (DR-75 mode) or "ended" (RecordEnded). row is the journal row of
\* the cursor, or 0.
Cur(mode, row) == [mode |-> mode, row |-> row]

\* The stage call of the operation op at the cursor cur (CR-4 to CR-6). At
\* Unread the lookup is the first open row of the source. At Anchored and at
\* Following it is the S(sid) successor of the row of the cursor. A miss
\* after the leading anchor alone puts the staging in DR-75 mode (V4-12, the
\* miss rule). A miss after a followed row answers RecordEnded (CR-6).
StageCall(op, cur) ==
    CASE ~FollowRecord -> Cur("free", 0)
      [] cur.mode = "unread" ->
           LET r == SelectOpen(op.src, done)
           IN IF r /= 0 THEN Cur("following", r) ELSE cur
      [] cur.mode \in {"anchored", "following"} ->
           LET r == Succ(op.src, cur.row)
           IN IF r /= 0 THEN Cur("following", r)
              ELSE IF cur.mode = "anchored" /\ AnchorOnlyMissDr75 THEN Cur("free", 0)
              ELSE IF RecordEnd THEN Cur("ended", 0)
              ELSE Cur("free", 0)
      [] OTHER -> cur

\* The stage call of the leading anchor op at an empty staging
\* (`stage_anchor_first`). A hit puts the cursor at Anchored.
AnchorCall(op) ==
    LET c == StageCall(op, Cur("unread", 0))
    IN IF c.mode = "following" THEN Cur("anchored", c.row) ELSE c

\* Retired by decision 124 (L2): replay fires the first qualifying waiter in
\* the DR-75 waiter order, so it probes no successor and reads no journal
\* index.

\* The qualifying candidates of the job j in the state s, in DR-75 order. The
\* qualification is eager and takes no lease (D-b). Replay always takes the
\* DR-75 window, so the guard of wg admits the pick only when canonFirst is
\* t2.
CascadeCands(j, s) ==
    IF ChainOf(j) = "cv"
      THEN IF "wg" \in s.cw /\ canonFirst = "t2" THEN <<2>> ELSE <<>>
      \* Changed by decision 124 (L2): the DR-75 order of w1 and w2 is the order
      \* that Init chose, as play reads it in CascadePlay.
      ELSE LET Waits(i) == LeadWaiter(j, i) \in s.cw IN SelectSeq(cascadeCase.waiterOrder, Waits)

\* The round cap: one round with capOne, otherwise two rounds.
CascadeCap == IF cascadeCase.capOne THEN 1 ELSE 2

\* Retired by decision 124 (L2): replay pins no source, so the set U, the pin
\* of the least journal index and the successor probes are retired.

\* The ends of a cascade staging of the job j with the operations ops.
\*   - EndDrain: no waiter qualifies, so the drain ends.
\*   - CapEnd: the cap ends the drain, and the finish of "cs" creates the cap
\*     job "cs2c".
\*   - CutEnd: the record end cuts the staging, and the finish of "cs"
\*     creates the stop re-issue "cs2s".
EndDrain(j, s, ops) ==
    IF ops = <<>> THEN Plain(CascadeRowless(j, s)) ELSE Plain(Staging(ops, s, "done"))
CapEnd(j, s, ops) ==
    Plain(WithChild(Staging(ops, s, "capped"), IF j = "cs" THEN "cs2c" ELSE "none"))
CutEnd(j, s, ops) == Cut(ops, s, "stopped", IF j = "cs" THEN "cs2s" ELSE "none")

\* Changed by decision 124 (L2): replay fires the first qualifying waiter, so
\* EmptyPin and FollowPin are retired.
\* The cascade drain of the job j in replay (decision record CR-30, section
\* 1.1, as decision 124 (L2) changes it). s is the state, ops holds the
\* staged operations, cur is the cursor and n counts the rounds of the job.
\*   - CascadeLoop: the drain ends when no waiter qualifies. Then the cap
\*     check runs. Otherwise the first qualifying waiter C[1] fires. When its
\*     round starts the job (ops is empty) and LeadAnchor is set for C[1],
\*     the chain anchor goes first, and its stage call (AnchorCall) puts the
\*     cursor at Anchored.
\*   - FireOps: the round of the candidate k stages its operations from the
\*     operation i on. A stage call that answers RecordEnded cuts the
\*     staging before its operation.
\* Changed by decision 124 (L2): EmptyPin and FollowPin are retired.
RECURSIVE CascadeLoop(_, _, _, _, _), FireOps(_, _, _, _, _, _, _)
CascadeLoop(j, s, ops, cur, n) ==
    LET C == CascadeCands(j, s)
    IN IF C = <<>> THEN EndDrain(j, s, ops)
       ELSE IF n >= CascadeCap THEN CapEnd(j, s, ops)
       \* Changed by decision 124 (L2): replay fires C[1], with the chain anchor
       \* first when the round starts the job and LeadAnchor is set (VJ:115).
       \* An empty ops comes only with the cursor Cur("unread", 0).
       ELSE IF ops = <<>> /\ LeadAnchor(j, C[1]) /= <<>>
         THEN FireOps(j, s, <<ChainAnchor(j)>>, AnchorCall(ChainAnchor(j)), n, C[1], 1)
       ELSE FireOps(j, s, ops, cur, n, C[1], 1)
\* Retired by decision 124 (L2): the pin at an empty staging, the successor
\* probes and the stage call at a miss. The anchor branch of EmptyPin moved
\* into CascadeLoop.
FireOps(j, s, ops, cur, n, k, i) ==
    IF i > Len(RoundOps(j, k))
      THEN CascadeLoop(j, FireCw(s, j, k), ops, cur, n + 1)
      ELSE LET op == RoundOps(j, k)[i]
               c == StageCall(op, cur)
           IN IF c.mode = "ended" THEN CutEnd(j, s, ops)
              ELSE FireOps(j, s, ops \o <<op>>, c, n, k, i + 1)

\* The replay of the stop re-issue "cs2s": the chain anchor first (CR-19
\* case 2), then the cascade drain.
CsReissueReplay(s) ==
    IF ReissueAnchorFirst
      THEN CascadeLoop("cs2s", s, <<ChainAnchor("cs2s")>>, AnchorCall(ChainAnchor("cs2s")), 0)
      ELSE CascadeLoop("cs2s", s, <<>>, Cur("unread", 0), 0)

\* Added by decision 124 (L3 and F2): the replay staging of "o" (base E3-a,
\* selection.rs:747-751). The offer reads the rows that its stage calls read
\* at the cursor, Unread here (StageCall). It is Stored when the row of the
\* element is Stored, and with OfferStoredFirst also when no open row of the
\* vehicle comes before it. On a Stored offer the staging skips wg, whose
\* commit may veto, so x is stored. Otherwise the search runs. The guard is
\* pure, so the search repeats the verdict of play: wg fires when the row of
\* its vehicle names it, as PpReplay follows the row.
OfferReplay(s) ==
    LET rp == StageCall(StoreX, Cur("unread", 0)).row
        rv == StageCall(VehFire, Cur("unread", 0)).row
        stored == rp /= 0 /\ journal[rp].cand = None
        first == ~OfferStoredFirst \/ rv = 0 \/ rp < rv
        skip == "wg" \in s.cw /\ stored /\ first
        named == rv /= 0 /\ journal[rv].cand = <<"wg", "cons">>
    IN IF "wg" \in s.cw /\ ~skip /\ named
         THEN Follow(OfferFire(s), [s EXCEPT !.cw = s.cw \ {"wg"}], "fired", "none")
         ELSE Plain(OfferStore(s))

\* Added by decisions 102 and 122 (plan v4, M3): the replay staging of the job
\* j in the state s (CR-4 to CR-6, CR-31, CR-32). It is deterministic. Replay
\* takes the mark-free finish (CR-31), and it reads `TailOf` of play
\* (D-OBL-1): the Tail of "td" is the choice of play.
\* Added by decision 124 (L3 and F2): the offer job "o" of the scenarios
\* "offerstore" and "offersource" skips wg on a Stored offer and otherwise
\* fires wg when the row of its vehicle names wg (OfferReplay).
ReplayStage(j, s) ==
    CASE j \in OldJobs -> Follow(Stage(j, s), s, "none", "none")
      [] j = "k2" -> Plain(Staging(<<ParkW2>>, [s EXCEPT !.bag = s.bag \cup {"w2"}], "parked"))
      [] j = "pp" -> PpReplay(s)
      [] j = "td" ->
           Follow(Staging(<<OwnA("td"), OwnB("td")>>, s, "done"), s,
                  IF pchoice["td"] = "return" THEN "ret" ELSE "stopped",
                  IF pchoice["td"] = "return" THEN "none" ELSE "rd")
      [] j = "rd" ->
           Follow(Staging(Anchored(ReissueAnchorFirst, "rd", <<OwnB("td")>>), s, "done"),
                  s, "done", "none")
      [] j = "pc" -> PickReplay("pc", <<"pc", "op">>, SelectOpen(<<"pc", "op">>, done), s)
      [] j = "tk8" ->
           Follow(Staging(<<OwnA("tk8"), ReofferR(s)>>, [s EXCEPT !.heads = Tail(s.heads)], "done"),
                  s, "stopped", "rk8")
      [] j = "rk8" -> Follow(ReofferStaging("rk8", ReissueAnchorFirst, s), s, "none", "none")
      [] j = "rt" -> RtReplay(s)
      [] j = "rj" -> RjReplay(s)
      [] j = "tq" -> Plain(WithChild(Staging(<<SendTq>>, s, "ret"), "tq2"))
      [] j = "tq2" -> Plain(Staging(<<StoreTq2>>, s, "stored"))
      [] j = "td2" -> Plain(Staging(<<OwnA("td2")>>, s, "ret"))
      [] j = "sj" -> Plain(Staging(<<AnchorOp("sj")>>, s, "quiescent"))
      [] j = "tr" ->
           IF s.heads = <<>>
             THEN Plain(WithChild(Staging(<<OwnA("tr")>>, s, "stopped"), "trj"))
             ELSE Follow(Staging(<<OwnA("tr"), ReofferR(s)>>, [s EXCEPT !.heads = Tail(s.heads)],
                                 "done"),
                         s, "stopped", "trj")
      [] j = "j0" -> Follow(ReofferStaging("j0", ReofferAnchor, s), s, "none", "none")
      [] j = "trj" -> Follow(ReofferStaging("trj", ReissueAnchorFirst, s), s, "none", "none")
      [] j = "sc" -> Plain(Staging(<<ClaimSc>>, [s EXCEPT !.claim = TRUE], "stored"))
      [] j = "sq" -> SqReplay(s)
      [] j = "te" -> TeReplay(s)
      [] j = "tej" -> TejReplay(s)
      \* Added by decision 102 (plan v4, rule CR-30): the cascade jobs.
      [] j \in {"cs", "cs2c", "cv"} -> CascadeLoop(j, s, <<>>, Cur("unread", 0), 0)
      [] j = "cs2s" -> CsReissueReplay(s)
      \* Added by decision 124 (L3 and F2): the offer job.
      [] j = "o" -> OfferReplay(s)

\* Changed by decision 122 (plan v4, M3): continuing past a row requires
\* CanContinue (stop "no" or "may"). Ending a cut staging at its last row
\* requires CanStop (stop "must" or "may").
\* The commit of the staged operations from the operation i on, over the
\* complete rows D. Each operation begins the first open row of its source
\* (`begin_row`). The row must be ready, and its candidate must be the staged
\* candidate. Otherwise the commit fails closed.
RECURSIVE CommitFrom(_, _, _, _)
CommitFrom(ops, i, D, cut) ==
    IF i > Len(ops)
      THEN [ok |-> TRUE, done |-> D]
      ELSE LET r == SelectOpen(ops[i].src, D)
           IN IF /\ r /= 0
                 /\ Pred(r) \subseteq D
                 /\ journal[r].cand = ops[i].cand
                 /\ i < Len(ops) => journal[r].stop \in {"no", "may"}
                 /\ (i = Len(ops) /\ cut) => journal[r].stop \in {"must", "may"}
                THEN CommitFrom(ops, i + 1, D \cup {r}, cut)
                ELSE [ok |-> FALSE, done |-> D]

Stopped == stalled \/ failed

\* Added by decision 122 (plan v4, V3-20): the jobs that exist and have not
\* ended.
Live(st) == {j \in Jobs : st[j] \notin {"ended", "absent"}}

\* Changed by decision 122 (plan v4, V3-20): an absent job is not live.
\* Some job has not ended, and every job that has not ended is parked.
AllParked(st) ==
    /\ Live(st) /= {}
    /\ \A j \in Live(st) : st[j] = "parked"

Wake(st) == [j \in Jobs |-> IF st[j] = "parked" THEN "run" ELSE st[j]]

\* Changed by decision 122 (plan v4, V3-20): the finish creates the child of
\* its staging, which starts as "run".
\* The job j ends with the staging st after its commit completed the rows D.
\* A commit that completes a row is a change, so it wakes every parked job. A
\* job that ends without a row changes nothing, and the stall check runs at
\* its end.
Finish(j, st, D) ==
    LET ended == [status EXCEPT ![j] = "ended"]
        made == IF st.child = "none" THEN ended ELSE [ended EXCEPT ![st.child] = "run"]
        next == IF D /= done THEN Wake(made) ELSE made
    IN /\ done' = D
       /\ rsigma' = st.sigma
       /\ status' = next
       /\ rverdict' = [rverdict EXCEPT ![j] = st.verdict]
       /\ stalled' = AllParked(next)
       \* Changed by decision 102 (plan v4, rule CR-30): the list names
       \* cascadeCase.
       /\ UNCHANGED <<journal, pverdict, pfinal, pchoice, canonFirst, cascadeCase, failed>>

\* Changed by decision 102 (plan v4, V3-20): the UNCHANGED list names pchoice
\* and canonFirst.
\* The commit fails closed: the epoch closes, and the block is rejected.
FailClosed ==
    /\ failed' = TRUE
    \* Changed by decision 102 (plan v4, rule CR-30): the list names cascadeCase.
    /\ UNCHANGED <<journal, pverdict, pfinal, pchoice, canonFirst, cascadeCase, rsigma, done,
                   status, rverdict, stalled>>

\* Changed by decision 102 (plan v4, V3-20): the UNCHANGED list names pchoice
\* and canonFirst.
\* NotReady: the job j releases its locks and parks as a counted waiter. The
\* park that leaves every job that has not ended parked declares the stall.
Park(j) ==
    LET next == [status EXCEPT ![j] = "parked"]
    IN /\ status' = next
       /\ stalled' = AllParked(next)
       \* Changed by decision 102 (plan v4, rule CR-30): the list names
       \* cascadeCase.
       /\ UNCHANGED <<journal, pverdict, pfinal, pchoice, canonFirst, cascadeCase, rsigma, done,
                      rverdict, failed>>

\* Changed by decisions 102 and 122 (plan v4, M3): the staging follows the
\* record, a job can fail closed at its staging, and the commit checks the
\* stop values of a cut staging.
\* One attempt of the job j (`run_job`): it stages from the current state,
\* checks its first row, and then commits, fails closed or parks.
Attempt(j) ==
    /\ ~Stopped
    /\ status[j] = "run"
    /\ LET st == ReplayStage(j, rsigma) IN
         IF st.bad
           THEN FailClosed
         ELSE IF st.ops = <<>>
           THEN Finish(j, st, done)
         ELSE IF FirstReady(st.ops[1].src)
           THEN LET c == CommitFrom(st.ops, 1, done, st.cut)
                IN IF c.ok THEN Finish(j, st, c.done) ELSE FailClosed
         ELSE IF ~UnknownSourceWaits /\ SelectOpen(st.ops[1].src, done) = 0
           THEN FailClosed
         ELSE Park(j)

\* Changed by decisions 102 and 122 (plan v4, V3-20): play is set-valued, the
\* DR-75 order of V is chosen first, and a child starts absent.
Init ==
    \* Changed by plan v4 erratum V4-12 (the anchor-only rule): "rafirst" also has
    \* both DR-75 orders.
    \* Changed by decision 102 (plan v4, rule CR-30): "cascadeveto" also has
    \* both DR-75 orders.
    /\ canonFirst \in (IF Scenario \in {"capfail", "rafirst", "cascadeveto"} THEN {"t1", "t2"}
                       ELSE {"t2"})
    \* Added by decision 102 (plan v4, rule CR-30): the case of "cascade". The
    \* other scenarios have one fixed value.
    \* Changed by decision 124 (L2): Init also chooses the DR-75 waiter order,
    \* as it chooses canonFirst.
    /\ cascadeCase \in (IF Scenario = "cascade"
                          THEN [capOne : BOOLEAN, persist2 : BOOLEAN,
                                waiterOrder : {<<1, 2>>, <<2, 1>>}]
                          ELSE {[capOne |-> FALSE, persist2 |-> FALSE, waiterOrder |-> <<1, 2>>]})
    /\ \E order \in Orders :
         \E p \in PlayFrom(order, InitSigma, <<>>, [j \in Jobs |-> "none"],
                           [j \in Jobs |-> "none"], {}) :
            /\ journal = p.rows
            /\ pverdict = p.verdicts
            /\ pfinal = p.sigma
            /\ pchoice = p.choices
    /\ rsigma = InitSigma
    /\ done = {}
    /\ status = [j \in Jobs |-> IF j \in Children THEN "absent" ELSE "run"]
    /\ rverdict = [j \in Jobs |-> "none"]
    /\ stalled = FALSE
    /\ failed = FALSE

Next == \E j \in Jobs : Attempt(j)

Spec == Init /\ [][Next]_vars

\* ---------------------------------------------------------------------
\* Invariants
\* ---------------------------------------------------------------------

Cells == {"empty", "waiter"} \cup Puts
HeadStates == {<<>>, <<"w1">>, <<"w2">>, <<"w1", "w2">>}
\* Added by decisions 102 and 122 (plan v4, M3): the states of the space.
\* Changed by decision 102 (plan v4, rule CR-30): the field cw.
Sigmas ==
    [cell : Cells, heads : HeadStates, bag : SUBSET Waiters, v : {"open", "t1", "t2"},
     claim : BOOLEAN, cw : SUBSET CascadeWaiters]
\* Changed by decisions 102 and 122 (plan v4, M3): the sources, candidates,
\* channels, verdicts and choices of the new scenarios.
\* Changed by decision 102 (plan v4, rule CR-30): the lift and re-offer
\* sources, the candidates, the verdict "capped" and the choices of the
\* cascade.
\* Added by decision 124 (L3 and F2): the role "veh", the vehicle of an element on an index channel.
Sources ==
    (Jobs \cup {"R", "v1", "v2", "t1", "t2"})
        \X {"prod", "cons", "anchor", "a", "b", "op", "lift1", "lift2", "veh"}
Candidates ==
    (Puts \X {"prod"}) \cup (Waiters \X {"cons"}) \cup {<<"tk", "cons">>, None, ConfigRecord}
        \cup ({"t1", "t2"} \X {"prod"}) \cup ({"v1", "v2"} \X {"prod"}) \cup {<<"wg", "cons">>}
Verdicts ==
    {"none", "stored", "fired", "occupied", "parked", "quiescent", "done", "stopped", "ret",
     "capacity", "aborted", "capped"} \cup Puts \cup Waiters
Choices == {"none", "staged", "stop", "return", "t1", "t2", "restaged", "all", "cap"} \cup Waiters
\* Added by decision 124 (L3 and F2): the index channel I of a PathMap prefix.
RowRecords ==
    [job : Jobs, src : Sources, chan : {"C", "D", "E", "F", "G", "I", "V"}, cand : Candidates,
     stop : {"no", "must", "may"}]

\* Changed by decisions 102 and 122 (plan v4, M3): the new variables, the new
\* state fields and the status "absent".
TypeOK ==
    /\ journal \in Seq(RowRecords)
    /\ pverdict \in [Jobs -> Verdicts]
    /\ pfinal \in Sigmas
    /\ pchoice \in [Jobs -> Choices]
    /\ canonFirst \in {"t1", "t2"}
    \* Added by decision 102 (plan v4, rule CR-30).
    \* Changed by decision 124 (L2): the DR-75 waiter order.
    /\ cascadeCase \in [capOne : BOOLEAN, persist2 : BOOLEAN, waiterOrder : {<<1, 2>>, <<2, 1>>}]
    /\ rsigma \in Sigmas
    /\ done \subseteq Rows
    /\ status \in [Jobs -> {"run", "parked", "ended", "absent"}]
    /\ rverdict \in [Jobs -> Verdicts]
    /\ stalled \in BOOLEAN
    /\ failed \in BOOLEAN

NoStall == ~stalled

NoFailClosed == ~failed

AllEnded == \A j \in Jobs : status[j] = "ended"

\* Added by decision 122 (plan v3): replay is quiet when every job has ended
\* or is absent.
Quiet == \A j \in Jobs : status[j] \in {"ended", "absent"}

\* Changed by decision 122 (plan v4, V3-20): the final state is compared when
\* replay is quiet and every row is complete.
Agreement ==
    /\ \A j \in Jobs : status[j] = "ended" => rverdict[j] = pverdict[j]
    /\ (Quiet /\ done = Rows) => rsigma = pfinal

\* Changed by decision 122 (plan v3): absent jobs count as quiet.
ExactRows == Quiet => done = Rows

\* Reachability witnesses. A configuration expects each one to be violated,
\* so the invariants do not hold only because replay never ends.
\* Changed by decision 122 (plan v4, V3-20):
NoHonestCompletion == ~(Quiet /\ done = Rows /\ ~Stopped)

NoFailureVerdict == ~(\E j \in Jobs : status[j] = "ended" /\ rverdict[j] = "occupied")

\* Added by decision 102 (plan v4, M3): replay ends a job whose play committed
\* after the R-A restage.
NoRestagedCommit ==
    ~(\E j \in Jobs : pchoice[j] = "restaged" /\ status[j] = "ended" /\ ~Stopped)

\* Added by decision 122 (plan v4, M3): replay ends a deviating job by the
\* Return branch of its Tail.
NoReturnAfterDeviation ==
    ~(\E j \in Jobs : status[j] = "ended" /\ rverdict[j] = "ret" /\ ~Stopped)
================================================================================
