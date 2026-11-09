----------------------------- MODULE JoinCascade -----------------------------
(***************************************************************************)
(* Joins in one space: the search, the join engine and the cascade (P5c,   *)
(* the joins plan with the amendments of 2026-10-07, sections 3, 4, 8.2).  *)
(*                                                                         *)
(* A join is a receive with two or more binds. Its binds can name one      *)
(* collection more than once (a repeated channel) or several collections   *)
(* of the space. The model has the collections c1 and c2 and the groups of *)
(* the constant Groups (each one a tuple of collections in group order).   *)
(* The discipline is a queue or a stack (head shapes: a waiter fires only  *)
(* at the head of each collection of its group), a bag, or a set (scan     *)
(* shapes: no head condition, every record in the window, DR-75 order). A  *)
(* pick on a set that is not persistent claims its record and its class    *)
(* (its payload). A claimed class blocks only the members of a class that  *)
(* has no persistent member (rule (a), below). Changed by decision 103     *)
(* (decision 118, C6 arbitration): a pick on a set also claims its record. *)
(* The text before: "A set claims a class (its payload) instead of a       *)
(* record."                                                                *)
(*                                                                         *)
(* GREEDY takes the binds in group order. Each bind picks the first        *)
(* unclaimed record of its window that matches its pattern. A pick that is *)
(* not persistent claims its record, and on a set also its class. The      *)
(* guard runs once over the values (decision 37). SEARCH-PRODUCE forces    *)
(* the incoming record at one position of its collection and fills the     *)
(* other positions from the windows with the incoming record placed        *)
(* (decision 28). An incoming record that is not persistent claims its     *)
(* record, and on a set also its class. The window of a collection for m   *)
(* binds holds the first m records of its head order (queue: the oldest    *)
(* first, stack: the newest first), or every record (bag and set). Changed *)
(* by decision 103 (decision 118, C6 arbitration): rule (a) gives the      *)
(* claims of a pick and of the incoming record. The text before: "Each     *)
(* bind picks the first unclaimed record of its window that matches its    *)
(* pattern, and the pick claims the record (or its class)." and "The       *)
(* incoming record claims its own class on a set."                         *)
(*                                                                         *)
(* Each step of the model is one space transaction (RUN-JOIN, plan 4.2).   *)
(* No other transaction runs inside it, so the step computes the whole     *)
(* transaction: TOUCH bumps the counter of each own collection (decision   *)
(* 31). A settle job has no plan: it makes its scope dirty, drains its     *)
(* pending second sends and the cascade, and commits (DV-1). Another round *)
(* runs the settle before the plan when its discipline cascades, clears    *)
(* the dirty collections, opens the plan only below the fire count cap     *)
(* (open_plan, decision 49, DV-2), runs OFFER or RECEIVE, and drains the   *)
(* second sends and the cascade. The head shapes and the set cascade       *)
(* (decisions 11 and 50), and the bag does not. A fire makes every         *)
(* collection of its group dirty. The cascade fires each fireable waiter   *)
(* of the dirty collections as one direct COMM (LIFT-AND-REOFFER, Rocq     *)
(* headline 11). A peek fire pushes the element in flight, also when it is *)
(* persistent, and its other non-persistent results as second sends (Rule  *)
(* E, decisions 30 and 45, DV-3), and the drain offers them again in       *)
(* receipt order. A set fire that does not peek retires the other          *)
(* non-persistent members of each bound class (RETIRE-SET-BINDS), also in  *)
(* the cascade. One fire count runs through the transaction. A stop that   *)
(* owes work commits a settle job with its seeds (the dirty collections),  *)
(* its pending second sends and its resume operation (CascadeState). The   *)
(* element of the operation returns to its operation. A stop that owes     *)
(* nothing re-issues its resume only (DV-5). A re-issued persistent        *)
(* operation keeps the ords of its first round (decision 43).              *)
(*                                                                         *)
(* Changed by decision 102 (DR-129 of the base): play picks at random. A   *)
(* bag or a set takes any matching record of a window that its group names *)
(* once, any waiter of an offer, and any waiter of the set cascade. A      *)
(* collection that a group names more than once keeps one deterministic    *)
(* list (decision 103, E1). The queue and the stack keep head order. The   *)
(* guard still runs once over the values of the pick (decision 37). So a   *)
(* receive parks when its guard refuses its pick, as base play parks a     *)
(* receive that its commit vetoes. The operators of the engine return the  *)
(* set of the end states of a transaction, and a round takes any of them.  *)
(*                                                                         *)
(* Added by decision 102 (plan v4, rule CR-30): with the constant          *)
(* CascadeCanon, the cascade search takes the DR-75-first entry of         *)
(* each window (F3, GreedyCanon). So the waiter order is the only          *)
(* random choice of the set cascade. The receive and the offer keep        *)
(* the random pick. GreedyCanon is a subset of Greedy. So the cascade      *)
(* with CascadeCanon reaches only end states that the cascade without it   *)
(* reaches, and every invariant still holds. QuiescentNoPendingMatch keeps *)
(* Greedy: a canonical pick that does not fire refutes Fireable. Every cfg *)
(* sets CascadeCanon = TRUE.                                               *)
(*                                                                         *)
(* Changed by decision 103 (decision 118, the claims of the base): a       *)
(* persistent record is never claimed, so it can bind several positions of *)
(* one fire. A set class with a persistent member is never claimed as a    *)
(* class (rule (a), below). On the collection of the incoming record x,    *)
(* the list of another position puts a persistent x first (E1) and leaves  *)
(* a non-persistent x out. The position retry of SEARCH-PRODUCE stays.     *)
(*                                                                         *)
(* Changed by decision 103 (decision 118, C6 arbitration): rule (a)        *)
(* replaces the set clause of CR-9. Under that clause, a non-persistent    *)
(* member of a set class with a persistent member claimed nothing. So it   *)
(* could bind two positions of one fire (finding F-118-2), and the code    *)
(* refuses that fire at staging (FR-1'). Rule (a) is the greedy pass of    *)
(* the base on the E1 list:                                                *)
(*   - A later position takes the next unclaimed entry of its list.        *)
(*   - A persistent entry claims nothing, and no claim blocks it.          *)
(*   - A non-persistent entry claims its record. On a set it also claims   *)
(*     its class.                                                          *)
(*   - An entry is skipped when its record is claimed. It is also skipped  *)
(*     when its class is claimed, it is not persistent, and the class has  *)
(*     no persistent member. A persistent x of the class counts as a       *)
(*     persistent member.                                                  *)
(* So a non-persistent record fills at most one position of a fire on      *)
(* every discipline, and two positions of one set class need a persistent  *)
(* member of that class. The claims hold record tags only (RTag). A        *)
(* claimed class is the set of the record tags of its members (ClassTags). *)
(*                                                                         *)
(* Changed by decision 102 (decision 119 G1): a single-bind fire on a set  *)
(* that does not peek dispatches the member of its class with the least    *)
(* identity. It then retires the other non-persistent members. A peek      *)
(* binds the representative alone (Rule E).                                *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   QuiescentNoPendingMatch  with no re-issued operation and no settle    *)
(*                            job outstanding: no waiter is fireable       *)
(*                            (INV_head: queue, stack and set), and no     *)
(*                            waiter without a guard has a greedy match    *)
(*                            (INV_scan: bag). Rocq: cascade_restores_inv. *)
(*                            Changed by decision 102: a waiter is         *)
(*                            fireable when every pick of its search       *)
(*                            fires, and INV_scan asks for a match of      *)
(*                            every pick. A pick that the guard refuses    *)
(*                            can leave a waiter parked. The spec search   *)
(*                            uses the claims of decision 118 under every  *)
(*                            control. Added by decision 103 (decision     *)
(*                            118, C6 arbitration): the control            *)
(*                            FreeClassUnclaimedUnsafe replaces the claim  *)
(*                            rule of the engine and of the spec search    *)
(*                            together.                                    *)
(*   JoinFiresHeadsOnly       every fire is at the head of each collection *)
(*                            of its group (Rocq: join_fires_heads_only).  *)
(*   NonPersistentBoundOnce   a non-persistent stored record fills at most *)
(*                            one position of a fire, on every discipline. *)
(*                            On a set, two positions of one class need a  *)
(*                            persistent member of that class (Rocq:       *)
(*                            join_nonpersistent_bound_once). Changed by   *)
(*                            decision 103 (decision 118): it restates     *)
(*                            DistinctRecordsPerBind. Retired: "the stored *)
(*                            records of one fire are pairwise distinct,   *)
(*                            and of distinct classes on a set (Rocq:      *)
(*                            join_repeated_channel_distinct_records)".    *)
(*                            Changed by decision 103 (decision 118, C6    *)
(*                            arbitration): rule (a) binds a               *)
(*                            non-persistent record once on a set too. So  *)
(*                            the record clause covers the set.            *)
(*   IncomingHeld             a produce-side fire binds its incoming       *)
(*                            record x, and it binds a non-persistent x at *)
(*                            exactly one position (Rocq:                  *)
(*                            join_incoming_held). Changed by decision 103 *)
(*                            (decision 118): it restates                  *)
(*                            IncomingBoundOnce. Retired: "a produce-side  *)
(*                            fire binds its incoming record at exactly    *)
(*                            one position (Rocq:                          *)
(*                            join_incoming_binds_once)".                  *)
(*   CascadeMeasureDecreases  each drain step that goes on lowers mu =     *)
(*                            (non-persistent waiters, non-persistent      *)
(*                            records + pending second sends, pending      *)
(*                            second sends) or stops (Rocq:                *)
(*                            cascade_terminates).                         *)
(*   TxnFiresBounded          no transaction makes more than Cap fires     *)
(*                            (Rocq: join_txn_fires_bounded).              *)
(*   NoRecordLost             every record that was stored is stored,      *)
(*                            spent by a fire of a waiter that does not    *)
(*                            peek or by a retirement, or pending in an    *)
(*                            outstanding settle job (Rocq:                *)
(*                            peek_join_restores_stored_candidates).       *)
(*   ReissueKeepsBindOrds     a re-issued persistent operation carries the *)
(*                            ords of its first round, and another         *)
(*                            re-issued operation carries none (Rocq:      *)
(*                            join_reissue_keeps_bind_ords).               *)
(*   TouchedCountersBumped    each staging on a collection follows a bump  *)
(*                            of its counter in the same transaction       *)
(*                            (queue and stack. Rocq:                      *)
(*                            cascade_bumps_touched_counters).             *)
(*                                                                         *)
(* Witness (the Bag bounds, MUST be violated). Added by decision 103       *)
(* (decision 118):                                                         *)
(*   NoPersistentTwice        no fire binds a persistent record at two     *)
(*                            positions. JoinPersistentTwiceReachable      *)
(*                            refutes it, so a fire reaches the rule of    *)
(*                            decision 118.                                *)
(* Witness (the Set bounds, MUST be violated). Added by decision 103       *)
(* (decision 118, C6 arbitration):                                         *)
(*   NoFreeClassTwice         no fire binds a non-persistent member and a  *)
(*                            persistent member of one set class at two    *)
(*                            positions. FreeClassTwiceReachable replaces  *)
(*                            PersistentTwice with FreeClassTwice, so the  *)
(*                            ghost pOk records that fire, and it refutes  *)
(*                            NoFreeClassTwice. So a fire reaches rule (a) *)
(*                            on a class with a persistent member.         *)
(*                                                                         *)
(* Unsafe controls (CONSTANT flags). Each one refutes exactly one          *)
(* invariant, and the complement run without that invariant must find no   *)
(* violation:                                                              *)
(*   NoCascade = TRUE        NoLiftUnsafe refutes QuiescentNoPendingMatch. *)
(*                           The drain has no cascade. A produce on c1     *)
(*                           fires the join head of c2, and the waiter     *)
(*                           behind it on c2 then matches a stored record  *)
(*                           with no fire (the exposure trace of plan      *)
(*                           7.1).                                         *)
(*   NoSetCascade = TRUE     NoSetCascadeUnsafe refutes                    *)
(*                           QuiescentNoPendingMatch. The set does not     *)
(*                           cascade (decision 50 reverted). A receive of  *)
(*                           any with the guard onlyB parks on the pick a. *)
(*                           A second receive consumes a, and no cascade   *)
(*                           fires the parked receive on b. Decision 118   *)
(*                           retired the old trace (finding F-JF-1).       *)
(*   PlainRestore = TRUE     PlainRestoreUnsafe refutes                    *)
(*                           QuiescentNoPendingMatch. A popped second send *)
(*                           is stored plainly: after a peek fire, the     *)
(*                           incoming record is stored although the next   *)
(*                           waiter matches it (the A.9 trace).            *)
(*   IgnoreClaims = TRUE     StoredRecordTwiceUnsafe refutes               *)
(*                           NonPersistentBoundOnce. The search ignores    *)
(*                           its claims, so a repeated channel binds one   *)
(*                           non-persistent stored record twice. Changed   *)
(*                           by decision 103 (decision 118): the invariant *)
(*                           was DistinctRecordsPerBind.                   *)
(*   IncomingTwice = TRUE    IncomingTwiceUnsafe refutes IncomingHeld. The *)
(*                           produce search forces the non-persistent      *)
(*                           incoming record at every matching position of *)
(*                           its collection. Changed by decision 103       *)
(*                           (decision 118): the invariant was             *)
(*                           IncomingBoundOnce.                            *)
(*   HeadOnIncomingOnly      HeadOnIncomingOnlyUnsafe refutes              *)
(*   = TRUE                  JoinFiresHeadsOnly. The produce side checks   *)
(*                           the head of the collection of the incoming    *)
(*                           record only.                                  *)
(*   PerPhaseCount = TRUE    JoinPerPhaseCountUnsafe refutes               *)
(*                           TxnFiresBounded. The plan and the drain after *)
(*                           it start their own counts at 0.               *)
(*   DropRestore = TRUE      RestoreDroppedUnsafe refutes NoRecordLost. A  *)
(*                           peek fire pushes no second sends.             *)
(*   FreshOrds = TRUE        JoinReissueFreshOrdsUnsafe refutes            *)
(*                           ReissueKeepsBindOrds. A re-issued persistent  *)
(*                           receive carries no ords.                      *)
(*   LazyTouch = TRUE        LazyTouchUnsafe refutes                       *)
(*                           TouchedCountersBumped. TOUCH bumps the own    *)
(*                           collections only, so a fire on another        *)
(*                           collection of the group stages on a counter   *)
(*                           with no bump.                                 *)
(*   PeekPersist = TRUE      PeekPersistUnsafe refutes                     *)
(*                           CascadeMeasureDecreases. A receive can peek   *)
(*                           and persist: its fire keeps the waiter and    *)
(*                           sends its record again, so mu stays.          *)
(*   PersistentClaimed       PersistentClaimedUnsafe refutes               *)
(*   = TRUE                  QuiescentNoPendingMatch. The engine claims    *)
(*                           every pick, as before decision 118. So a join *)
(*                           of two positions on one persistent record     *)
(*                           parks, and the spec search fires it. Added by *)
(*                           decision 103 (decision 118).                  *)
(*   FreeMemberClaimsRecord  FreeClassUnclaimedUnsafe refutes              *)
(*   <- FreeMemberUnclaimed  NonPersistentBoundOnce. The control is an     *)
(*                           operator replacement in its cfg, not a        *)
(*                           CONSTANT flag. A non-persistent member of a   *)
(*                           set class with a persistent member claims     *)
(*                           nothing (the rule before the C6               *)
(*                           arbitration). So the join <c1,c1> binds that  *)
(*                           member at both positions. The replacement     *)
(*                           also changes the spec search. Added by        *)
(*                           decision 103 (decision 118, C6 arbitration).  *)
(*                                                                         *)
(* Code (under rholang/src/rust/interpreter, the working tree of the model *)
(* alignment pass of 2026-10-07, spaces/join.rs with 4329 lines):          *)
(*   Win, AtHead, HeadsAll   spaces/join.rs:1679-1774                      *)
(*                           (KeyedSource::window), 1489-1585 (at_head,    *)
(*                           at_head_incoming) and 2284-2303 (all_at_head) *)
(*   Tag, PickIdx, Greedy,   spaces/join.rs:3081-3194 (search), 1392-1411  *)
(*   Fill, GuardOk           (claim), 3516-3535 (is_claimed) and 2407-2432 *)
(*                           (admits)                                      *)
(*   SearchProduce, InWin    spaces/join.rs:3270-3382 (search_produce)     *)
(*   Cascades                spaces/join.rs:2597-2602 (Engine::cascades)   *)
(*   Touch                   spaces/join.rs:2611-2655 (touch, touch_all)   *)
(*   Offer, OfferCands,      spaces/join.rs:3697-3855 (offer), 1807-1841   *)
(*   Store                   (offer_order), 2312-2391 (union_order) and    *)
(*                           3687-3707 (store)                             *)
(*   Receive, RcvStays       spaces/join.rs:3997-4190 and 4312-4362        *)
(*                           (receive, after_receive, park) and 2843-2907  *)
(*                           (class_candidates)                            *)
(*   PushSends               spaces/join.rs:3578-3627 (second_sends,       *)
(*                           push_work)                                    *)
(*   Retire, ClassDead       spaces/join.rs:2244-2296 (retire_classes,     *)
(*                           class_claims)                                 *)
(*   CascadeFire             spaces/join.rs:4587-4748 (lift_and_reoffer,   *)
(*                           lift)                                         *)
(*   NextFireIn, Drain       spaces/join.rs:4366-4408 and 4451-4483        *)
(*                           (next_fire, drain_and_cascade) and 1447-1487  *)
(*                           (cascade_order)                               *)
(*   RunJoinOwn, OpenPlan    spaces/join.rs:2671-2758 (Engine::run)        *)
(*   Pending, ResumeOf,      spaces/join.rs:4753-4908 (commit,             *)
(*   CommitWith, ReissueOp   settle_fields, resume)                        *)
(*   Scope                   spaces/join.rs:371-404 and 531-646 (classify, *)
(*                           join_scope)                                   *)
(* The alignment points with join.rs (DV-1 to DV-5, A1 to A5) are in the   *)
(* header of JoinCascade.v.                                                *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - Payloads a and b. A pattern accepts a payload or every payload      *)
(*     (any). The guard onlyB admits a list of values when every value is  *)
(*     b. A persistent receive never peeks (CONSUME_ARROWS), except under  *)
(*     the control PeekPersist. The class of a set member is its payload.  *)
(*   - Each record and waiter has an identity (the round of its first      *)
(*     operation). The bag and the set order by identity (DR-75 stands for *)
(*     it).                                                                *)
(*   - Added by decision 102: a random pick of play is a choice of the     *)
(*     round. The operators return every end state that some order of the  *)
(*     shuffle reaches. An offer on a bag or a set reaches every waiter    *)
(*     that fires under a pick, and it stores x only when a pick skips     *)
(*     every waiter. The set cascade does the same over the waiters of its *)
(*     dirty collections.                                                  *)
(*   - Added by decision 102 (plan v4, rule CR-30): with CascadeCanon, the *)
(*     set cascade reaches every waiter that fires under the DR-75-first   *)
(*     entry of each window. It fires nothing only when no waiter fires    *)
(*     under that entry.                                                   *)
(*   - The ord of an element is the counter value of its collection at its *)
(*     round (bump_value), or the carried ord of a re-issue.               *)
(*   - The groups of the model are in group order already, so the bind     *)
(*     order and the group order agree. The Rocq headline                  *)
(*     bind_permutation_restores_order covers the permutation.             *)
(*   - The cascade fire is one direct COMM of the stored records (Rocq     *)
(*     headline 11 equates it with the lift and the re-offer).             *)
(*   - The fuel of the recursive operators makes them total. A run out of  *)
(*     fuel stops like the cap.                                            *)
(*   - History variables replace the fire logs: hOk, dOk, iOk, mOk and tOk *)
(*     say that every fire and every drain step so far kept its rule. nOk  *)
(*     says that every transaction so far kept each record that it held,   *)
(*     and fOk that every transaction so far made at most Cap fires. The   *)
(*     booleans keep the state space small: a set of identities would tell *)
(*     apart states that differ only in their history.                     *)
(*   - Added by decision 103 (decision 118): the history variable pOk says *)
(*     that no fire so far bound a persistent record at two positions.     *)
(*     Only the witnesses NoPersistentTwice and NoFreeClassTwice read it.  *)
(*     FreeClassTwiceReachable replaces PersistentTwice with               *)
(*     FreeClassTwice, so there pOk says that no fire so far bound a       *)
(*     non-persistent member and a persistent member of one set class at   *)
(*     two positions. Changed by decision 103 (decision 118, C6            *)
(*     arbitration): the witness NoFreeClassTwice also reads pOk.          *)
(*   - Identical settle jobs merge in the set jobs. A settle job is        *)
(*     idempotent, so the merge removes only a repeated settle.            *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    \* "queue", "stack", "bag" or "set".
    Discipline,
    \* The collections: a subset of {"c1", "c2"}.
    Colls,
    \* The payloads.
    Vals,
    \* The patterns: a payload or "any".
    Pats,
    \* The guards: "none" or "onlyB".
    Guards,
    \* The join groups: tuples of collections in group order.
    Groups,
    \* The rounds of one run: operations, re-entries and settle jobs.
    MaxOps,
    \* FIRE_ROUND_CAP of the model.
    Cap,
    \* TRUE: persistent sends, persistent receives and peek receives.
    Reissues,
    \* The Unsafe controls.
    NoCascade, PlainRestore, IgnoreClaims, IncomingTwice, HeadOnIncomingOnly,
    PerPhaseCount, DropRestore, FreshOrds, LazyTouch, PeekPersist, NoSetCascade,
    \* Added by decision 103 (decision 118): the Unsafe control of the claims of persistent entries.
    \* Changed by decision 102 (plan v4, rule CR-30): a constant follows, so this constant
    \* takes a comma.
    \* PersistentClaimed
    PersistentClaimed,
    \* Added by decision 102 (plan v4, rule CR-30): the cascade search takes the DR-75-first
    \* entry of each window (F3). Otherwise it takes any pick, as the receive and the offer
    \* do. Every cfg sets it TRUE.
    CascadeCanon

ASSUME Discipline \in {"queue", "stack", "bag", "set"}
ASSUME Colls \subseteq {"c1", "c2"} /\ Colls # {}
ASSUME Vals \subseteq {"a", "b"} /\ Vals # {}
ASSUME Pats \subseteq Vals \cup {"any"} /\ Pats # {}
ASSUME Guards \subseteq {"none", "onlyB"} /\ "none" \in Guards
ASSUME \A g \in Groups : g \in Seq(Colls) /\ Len(g) \in 1..2
ASSUME MaxOps \in 1..8
ASSUME Cap \in 1..4
\* Changed by decision 103 (decision 118): the new control PersistentClaimed.
\* ASSUME {Reissues, NoCascade, PlainRestore, IgnoreClaims, IncomingTwice, HeadOnIncomingOnly,
\*         PerPhaseCount, DropRestore, FreshOrds, LazyTouch, PeekPersist, NoSetCascade} \subseteq BOOLEAN
\* Changed by decision 102 (plan v4, rule CR-30): the new constant CascadeCanon.
\* ASSUME {Reissues, NoCascade, PlainRestore, IgnoreClaims, IncomingTwice, HeadOnIncomingOnly,
\*         PerPhaseCount, DropRestore, FreshOrds, LazyTouch, PeekPersist, NoSetCascade,
\*         PersistentClaimed} \subseteq BOOLEAN
ASSUME {Reissues, NoCascade, PlainRestore, IgnoreClaims, IncomingTwice, HeadOnIncomingOnly,
        PerPhaseCount, DropRestore, FreshOrds, LazyTouch, PeekPersist, NoSetCascade,
        PersistentClaimed, CascadeCanon} \subseteq BOOLEAN

VARIABLES
    \* The stored records.
    recs,
    \* The parked waiters.
    wtrs,
    \* The collection counters (the next value of each counter).
    cnt,
    \* The re-issued operations that wait for their next round.
    ops,
    \* The re-issued settle jobs (CascadeState).
    jobs,
    \* The rounds so far.
    rounds,
    \* History: every fire so far was at the heads of its group.
    hOk,
    \* Changed by decision 103 (decision 118): a persistent record can bind
    \* several positions of one fire, so the records of a fire can repeat.
    \* The text before:
    \* \* History: every fire so far bound distinct stored records.
    \* History: two non-persistent stored picks of one fire never share a
    \* record. On a set, two positions of one class need a persistent member
    \* of that class. A persistent entry is never blocked.
    dOk,
    \* History: every produce-side fire so far bound its incoming record once.
    iOk,
    \* History: every drain step so far that went on lowered mu.
    mOk,
    \* History: every staging so far followed a bump of its counter.
    tOk,
    \* History: every transaction so far kept each record that it held
    \* (stored, spent by a fire of a waiter that does not peek, or pending).
    nOk,
    \* History: every transaction so far made at most Cap fires.
    fOk,
    \* Added by decision 103 (decision 118). History: no fire so far bound a persistent record
    \* at two positions (the witness NoPersistentTwice).
    \* Added by decision 103 (decision 118, C6 arbitration): FreeClassTwiceReachable replaces
    \* PersistentTwice with FreeClassTwice. There pOk says that no fire so far bound a
    \* non-persistent member and a persistent member of one set class (the witness
    \* NoFreeClassTwice).
    pOk

\* Changed by decision 103 (decision 118): the history variable pOk.
\* vars == <<recs, wtrs, cnt, ops, jobs, rounds, hOk, dOk, iOk, mOk, tOk, nOk, fOk>>
vars == <<recs, wtrs, cnt, ops, jobs, rounds, hOk, dOk, iOk, mOk, tOk, nOk, fOk, pOk>>

NoOrd == -1
NoId == -1
\* The fuel of the recursive operators (more than the steps of a transaction).
Fuel == 4 * (MaxOps + Cap) + 8

NoRec == [id |-> NoId, coll |-> "c1", ord |-> NoOrd, val |-> "a", pers |-> FALSE]
NoW == [id |-> NoId, colls |-> << >>, pats |-> << >>, ords |-> << >>, guard |-> "none",
        pers |-> FALSE, peek |-> FALSE]
\* An operation: a produce of x or a receive of w, its carried ords and the
\* ghost ords of its first round.
NoOp == [kind |-> "none", x |-> NoRec, w |-> NoW, carried |-> NoOrd, cords |-> << >>,
         first |-> NoOrd, fords |-> << >>]

\* The group universes of the configurations (a .cfg file cannot write a
\* tuple, so the configurations substitute these operators for Groups).
AllGroups == {<<"c1">>, <<"c2">>, <<"c1", "c2">>, <<"c1", "c1">>}
JoinGroups == {<<"c1", "c2">>, <<"c1", "c1">>}
CrossGroups == {<<"c2">>, <<"c1", "c2">>}
RepeatGroups == {<<"c1">>, <<"c1", "c1">>}
SingleGroups == {<<"c1">>}
MixedGroups == {<<"c2">>, <<"c1", "c2">>, <<"c1", "c1">>}
RepeatOnlyGroups == {<<"c1", "c1">>}

Range(s) == {s[i] : i \in DOMAIN s}
Matches(p, v) == p = "any" \/ p = v
Min(S) == CHOOSE m \in S : \A k \in S : m <= k
UsesCounter == Discipline \in {"queue", "stack"}
\* A bag and a set have scan windows: every record, no head condition.
ScanShaped == Discipline \in {"bag", "set"}
IsSet == Discipline = "set"
\* CASCADES (decisions 11 and 50): the head shapes and the set. Unsafe
\* NoCascade removes every cascade, and Unsafe NoSetCascade removes the
\* cascade of the set.
Cascades == ~NoCascade /\ (Discipline \in {"queue", "stack"} \/ (IsSet /\ ~NoSetCascade))
\* Changed by decision 103 (decision 118, C6 arbitration): the claims hold record tags (RTag),
\* and Tag is the class tag. Tag itself does not change. The text before:
\* \* The claim tag of a record: the record, or the class of a set member (the
\* \* payload is the class key, so the members of a class bind alike).
\* The class tag of a record: the record, or the class of a set member (the payload is the
\* class key, so the members of a class bind alike).
Tag(r) == IF IsSet THEN <<r.coll, r.val>> ELSE <<r.coll, r.id>>
\* Added by decision 103 (decision 118, C6 arbitration): the record tag of a record. The claims
\* of the search hold record tags only. So TLC never compares a payload with an identity.
RTag(r) == <<r.coll, r.id>>
CollRank(c) == IF c = "c1" THEN 1 ELSE 2

-----------------------------------------------------------------------------
(* Windows and heads (plan section 3)                                      *)
(*                                                                         *)
(* TLC evaluates a LET definition again at each of its references, and an  *)
(* operator argument only once. So the operators pass each computed value  *)
(* to a helper operator as an argument.                                    *)

\* The visit order of the records: the oldest first (queue), the newest
\* first (stack), or the identity (bag and set, DR-75).
RKey(r) == CASE Discipline = "queue" -> r.ord
             [] Discipline = "stack" -> 0 - r.ord
             [] OTHER -> r.id
RBefore(r, s) == RKey(r) < RKey(s) \/ (RKey(r) = RKey(s) /\ r.id < s.id)

RECURSIVE RecSeq(_), RecSeqFrom(_, _)
RecSeqFrom(S, m) == <<m>> \o RecSeq(S \ {m})
RecSeq(S) ==
    IF S = {} THEN << >>
    ELSE RecSeqFrom(S, CHOOSE r \in S : \A s \in S \ {r} : RBefore(r, s))

\* m_W(c): the binds of W on c.
Mult(W, c) == Cardinality({i \in DOMAIN W.colls : W.colls[i] = c})

\* WIN(c, m): the first m records of c (head shapes), or all (bag and set:
\* the claims of a set skip the members of a claimed class).
WinOf(s, m) == IF ScanShaped THEN s ELSE SubSeq(s, 1, IF m < Len(s) THEN m ELSE Len(s))
Win(c, m, R) == WinOf(RecSeq({r \in R : r.coll = c}), m)

\* The head order of the waiters of c: by the ord of c (queue: the oldest
\* first, stack: the newest first), or by identity (bag and set).
FirstPos(W, c) == Min({i \in DOMAIN W.colls : W.colls[i] = c})
WKey(W, c) == CASE Discipline = "queue" -> W.ords[FirstPos(W, c)]
                [] Discipline = "stack" -> 0 - W.ords[FirstPos(W, c)]
                [] OTHER -> W.id
WBeforeK(W, V, kw, kv) == kw < kv \/ (kw = kv /\ W.id < V.id)
WBefore(W, V, c) == WBeforeK(W, V, WKey(W, c), WKey(V, c))

WaitersOf(c, WS) == {W \in WS : c \in Range(W.colls)}

RECURSIVE WSeq(_, _), WSeqFrom(_, _, _)
WSeqFrom(S, c, m) == <<m>> \o WSeq(S \ {m}, c)
WSeq(S, c) ==
    IF S = {} THEN << >>
    ELSE WSeqFrom(S, c, CHOOSE W \in S : \A V \in S \ {W} : WBefore(W, V, c))

\* AT-HEAD(W, c): W is ahead of every other waiter of c. With W not parked,
\* this is AT-HEAD-INCOMING. A bag and a set have no head condition.
AtHead(W, c, WS) == ScanShaped \/ \A V \in WaitersOf(c, WS) : V.id # W.id => WBefore(W, V, c)
HeadsOn(W, cs, WS) == \A c \in cs : AtHead(W, c, WS)
HeadsAll(W, WS) == HeadsOn(W, Range(W.colls), WS)

-----------------------------------------------------------------------------
(* The search (plan 3.4 and 3.5)                                           *)

Fail == [ok |-> FALSE, pos |-> 0, seq |-> << >>]

\* The indices of L whose record is unclaimed and matches p.
\* Changed by decision 103 (decision 118, C6 arbitration): the claims hold record tags, so the
\* test reads RTag. The text before:
\* PickIdx(L, p, claims) ==
\*     {k \in DOMAIN L : (IgnoreClaims \/ Tag(L[k]) \notin claims) /\ Matches(p, L[k].val)}
PickIdx(L, p, claims) ==
    {k \in DOMAIN L : (IgnoreClaims \/ RTag(L[k]) \notin claims) /\ Matches(p, L[k].val)}

\* Added by decision 102: the indices that the pick of a slot on c may take. A bag or a set
\* picks any index of a window that the group names once (DR-129 of the base). Head order and
\* the E1 list of a collection that the group names more than once (decision 103) take the
\* first index.
Choices(W, c, ks) == IF ScanShaped /\ Mult(W, c) = 1 THEN ks ELSE {Min(ks)}

\* Changed by decision 103 (decision 118, C6 arbitration): rule (a) replaces the set clause of
\* CR-9. Under that clause, a non-persistent member of a set class with a persistent member
\* claimed nothing, so it could bind two positions of one fire (finding F-118-2), and the code
\* refuses that fire at staging (FR-1'). The claims now hold record tags only (RTag).
\* ClassHasPersistent does not change.
\* The claims after a pick of e from the store R (decision 118, CR-9, with rule (a) of the C6
\* arbitration). A persistent record claims nothing, and no claim blocks it. A non-persistent
\* record claims its record (RTag). On a set it also claims its class (ClassTags). A class with
\* a persistent member in R (ClassHasPersistent) is never claimed as a class, so there the pick
\* claims its record only. R holds the incoming record x on the produce side, so a persistent x
\* of the class counts. pc = TRUE claims every pick and its class, as before decision 118 (the
\* Unsafe PersistentClaimed in the engine). On a queue, a stack and a bag,
\* ClassTags(R, e) = {RTag(e)} = {Tag(e)}, so the claims there do not change.
ClassHasPersistent(R, t) == \E r \in R : r.pers /\ Tag(r) = t
\* Added by decision 103 (decision 118, C6 arbitration): the record tags of the members of the
\* class of e in R. A claimed class is this set of tags. e is in R at every call, so the set
\* holds RTag(e).
ClassTags(R, e) == {RTag(r) : r \in {s \in R : Tag(s) = Tag(e)}}
\* Added by decision 103 (decision 118, C6 arbitration): the claim of a non-persistent member of
\* a set class with a persistent member. Rule (a) claims its record (FreeMemberClaimsRecord).
\* The Unsafe control FreeClassUnclaimedUnsafe replaces it (FreeMemberClaimsRecord <-
\* FreeMemberUnclaimed) with the rule before the C6 arbitration, which claims nothing. The
\* replacement changes the engine and the spec search together.
FreeMemberClaimsRecord == TRUE
FreeMemberUnclaimed == FALSE
Claim(claims, e, R, pc) ==
    IF pc THEN claims \cup ClassTags(R, e)
    ELSE IF e.pers THEN claims
    ELSE IF IsSet /\ ClassHasPersistent(R, Tag(e))
         THEN IF FreeMemberClaimsRecord THEN claims \cup {RTag(e)} ELSE claims
    ELSE claims \cup ClassTags(R, e)

\* Changed by decision 102: GREEDY returns the set of the results of every pick, and the claims
\* follow decision 103 (decision 118). The old form follows.
\* \* GREEDY: the slot i picks the first index of ks in its window L.
\* RECURSIVE Fill(_, _, _, _, _), FillAt(_, _, _, _, _, _), FillKs(_, _, _, _, _, _, _), FillPick(_, _, _, _, _, _)
\* FillPick(W, R, i, claims, acc, e) == Fill(W, R, i + 1, claims \cup {Tag(e)}, Append(acc, e))
\* FillKs(W, R, i, claims, acc, L, ks) == IF ks = {} THEN Fail ELSE FillPick(W, R, i, claims, acc, L[Min(ks)])
\* FillAt(W, R, i, claims, acc, L) == FillKs(W, R, i, claims, acc, L, PickIdx(L, W.pats[i], claims))
\* Fill(W, R, i, claims, acc) ==
\*     IF i > Len(W.colls) THEN [ok |-> TRUE, pos |-> 0, seq |-> acc]
\*     ELSE FillAt(W, R, i, claims, acc, Win(W.colls[i], Mult(W, W.colls[i]), R))
\*
\* \* GREEDY(W): one pick for each position in group order.
\* Greedy(W, R) == Fill(W, R, 1, {}, << >>)

\* GREEDY: the slot i picks an index of Choices in its window L, with the claim rule pc. The
\* result is the set of the fills of every pick.
\* Changed by decision 102 (plan v4, rule CR-30): the operators take the mode canon. With
\* canon, the slot takes the first index of ks, which is the DR-75-first entry of its window
\* (F3).
RECURSIVE Fill(_, _, _, _, _, _, _), FillAt(_, _, _, _, _, _, _, _),
          FillKs(_, _, _, _, _, _, _, _, _), FillPick(_, _, _, _, _, _, _, _)
FillPick(W, R, i, claims, acc, e, pc, canon) ==
    Fill(W, R, i + 1, Claim(claims, e, R, pc), Append(acc, e), pc, canon)
FillKs(W, R, i, claims, acc, L, ks, pc, canon) ==
    IF ks = {} THEN {Fail}
    ELSE UNION {FillPick(W, R, i, claims, acc, L[k], pc, canon)
                : k \in (IF canon THEN {Min(ks)} ELSE Choices(W, W.colls[i], ks))}
FillAt(W, R, i, claims, acc, L, pc, canon) ==
    FillKs(W, R, i, claims, acc, L, PickIdx(L, W.pats[i], claims), pc, canon)
Fill(W, R, i, claims, acc, pc, canon) ==
    IF i > Len(W.colls) THEN {[ok |-> TRUE, pos |-> 0, seq |-> acc]}
    ELSE FillAt(W, R, i, claims, acc, Win(W.colls[i], Mult(W, W.colls[i]), R), pc, canon)

\* GREEDY(W): one pick for each position in group order, with the claim rule pc.
\* Changed by decision 102 (plan v4, rule CR-30): Greedy takes every pick (canon = FALSE).
Greedy(W, R, pc) == Fill(W, R, 1, {}, << >>, pc, FALSE)

\* Added by decision 102 (plan v4, rule CR-30): GREEDY of the cascade search. Each slot takes
\* the DR-75-first entry of its window (F3). {Min(ks)} is a subset of Choices(W, c, ks), so
\* GreedyCanon(W, R, pc) is a subset of Greedy(W, R, pc).
GreedyCanon(W, R, pc) == Fill(W, R, 1, {}, << >>, pc, TRUE)

\* admits_all: the guard once, over the values.
GuardOk(W, s) == W.guard = "none" \/ \A i \in DOMAIN s : s[i].val = "b"

\* Added by decision 102: the result g of a search of W fires, with the waiters WS: the heads,
\* the match and the guard once (decision 37).
FiresR(W, WS, g) == HeadsAll(W, WS) /\ g.ok /\ GuardOk(W, g.seq)

\* The positions where the produce search forces the incoming record x.
Forced(W, x, p, i) ==
    IF IncomingTwice THEN W.colls[i] = x.coll /\ Matches(W.pats[i], x.val) ELSE i = p

\* x lies in the window of its collection with x placed. Changed by decision 103 (decision 118):
\* InWinL and InWin move above the produce fill, because ProdWin uses InWinL.
InWinL(x, L) == \E k \in DOMAIN L : L[k].id = x.id
InWin(W, x, R) == InWinL(x, Win(x.coll, Mult(W, x.coll), R \cup {x}))

\* Added by decision 103 (decision 118 E1): the list L of a position i other than the forced
\* position. On the collection of x, a persistent x that lies in the window comes first, and a
\* non-persistent x stays out.
ProdWin(W, x, i, L) ==
    IF x.pers /\ W.colls[i] = x.coll /\ InWinL(x, L)
    THEN <<x>> \o SelectSeq(L, LAMBDA e : e.id # x.id)
    ELSE SelectSeq(L, LAMBDA e : e.id # x.id)

\* Changed by decision 102: the produce fill returns the set of the results of every pick, and
\* decision 103 (decision 118) gives its claims and its E1 list. The old form follows.
\* RECURSIVE FillProd(_, _, _, _, _, _, _), FillProdAt(_, _, _, _, _, _, _, _),
\*           FillProdKs(_, _, _, _, _, _, _, _, _), FillProdPick(_, _, _, _, _, _, _, _)
\* FillProdPick(W, x, R, p, i, claims, acc, e) == FillProd(W, x, R, p, i + 1, claims \cup {Tag(e)}, Append(acc, e))
\* FillProdKs(W, x, R, p, i, claims, acc, L, ks) ==
\*     IF ks = {} THEN Fail ELSE FillProdPick(W, x, R, p, i, claims, acc, L[Min(ks)])
\* FillProdAt(W, x, R, p, i, claims, acc, L) == FillProdKs(W, x, R, p, i, claims, acc, L, PickIdx(L, W.pats[i], claims))
\* FillProd(W, x, R, p, i, claims, acc) ==
\*     IF i > Len(W.colls) THEN [ok |-> TRUE, pos |-> p, seq |-> acc]
\*     ELSE IF Forced(W, x, p, i) THEN FillProd(W, x, R, p, i + 1, claims, Append(acc, x))
\*     ELSE FillProdAt(W, x, R, p, i, claims, acc,
\*                     SelectSeq(Win(W.colls[i], Mult(W, W.colls[i]), R \cup {x}), LAMBDA e : e.id # x.id))
RECURSIVE FillProd(_, _, _, _, _, _, _, _), FillProdAt(_, _, _, _, _, _, _, _, _),
          FillProdKs(_, _, _, _, _, _, _, _, _, _), FillProdPick(_, _, _, _, _, _, _, _, _)
FillProdPick(W, x, R, p, i, claims, acc, e, pc) ==
    FillProd(W, x, R, p, i + 1, Claim(claims, e, R \cup {x}, pc), Append(acc, e), pc)
FillProdKs(W, x, R, p, i, claims, acc, L, ks, pc) ==
    IF ks = {} THEN {Fail}
    ELSE UNION {FillProdPick(W, x, R, p, i, claims, acc, L[k], pc) : k \in Choices(W, W.colls[i], ks)}
FillProdAt(W, x, R, p, i, claims, acc, L, pc) ==
    FillProdKs(W, x, R, p, i, claims, acc, L, PickIdx(L, W.pats[i], claims), pc)
FillProd(W, x, R, p, i, claims, acc, pc) ==
    IF i > Len(W.colls) THEN {[ok |-> TRUE, pos |-> p, seq |-> acc]}
    ELSE IF Forced(W, x, p, i) THEN FillProd(W, x, R, p, i + 1, claims, Append(acc, x), pc)
    ELSE FillProdAt(W, x, R, p, i, claims, acc,
                    ProdWin(W, x, i, Win(W.colls[i], Mult(W, W.colls[i]), R \cup {x})), pc)

\* Changed by decision 102: the position retry runs over the set of the results, and the claims
\* start with the claim of x by decision 103 (decision 118). The old form follows.
\* RECURSIVE SearchAt(_, _, _, _), SearchAtF(_, _, _, _, _)
\* SearchAtF(W, x, R, ps, f) ==
\*     IF Matches(W.pats[Head(ps)], x.val) /\ InWin(W, x, R) /\ f.ok THEN f ELSE SearchAt(W, x, R, Tail(ps))
\* SearchAt(W, x, R, ps) ==
\*     IF ps = << >> THEN Fail ELSE SearchAtF(W, x, R, ps, FillProd(W, x, R, Head(ps), 1, {Tag(x)}, << >>))
\*
\* \* SEARCH-PRODUCE(W, x): the positions of the collection of x, in group
\* \* order (the position retry).
\* SearchProduce(W, x, R) ==
\*     SearchAt(W, x, R, SelectSeq([i \in 1..Len(W.colls) |-> i], LAMBDA i : W.colls[i] = x.coll))
\* A position p of the retry gives its results that are ok. A pick at p that is not ok moves to
\* the next position.
RECURSIVE SearchAt(_, _, _, _, _), SearchAtF(_, _, _, _, _, _)
SearchAtF(W, x, R, ps, fs, pc) ==
    {f \in fs : f.ok} \cup (IF \E f \in fs : ~f.ok THEN SearchAt(W, x, R, Tail(ps), pc) ELSE {})
SearchAt(W, x, R, ps, pc) ==
    IF ps = << >> THEN {Fail}
    ELSE IF Matches(W.pats[Head(ps)], x.val) /\ InWin(W, x, R)
         THEN SearchAtF(W, x, R, ps, FillProd(W, x, R, Head(ps), 1, Claim({}, x, R \cup {x}, pc), << >>, pc), pc)
         ELSE SearchAt(W, x, R, Tail(ps), pc)

\* SEARCH-PRODUCE(W, x): the positions of the collection of x, in group
\* order (the position retry), with the claim rule pc.
SearchProduce(W, x, R, pc) ==
    SearchAt(W, x, R, SelectSeq([i \in 1..Len(W.colls) |-> i], LAMBDA i : W.colls[i] = x.coll), pc)

\* The head condition of the produce side.
HeadsReq(W, x) == IF HeadOnIncomingOnly THEN {x.coll} ELSE Range(W.colls)

\* Changed by decision 102: OFFER takes the set of the results of each candidate (OfferPicks).
\* FiresWithS(W, x, WS, s) == IF HeadsOn(W, HeadsReq(W, x), WS) /\ s.ok /\ GuardOk(W, s.seq) THEN s ELSE Fail
\* FiresWith(W, x, R, WS) == FiresWithS(W, x, WS, SearchProduce(W, x, R))
\* Added by decision 102: the result f of the produce search of W fires: the heads, the match
\* and the guard once (decision 37).
FiresP(W, x, WS, f) == HeadsOn(W, HeadsReq(W, x), WS) /\ f.ok /\ GuardOk(W, f.seq)

\* The candidates of OFFER: the heads of the collection of x (head shapes)
\* or every waiter of it (bag and set), in the head order.
OfferCandsQ(x, WS, q) == IF ScanShaped THEN q ELSE SelectSeq(q, LAMBDA W : AtHead(W, x.coll, WS))
OfferCands(x, WS) == OfferCandsQ(x, WS, WSeq(WaitersOf(x.coll, WS), x.coll))

NoPick == [ok |-> FALSE, w |-> NoW, pos |-> 0, seq |-> << >>]

\* Changed by decision 102: OFFER picks among its candidates at random on a bag or a set. The
\* old form follows.
\* RECURSIVE OfferPick(_, _, _, _), OfferPickF(_, _, _, _, _)
\* OfferPickF(x, R, WS, cs, f) ==
\*     IF f.ok THEN [ok |-> TRUE, w |-> Head(cs), pos |-> f.pos, seq |-> f.seq] ELSE OfferPick(x, R, WS, Tail(cs))
\* OfferPick(x, R, WS, cs) ==
\*     IF cs = << >> THEN NoPick ELSE OfferPickF(x, R, WS, cs, FiresWith(Head(cs), x, R, WS))
\* Added by decision 102: the picks of the candidate W with the results fs of its produce
\* search, and whether a result skips W.
PicksOf(W, x, WS, fs) ==
    {[ok |-> TRUE, w |-> W, pos |-> f.pos, seq |-> f.seq] : f \in {g \in fs : FiresP(W, x, WS, g)}}
Skips(W, x, WS, fs) == \E f \in fs : ~FiresP(W, x, WS, f)

\* The picks of OFFER in head order (queue and stack): the first candidate that fires.
RECURSIVE OfferPicksSeq(_, _, _, _, _), OfferPicksSeqF(_, _, _, _, _, _)
OfferPicksSeqF(x, R, WS, cs, pc, fs) ==
    PicksOf(Head(cs), x, WS, fs) \cup (IF Skips(Head(cs), x, WS, fs) THEN OfferPicksSeq(x, R, WS, Tail(cs), pc) ELSE {})
OfferPicksSeq(x, R, WS, cs, pc) ==
    IF cs = << >> THEN {NoPick} ELSE OfferPicksSeqF(x, R, WS, cs, pc, SearchProduce(Head(cs), x, R, pc))

\* The picks of OFFER on a bag or a set (union_order shuffled, decision 102): any candidate that
\* fires under a pick, and the store of x only when a pick skips every candidate. fsOf maps the
\* index of each candidate to the results of its produce search.
OfferPicksScanF(x, WS, cs, fsOf) ==
    UNION {PicksOf(cs[i], x, WS, fsOf[i]) : i \in DOMAIN cs}
    \cup (IF \A i \in DOMAIN cs : Skips(cs[i], x, WS, fsOf[i]) THEN {NoPick} ELSE {})
OfferPicksScan(x, R, WS, cs, pc) ==
    OfferPicksScanF(x, WS, cs, [i \in DOMAIN cs |-> SearchProduce(cs[i], x, R, pc)])

OfferPicks(x, R, WS, cs, pc) ==
    IF ScanShaped THEN OfferPicksScan(x, R, WS, cs, pc) ELSE OfferPicksSeq(x, R, WS, cs, pc)

\* Changed by decision 102: a waiter is fireable when every pick of its search fires. A pick
\* that its guard refuses can leave it parked (decision 37), as base play parks a receive that
\* its commit vetoes. The spec search uses the claims of decision 118. The old form follows.
\* \* FIREABLE(W): the heads, GREEDY and the guard once.
\* FireableG(W, WS, g) == HeadsAll(W, WS) /\ g.ok /\ GuardOk(W, g.seq)
\* Fireable(W, R, WS) == FireableG(W, WS, Greedy(W, R))
\* FIREABLE(W): the heads, GREEDY and the guard once, for every pick.
FireableG(W, WS, gs) == \A g \in gs : FiresR(W, WS, g)
Fireable(W, R, WS) == FireableG(W, WS, Greedy(W, R, FALSE))

-----------------------------------------------------------------------------
(* The engine (plan section 4)                                             *)

\* FireRounds::after_fire with the fire count after this fire.
AfterFire(n, isSpent, stays) ==
    IF isSpent THEN "done" ELSE IF stays THEN "divergent" ELSE IF Cap <= n THEN "cap" ELSE "continue"

\* One fire count for each transaction (decision 49). Unsafe PerPhaseCount
\* starts each phase at 0.
PhaseCount(n) == IF PerPhaseCount THEN 0 ELSE n

\* TOUCH (plan 4.3): the bump of each collection of cs before its first
\* staging, once in a transaction. Unsafe LazyTouch bumps the own
\* collections only.
TouchNew(J, new) ==
    [J EXCEPT !.bumps = J.bumps \cup new,
              !.cnt = [c \in Colls |-> IF c \in new THEN J.cnt[c] + 1 ELSE J.cnt[c]]]
Touch(J, cs, own) == TouchNew(J, {c \in cs : UsesCounter /\ (LazyTouch => c \in own) /\ c \notin J.bumps})

\* The check of a staging on the collections cs.
Staged(cs, J) == UsesCounter => cs \subseteq J.bumps

\* STORE of the element x = [rec, own].
StoreAt(J1, x) ==
    [J1 EXCEPT !.recs = J1.recs \cup {x.rec}, !.dirty = J1.dirty \cup {x.rec.coll},
               !.own = IF x.own THEN "stored" ELSE J1.own,
               !.tOk = J1.tOk /\ Staged({x.rec.coll}, J1),
               !.bornT = J1.bornT \cup {x.rec.id}]
Store(J, x, own) == StoreAt(Touch(J, {x.rec.coll}, own), x)

\* A fire of W over seq. inc is the position of the incoming record (0 for
\* none) and xid its identity. The stored records of the other positions
\* that are not persistent leave the store, and W leaves unless it is
\* persistent. The ghosts check the rules of the fire. J0 is J after TOUCH.
\* Changed by decision 103 (decision 118): the ghost dOk checks NonPersistentBoundOnce, the
\* ghost iOk checks IncomingHeld, and the ghost pOk records a persistent record at two
\* positions. The old form follows.
\* DistinctSp(seq, sp) == Cardinality({Tag(seq[i]) : i \in sp}) = Cardinality(sp)
\* FireAt(J0, J, W, seq, inc, xid) ==
\*     [J0 EXCEPT !.recs = J0.recs \ {seq[k] : k \in {i \in DOMAIN seq : i # inc /\ ~seq[i].pers}},
\*                !.wtrs = IF W.pers THEN J0.wtrs ELSE J0.wtrs \ {W},
\*                !.fired = J0.fired + 1, !.fires = J0.fires + 1,
\*                !.dirty = J0.dirty \cup Range(W.colls),
\*                !.hOk = J0.hOk /\ HeadsAll(W, J.wtrs),
\*                !.dOk = J0.dOk /\ DistinctSp(seq, {i \in DOMAIN seq : seq[i] \in J.recs}),
\*                !.iOk = J0.iOk /\ (xid # NoId => Cardinality({i \in DOMAIN seq : seq[i].id = xid}) = 1),
\*                !.tOk = J0.tOk /\ Staged(Range(W.colls), J0),
\*                !.spentT = IF W.peek THEN J0.spentT
\*                           ELSE J0.spentT \cup {seq[i].id : i \in {k \in DOMAIN seq : ~seq[k].pers}}]
\* Changed by decision 103 (decision 118, C6 arbitration): rule (a) binds a non-persistent
\* record at most once on a set too, so the record clause covers every discipline. On a queue,
\* a stack and a bag the new form equals the old one.
\* NonPersistentBoundOnce on the stored positions sp of a fire with the store R: a
\* non-persistent record fills at most one position, on every discipline. On a set, two
\* positions of one class need a persistent member of that class in R.
BoundOnce(seq, sp, R) ==
    \A i, j \in sp : i # j =>
        /\ ~seq[i].pers => seq[i].id # seq[j].id
        /\ (IsSet /\ Tag(seq[i]) = Tag(seq[j])) => ClassHasPersistent(R, Tag(seq[i]))
\* IncomingHeld at the positions xs of the incoming record, which is seq[inc]: it fills a
\* position, and a non-persistent incoming record fills one.
HeldAt(seq, inc, xs) == xs # {} /\ (~seq[inc].pers => Cardinality(xs) = 1)
\* A persistent record at two positions (the witness of decision 118).
PersistentTwice(seq) == \E i, j \in DOMAIN seq : i # j /\ seq[i].pers /\ seq[i].id = seq[j].id
\* Added by decision 103 (decision 118, C6 arbitration): a non-persistent member and a persistent
\* member of one set class at two positions (the witness of rule (a)). The cfg
\* FreeClassTwiceReachable replaces PersistentTwice with this operator, so the ghost pOk records
\* such a fire there.
FreeClassTwice(seq) ==
    IsSet /\ \E i, j \in DOMAIN seq : i # j /\ Tag(seq[i]) = Tag(seq[j]) /\ ~seq[i].pers /\ seq[j].pers
FireAt(J0, J, W, seq, inc, xid) ==
    [J0 EXCEPT !.recs = J0.recs \ {seq[k] : k \in {i \in DOMAIN seq : i # inc /\ ~seq[i].pers}},
               !.wtrs = IF W.pers THEN J0.wtrs ELSE J0.wtrs \ {W},
               !.fired = J0.fired + 1, !.fires = J0.fires + 1,
               !.dirty = J0.dirty \cup Range(W.colls),
               !.hOk = J0.hOk /\ HeadsAll(W, J.wtrs),
               !.dOk = J0.dOk /\ BoundOnce(seq, {i \in DOMAIN seq : seq[i] \in J.recs}, J.recs),
               !.iOk = J0.iOk /\ (xid # NoId => HeldAt(seq, inc, {i \in DOMAIN seq : seq[i].id = xid})),
               !.pOk = J0.pOk /\ ~PersistentTwice(seq),
               !.tOk = J0.tOk /\ Staged(Range(W.colls), J0),
               !.spentT = IF W.peek THEN J0.spentT
                          ELSE J0.spentT \cup {seq[i].id : i \in {k \in DOMAIN seq : ~seq[k].pers}}]
Fire(J, W, seq, inc, xid, own) == FireAt(Touch(J, Range(W.colls), own), J, W, seq, inc, xid)

\* Added by decision 102 (decision 119 G1): a single-bind fire of W on a set that does not
\* peek dispatches the member of its class with the least identity (class_candidates in DR-75
\* order). A peek binds the representative alone (Rule E). R is the store before the fire.
ClassOf(R, e) == {r \in R : Tag(r) = Tag(e)}
LeastOf(S) == CHOOSE m \in S : \A r \in S : m.id <= r.id
G1Seq(W, seq, R) ==
    IF IsSet /\ ~W.peek /\ Len(seq) = 1 /\ seq[1] \in R THEN <<LeastOf(ClassOf(R, seq[1]))>> ELSE seq

\* PUSH-SECOND-SENDS (plan 4.8, decision 30, DV-3): in receipt order on top
\* of the work, the element in flight at the incoming position, whether or
\* not it is persistent, and each other result that is not persistent. The
\* incoming position of the cascade is in flight only when the cascade
\* lifted its record (inflight). The element in flight keeps its own flag.
\* Unsafe DropRestore pushes nothing.
PushIdx(seq, inc, inflight) ==
    SelectSeq([i \in 1..Len(seq) |-> i], LAMBDA i : IF i = inc THEN inflight ELSE ~seq[i].pers)
PushFrom(J, seq, inc, ownInc, idx) ==
    [J EXCEPT !.work = [k \in DOMAIN idx |-> [rec |-> seq[idx[k]], own |-> ownInc /\ idx[k] = inc]] \o J.work]
PushSends(J, seq, inc, ownInc, inflight) ==
    IF DropRestore THEN J ELSE PushFrom(J, seq, inc, ownInc, PushIdx(seq, inc, inflight))

\* RETIRE-SET-BINDS (plan 4.7, decision 50): after a set fire that does not
\* peek, every other stored member of each bound class that is not
\* persistent leaves the store (K_retire) and is spent. J1 is the state
\* after the fire.
ClassDead(R, seq) == {r \in R : ~r.pers /\ \E i \in DOMAIN seq : r.coll = seq[i].coll /\ r.val = seq[i].val}
RetireDead(J1, dead) == [J1 EXCEPT !.recs = J1.recs \ dead, !.spentT = J1.spentT \cup {r.id : r \in dead}]
Retire(J1, W, seq) == IF IsSet /\ ~W.peek THEN RetireDead(J1, ClassDead(J1.recs, seq)) ELSE J1

\* The partner of a consume-side fire stays (FireRounds::after_fire): the
\* waiter peeks, or every bound record is persistent. A single-bind fire on
\* a set binds the class of its representative, so its partner stays when a
\* member of the class is persistent (class_candidates). R is the store
\* before the fire.
RcvStays(W, seq, R) ==
    W.peek \/ IF IsSet /\ Len(seq) = 1
              THEN \E r \in R : r.pers /\ r.coll = seq[1].coll /\ r.val = seq[1].val
              ELSE \A i \in DOMAIN seq : seq[i].pers

\* OFFER(x) (plan 4.5): the first candidate that fires, or the store of x.
\* A persistent x goes on while its waiter left. pk is the pick, J1 the
\* state after the fire, J2 the state after its second sends, st the step.
\* A divergent pair of the element leaves it in flight and does not stop the
\* transaction (plan 4.10). Only the cap stops it.
\* Changed by decision 102: OFFER returns the set of its end states. The old form follows.
\* RECURSIVE Offer(_, _, _, _), OfferWith(_, _, _, _, _), OfferFired(_, _, _, _, _), OfferTail(_, _, _, _, _, _)
\* OfferTail(J2, x, own, fuel, W, st) ==
\*     IF W.peek THEN J2
\*     ELSE CASE st = "done" -> [J2 EXCEPT !.own = IF x.own THEN "spent" ELSE J2.own]
\*            [] st = "continue" -> Offer(J2, x, own, fuel - 1)
\*            [] st = "divergent" -> [J2 EXCEPT !.own = IF x.own THEN "flight" ELSE J2.own]
\*            [] OTHER -> [J2 EXCEPT !.stop = TRUE, !.own = IF x.own THEN "flight" ELSE J2.own]
\* OfferFired(x, own, fuel, pk, J1) ==
\*     OfferTail(IF pk.w.peek THEN PushSends(J1, pk.seq, pk.pos, x.own, TRUE) ELSE Retire(J1, pk.w, pk.seq), x, own, fuel, pk.w,
\*               AfterFire(J1.fired, ~x.rec.pers /\ ~pk.w.peek, pk.w.pers))
\* OfferWith(J, x, own, fuel, pk) ==
\*     IF ~pk.ok THEN Store(J, x, own)
\*     ELSE OfferFired(x, own, fuel, pk, Fire(J, pk.w, pk.seq, pk.pos, x.rec.id, own))
\* Offer(J, x, own, fuel) ==
\*     IF fuel = 0 THEN [J EXCEPT !.stop = TRUE, !.own = IF x.own THEN "flight" ELSE J.own]
\*     ELSE OfferWith(J, x, own, fuel, OfferPick(x.rec, J.recs, J.wtrs, OfferCands(x.rec, J.wtrs)))
RECURSIVE Offer(_, _, _, _), OfferWith(_, _, _, _, _), OfferFired(_, _, _, _, _), OfferTail(_, _, _, _, _, _)
OfferTail(J2, x, own, fuel, W, st) ==
    IF W.peek THEN {J2}
    ELSE CASE st = "done" -> {[J2 EXCEPT !.own = IF x.own THEN "spent" ELSE J2.own]}
           [] st = "continue" -> Offer(J2, x, own, fuel - 1)
           [] st = "divergent" -> {[J2 EXCEPT !.own = IF x.own THEN "flight" ELSE J2.own]}
           [] OTHER -> {[J2 EXCEPT !.stop = TRUE, !.own = IF x.own THEN "flight" ELSE J2.own]}
OfferFired(x, own, fuel, pk, J1) ==
    OfferTail(IF pk.w.peek THEN PushSends(J1, pk.seq, pk.pos, x.own, TRUE) ELSE Retire(J1, pk.w, pk.seq), x, own, fuel, pk.w,
              AfterFire(J1.fired, ~x.rec.pers /\ ~pk.w.peek, pk.w.pers))
OfferWith(J, x, own, fuel, pk) ==
    IF ~pk.ok THEN {Store(J, x, own)}
    ELSE OfferFired(x, own, fuel, pk, Fire(J, pk.w, pk.seq, pk.pos, x.rec.id, own))
Offer(J, x, own, fuel) ==
    IF fuel = 0 THEN {[J EXCEPT !.stop = TRUE, !.own = IF x.own THEN "flight" ELSE J.own]}
    ELSE UNION {OfferWith(J, x, own, fuel, pk) :
                    pk \in OfferPicks(x.rec, J.recs, J.wtrs, OfferCands(x.rec, J.wtrs), PersistentClaimed)}

\* RECEIVE(W) (plan 4.4): AT-HEAD-INCOMING, GREEDY and the guard, or the
\* park of W. g is the greedy search, J1 the state after the fire. A
\* divergent pair leaves W in flight and does not stop the transaction (plan
\* 4.10). Only the cap stops it.
\* Changed by decision 102: RECEIVE returns the set of its end states, and a single-bind fire on
\* a set that does not peek dispatches by G1 (decision 119). The old form follows.
\* RECURSIVE Receive(_, _, _, _), ReceiveWith(_, _, _, _, _), ReceiveFired(_, _, _, _, _, _), ReceiveTail(_, _, _, _, _)
\* ReceiveTail(J2, W, own, fuel, st) ==
\*     CASE st = "done" -> [J2 EXCEPT !.own = "spent"]
\*       [] st = "continue" -> Receive(J2, W, own, fuel - 1)
\*       [] st = "divergent" -> [J2 EXCEPT !.own = "flight"]
\*       [] OTHER -> [J2 EXCEPT !.stop = TRUE, !.own = "flight"]
\* ReceiveFired(W, own, fuel, g, J1, stays) ==
\*     ReceiveTail(IF W.peek THEN PushSends(J1, g.seq, 0, FALSE, FALSE) ELSE Retire(J1, W, g.seq), W, own, fuel,
\*                 AfterFire(J1.fired, ~W.pers, stays))
\* ReceiveWith(J, W, own, fuel, g) ==
\*     IF HeadsAll(W, J.wtrs) /\ g.ok /\ GuardOk(W, g.seq)
\*     THEN ReceiveFired(W, own, fuel, g, Fire(J, W, g.seq, 0, NoId, own), RcvStays(W, g.seq, J.recs))
\*     ELSE [J EXCEPT !.wtrs = J.wtrs \cup {W}, !.own = "parked", !.tOk = J.tOk /\ Staged(Range(W.colls), J)]
\* Receive(J, W, own, fuel) ==
\*     IF fuel = 0 THEN [J EXCEPT !.stop = TRUE, !.own = "flight"]
\*     ELSE ReceiveWith(J, W, own, fuel, Greedy(W, J.recs))
\* The park of W after a search that does not fire.
Park(J, W) == [J EXCEPT !.wtrs = J.wtrs \cup {W}, !.own = "parked", !.tOk = J.tOk /\ Staged(Range(W.colls), J)]
RECURSIVE Receive(_, _, _, _), ReceiveWith(_, _, _, _, _), ReceiveFired(_, _, _, _, _, _), ReceiveTail(_, _, _, _, _)
ReceiveTail(J2, W, own, fuel, st) ==
    CASE st = "done" -> {[J2 EXCEPT !.own = "spent"]}
      [] st = "continue" -> Receive(J2, W, own, fuel - 1)
      [] st = "divergent" -> {[J2 EXCEPT !.own = "flight"]}
      [] OTHER -> {[J2 EXCEPT !.stop = TRUE, !.own = "flight"]}
ReceiveFired(W, own, fuel, s, J1, stays) ==
    ReceiveTail(IF W.peek THEN PushSends(J1, s, 0, FALSE, FALSE) ELSE Retire(J1, W, s), W, own, fuel,
                AfterFire(J1.fired, ~W.pers, stays))
\* Each pick that fires gives a fire of its G1 values. A pick that does not fire parks W.
ReceiveWith(J, W, own, fuel, gs) ==
    UNION {ReceiveFired(W, own, fuel, s, Fire(J, W, s, 0, NoId, own), RcvStays(W, s, J.recs)) :
               s \in {G1Seq(W, g.seq, J.recs) : g \in {h \in gs : FiresR(W, J.wtrs, h)}}}
    \cup (IF \E h \in gs : ~FiresR(W, J.wtrs, h) THEN {Park(J, W)} ELSE {})
Receive(J, W, own, fuel) ==
    IF fuel = 0 THEN {[J EXCEPT !.stop = TRUE, !.own = "flight"]}
    ELSE ReceiveWith(J, W, own, fuel, Greedy(W, J.recs, PersistentClaimed))

\* NEXT-FIRE (plan 4.9): the dirty collections in ascending order, the
\* heads of each in head order (every waiter on a set, decision 50), the
\* first fireable one.
RECURSIVE CSeq(_), CSeqFrom(_, _)
CSeqFrom(S, m) == <<m>> \o CSeq(S \ {m})
CSeq(S) == IF S = {} THEN << >> ELSE CSeqFrom(S, CHOOSE c \in S : \A d \in S : CollRank(c) <= CollRank(d))

NoFire == [ok |-> FALSE, w |-> NoW, seq |-> << >>]

\* Changed by decision 102: NEXT-FIRE returns the set of the fires of every pick. The old form
\* follows.
\* RECURSIVE FindFire(_, _, _), FindFireG(_, _, _, _)
\* FindFireG(R, WS, q, g) ==
\*     IF HeadsAll(Head(q), WS) /\ g.ok /\ GuardOk(Head(q), g.seq) THEN [ok |-> TRUE, w |-> Head(q), seq |-> g.seq]
\*     ELSE FindFire(R, WS, Tail(q))
\* FindFire(R, WS, q) == IF q = << >> THEN NoFire ELSE FindFireG(R, WS, q, Greedy(Head(q), R))
\*
\* RECURSIVE NextFireIn(_, _, _), NextFireF(_, _, _, _)
\* NextFireF(R, WS, cs, f) == IF f.ok THEN f ELSE NextFireIn(R, WS, Tail(cs))
\* NextFireIn(R, WS, cs) ==
\*     IF cs = << >> THEN NoFire
\*     ELSE NextFireF(R, WS, cs, FindFire(R, WS, SelectSeq(WSeq(WaitersOf(Head(cs), WS), Head(cs)),
\*                                                        LAMBDA W : AtHead(W, Head(cs), WS))))
\* The head shapes: the waiters in head order, the first one that fires.
RECURSIVE FindFire(_, _, _), FindFireG(_, _, _, _)
FindFireG(R, WS, q, gs) ==
    {[ok |-> TRUE, w |-> Head(q), seq |-> g.seq] : g \in {h \in gs : FiresR(Head(q), WS, h)}}
    \cup (IF \E h \in gs : ~FiresR(Head(q), WS, h) THEN FindFire(R, WS, Tail(q)) ELSE {})
FindFire(R, WS, q) == IF q = << >> THEN {NoFire} ELSE FindFireG(R, WS, q, Greedy(Head(q), R, PersistentClaimed))

RECURSIVE NextFireIn(_, _, _), NextFireF(_, _, _, _)
NextFireF(R, WS, cs, fs) == {f \in fs : f.ok} \cup (IF \E f \in fs : ~f.ok THEN NextFireIn(R, WS, Tail(cs)) ELSE {})
NextFireIn(R, WS, cs) ==
    IF cs = << >> THEN {NoFire}
    ELSE NextFireF(R, WS, cs, FindFire(R, WS, SelectSeq(WSeq(WaitersOf(Head(cs), WS), Head(cs)),
                                                        LAMBDA W : AtHead(W, Head(cs), WS))))

\* Added by decision 102 (the cascade order of a set goes through union_order): the set cascade
\* fires any waiter of the dirty collections that fires under a pick, and it fires nothing
\* only when a pick skips every waiter. gsOf maps each waiter to the results of its search.
NextFiresScanF(WS, ws, gsOf) ==
    UNION {{[ok |-> TRUE, w |-> W, seq |-> g.seq] : g \in {h \in gsOf[W] : FiresR(W, WS, h)}} : W \in ws}
    \cup (IF \A W \in ws : \E h \in gsOf[W] : ~FiresR(W, WS, h) THEN {NoFire} ELSE {})
\* Changed by decision 102 (plan v4, rule CR-30): with CascadeCanon, the search of each waiter
\* takes the DR-75-first entry of each window (F3, GreedyCanon).
NextFiresScan(R, WS, ws) ==
    NextFiresScanF(WS, ws, [W \in ws |-> IF CascadeCanon THEN GreedyCanon(W, R, PersistentClaimed)
                                          ELSE Greedy(W, R, PersistentClaimed)])
\* NEXT-FIRE over the dirty collections.
NextFires(R, WS, dirty) ==
    IF ScanShaped THEN NextFiresScan(R, WS, UNION {WaitersOf(c, WS) : c \in dirty})
    ELSE NextFireIn(R, WS, CSeq(dirty))

\* LIFT-AND-REOFFER (plan 4.6) as one direct COMM: the first non-persistent
\* bound record is lifted and offered again, so every non-persistent bound
\* record leaves the store. A peek pushes the second sends: the re-offered
\* record is in flight only when it was lifted. A set fire that does not
\* peek retires the bound classes (decision 50).
FirstNp(seq) ==
    IF \E i \in DOMAIN seq : ~seq[i].pers THEN Min({i \in DOMAIN seq : ~seq[i].pers}) ELSE 1

CascadeEnd(J2, st) == IF st \in {"divergent", "cap"} THEN [J2 EXCEPT !.stop = TRUE] ELSE J2
CascadeFired(W, seq, vp, J1) ==
    CascadeEnd(IF W.peek THEN PushSends(J1, seq, vp, FALSE, ~seq[vp].pers) ELSE Retire(J1, W, seq),
               AfterFire(J1.fired, ~seq[vp].pers /\ ~W.peek, W.pers))
\* Changed by decision 102 (decision 119 G1): a single-bind cascade fire on a set that does not
\* peek dispatches the least member of its class. The old form follows.
\* CascadeFire(J, W, seq, own) == CascadeFired(W, seq, FirstNp(seq), Fire(J, W, seq, 0, NoId, own))
CascadeFireG(J, W, s, own) == CascadeFired(W, s, FirstNp(s), Fire(J, W, s, 0, NoId, own))
CascadeFire(J, W, seq, own) == CascadeFireG(J, W, G1Seq(W, seq, J.recs), own)

\* mu (plan 4.9).
Mu(J) == << Cardinality({W \in J.wtrs : ~W.pers}),
            Cardinality({r \in J.recs : ~r.pers}) + Len(J.work), Len(J.work) >>
LexLess(a, b) == a[1] < b[1] \/ (a[1] = b[1] /\ (a[2] < b[2] \/ (a[2] = b[2] /\ a[3] < b[3])))

\* A popped second send goes through OFFER (Unsafe PlainRestore stores it).
\* Changed by decision 102: OFFER returns a set, so the resend does too. The old form follows.
\* Resend(J0, x, own) == IF PlainRestore THEN Store(J0, x, own) ELSE Offer(J0, x, own, Fuel)
Resend(J0, x, own) == IF PlainRestore THEN {Store(J0, x, own)} ELSE Offer(J0, x, own, Fuel)

\* DRAIN-AND-CASCADE (plan 4.9): the pending second sends first, then the
\* cascade when the discipline cascades (Cascades). The cap check comes
\* before each step. A step that goes on checks mu (mu0 is the mu before
\* the step). Changed by decision 102: the drain returns the set of its end
\* states. The old form follows.
\* RECURSIVE Drain(_, _, _), DrainStep(_, _, _, _), DrainFire(_, _, _, _)
\* DrainStep(J1, mu0, own, fuel) ==
\*     Drain([J1 EXCEPT !.mOk = J1.mOk /\ (J1.stop \/ LexLess(Mu(J1), mu0))], own, fuel)
\* DrainFire(J, own, fuel, nf) ==
\*     IF ~nf.ok THEN J
\*     ELSE IF Cap <= J.fired THEN [J EXCEPT !.stop = TRUE]
\*     ELSE DrainStep(CascadeFire(J, nf.w, nf.seq, own), Mu(J), own, fuel - 1)
\* Drain(J, own, fuel) ==
\*     IF J.stop THEN J
\*     ELSE IF fuel = 0 THEN [J EXCEPT !.stop = TRUE]
\*     ELSE IF J.work # << >>
\*          THEN IF Cap <= J.fired THEN [J EXCEPT !.stop = TRUE]
\*               ELSE DrainStep(Resend([J EXCEPT !.work = Tail(J.work)],
\*                                     [rec |-> Head(J.work).rec, own |-> Head(J.work).own], own),
\*                              Mu(J), own, fuel - 1)
\*     ELSE IF ~Cascades THEN J
\*     ELSE DrainFire(J, own, fuel, NextFireIn(J.recs, J.wtrs, CSeq(J.dirty)))
RECURSIVE Drain(_, _, _), DrainStep(_, _, _, _), DrainFire(_, _, _, _)
DrainStep(J1, mu0, own, fuel) ==
    Drain([J1 EXCEPT !.mOk = J1.mOk /\ (J1.stop \/ LexLess(Mu(J1), mu0))], own, fuel)
DrainFire(J, own, fuel, nf) ==
    IF ~nf.ok THEN {J}
    ELSE IF Cap <= J.fired THEN {[J EXCEPT !.stop = TRUE]}
    ELSE DrainStep(CascadeFire(J, nf.w, nf.seq, own), Mu(J), own, fuel - 1)
Drain(J, own, fuel) ==
    IF J.stop THEN {J}
    ELSE IF fuel = 0 THEN {[J EXCEPT !.stop = TRUE]}
    ELSE IF J.work # << >>
         THEN IF Cap <= J.fired THEN {[J EXCEPT !.stop = TRUE]}
              ELSE UNION {DrainStep(J1, Mu(J), own, fuel - 1) :
                              J1 \in Resend([J EXCEPT !.work = Tail(J.work)],
                                            [rec |-> Head(J.work).rec, own |-> Head(J.work).own], own)}
    ELSE IF ~Cascades THEN {J}
    ELSE UNION {DrainFire(J, own, fuel, nf) : nf \in NextFires(J.recs, J.wtrs, J.dirty)}

-----------------------------------------------------------------------------
(* RUN-JOIN and COMMIT (plan 4.1, 4.2, 4.10, 4.11)                         *)

BumpValue(c) == IF UsesCounter THEN cnt[c] ELSE 0

\* The element of a round: the carried ords, or the counter values.
XOf(o) == [o.x EXCEPT !.ord = IF o.carried # NoOrd THEN o.carried ELSE BumpValue(o.x.coll)]
WOf(o) == [o.w EXCEPT !.ords = IF o.cords # << >> THEN o.cords
                                ELSE [i \in DOMAIN o.w.colls |-> BumpValue(o.w.colls[i])]]

OwnOf(o) == CASE o.kind = "produce" -> {o.x.coll}
              [] o.kind = "receive" -> Range(o.w.colls)
              [] OTHER -> {o.key}

KeyOf(o) == CASE o.kind = "produce" -> o.x.coll
              [] o.kind = "receive" -> o.w.colls[1]
              [] OTHER -> o.key

SeedsOf(o) == IF o.kind = "settle" THEN {o.key} \cup o.seeds \cup {o.pending[i].rec.coll : i \in DOMAIN o.pending}
              ELSE OwnOf(o)
PendingOf(o) == IF o.kind = "settle" THEN o.pending ELSE << >>

\* CLASSIFY: the closure of the seeds under the join groups (two levels
\* reach every collection of the model).
Close1(S, WS) == S \cup UNION {Range(W.colls) : W \in {V \in WS : Range(V.colls) \cap S # {}}}
Scope(o) == Close1(Close1(SeedsOf(o) \cup OwnOf(o), wtrs), wtrs)

\* REISSUE-OP (decision 43): a persistent element carries its first-round
\* ords. Unsafe FreshOrds drops the ords of a receive.
ReissueOp(o) ==
    CASE o.kind = "produce" ->
            [o EXCEPT !.carried = IF o.x.pers THEN XOf(o).ord ELSE NoOrd,
                      !.first = IF o.first = NoOrd THEN XOf(o).ord ELSE o.first]
      [] o.kind = "receive" ->
            [o EXCEPT !.cords = IF o.w.pers /\ ~FreshOrds THEN WOf(o).ords ELSE << >>,
                      !.fords = IF o.fords = << >> THEN WOf(o).ords ELSE o.fords]
      [] OTHER -> o.resume

\* The plan: OFFER of the produced element or RECEIVE of the waiter. A
\* settle job has no plan, and RunJoinOwn does not call Plan for it.
\* Changed by decision 102: the plan returns the set of its end states. The old form follows.
\* Plan(J, o, own) ==
\*     CASE o.kind = "produce" -> Offer(J, [rec |-> XOf(o), own |-> TRUE], own, Fuel)
\*       [] o.kind = "receive" -> Receive(J, WOf(o), own, Fuel)
\*       [] OTHER -> J
Plan(J, o, own) ==
    CASE o.kind = "produce" -> Offer(J, [rec |-> XOf(o), own |-> TRUE], own, Fuel)
      [] o.kind = "receive" -> Receive(J, WOf(o), own, Fuel)
      [] OTHER -> {J}

\* The state of a transaction at its start. Changed by decision 103 (decision 118): the ghost
\* pOk. The old form follows.
\* J0Of(o) == [recs |-> recs, wtrs |-> wtrs, cnt |-> cnt, bumps |-> {}, dirty |-> {},
\*             work |-> PendingOf(o), own |-> "not", stop |-> FALSE, fired |-> 0, fires |-> 0,
\*             hOk |-> hOk, dOk |-> dOk, iOk |-> iOk, mOk |-> mOk, tOk |-> tOk,
\*             bornT |-> {}, spentT |-> {}]
J0Of(o) == [recs |-> recs, wtrs |-> wtrs, cnt |-> cnt, bumps |-> {}, dirty |-> {},
            work |-> PendingOf(o), own |-> "not", stop |-> FALSE, fired |-> 0, fires |-> 0,
            hOk |-> hOk, dOk |-> dOk, iOk |-> iOk, mOk |-> mOk, tOk |-> tOk, pOk |-> pOk,
            bornT |-> {}, spentT |-> {}]

\* RUN-JOIN (plan 4.2, Engine::run): TOUCH the own collections. A settle job
\* has no plan: it makes its scope dirty, drains its pending second sends
\* and the cascade, and commits (DV-1). Another operation runs the settle
\* before the plan when its discipline cascades (J3), clears the dirty
\* collections, opens the plan only below the cap (open_plan, DV-2), runs
\* the plan (J5) and the drain after the plan.
\* Changed by decision 102: RUN-JOIN returns the set of the end states of the
\* transaction. The old form follows.
\* AfterPlan(own, J5) == Drain([J5 EXCEPT !.fired = PhaseCount(J5.fired)], own, Fuel)
\* OpenPlan(o, own, J4) ==
\*     IF Cap <= J4.fired THEN [J4 EXCEPT !.stop = TRUE]
\*     ELSE AfterPlan(own, Plan([J4 EXCEPT !.fired = PhaseCount(J4.fired)], o, own))
\* AfterPre(o, own, J3) == IF J3.stop THEN J3 ELSE OpenPlan(o, own, [J3 EXCEPT !.dirty = {}])
\* PreDrain(o, own, J1) == IF Cascades THEN Drain([J1 EXCEPT !.dirty = Scope(o)], own, Fuel) ELSE J1
\* RunJoinOwn(o, own) ==
\*     IF o.kind = "settle" THEN Drain([Touch(J0Of(o), own, own) EXCEPT !.dirty = Scope(o)], own, Fuel)
\*     ELSE AfterPre(o, own, PreDrain(o, own, Touch(J0Of(o), own, own)))
AfterPlan(own, J5) == Drain([J5 EXCEPT !.fired = PhaseCount(J5.fired)], own, Fuel)
OpenPlan(o, own, J4) ==
    IF Cap <= J4.fired THEN {[J4 EXCEPT !.stop = TRUE]}
    ELSE UNION {AfterPlan(own, J5) : J5 \in Plan([J4 EXCEPT !.fired = PhaseCount(J4.fired)], o, own)}
AfterPre(o, own, J3) == IF J3.stop THEN {J3} ELSE OpenPlan(o, own, [J3 EXCEPT !.dirty = {}])
PreDrain(o, own, J1) == IF Cascades THEN Drain([J1 EXCEPT !.dirty = Scope(o)], own, Fuel) ELSE {J1}
RunJoinOwn(o, own) ==
    IF o.kind = "settle" THEN Drain([Touch(J0Of(o), own, own) EXCEPT !.dirty = Scope(o)], own, Fuel)
    ELSE UNION {AfterPre(o, own, J3) : J3 \in PreDrain(o, own, Touch(J0Of(o), own, own))}
RunJoin(o) == RunJoinOwn(o, OwnOf(o))

\* The identities that a transaction holds: the stored records and the
\* pending second sends at its start, and the records that it stores.
HeldIds(o) == {r.id : r \in recs} \cup {PendingOf(o)[i].rec.id : i \in DOMAIN PendingOf(o)}
\* The identities that it keeps: stored, pending in the settle job, or spent.
KeptIds(J, pend) == {r.id : r \in J.recs} \cup {pend[i].rec.id : i \in DOMAIN pend} \cup J.spentT

\* The pending second sends of COMMIT (plan 4.10): the work without the
\* element of the operation, which returns to its operation.
Pending(J) == SelectSeq(J.work, LAMBDA q : ~q.own)
OwnInWork(J) == \E i \in DOMAIN J.work : J.work[i].own

\* The resume (Engine::resume): a settle job resumes the operation that it
\* carries. Another operation re-issues when its element is still in
\* flight, in the work or not started (decision 43).
ResumeOf(o, J) ==
    IF o.kind = "settle" THEN o.resume
    ELSE IF OwnInWork(J) \/ J.own \in {"not", "flight"} THEN ReissueOp(o) ELSE NoOp

\* COMMIT of the end state J with the resume res and the pending second
\* sends pend. opsBase and jobsBase are the outstanding re-issues without
\* the one that this round takes. A stop that owes work (a pending second
\* send, or a dirty collection of a discipline that cascades) commits a
\* settle job. Another end re-issues the resume only, so a stop that owes
\* nothing and resumes nothing re-issues nothing (DV-5).
CommitWith(o, J, res, pend, opsBase, jobsBase) ==
    /\ recs' = J.recs
    /\ wtrs' = J.wtrs
    /\ cnt' = J.cnt
    /\ hOk' = J.hOk
    /\ dOk' = J.dOk
    /\ iOk' = J.iOk
    /\ mOk' = J.mOk
    /\ tOk' = J.tOk
    /\ nOk' = (nOk /\ (HeldIds(o) \cup J.bornT) \subseteq KeptIds(J, pend))
    /\ fOk' = (fOk /\ J.fires <= Cap)
    \* Added by decision 103 (decision 118): the ghost of the witness.
    /\ pOk' = J.pOk
    /\ rounds' = rounds + 1
    /\ IF ~J.stop \/ (pend = << >> /\ ~(Cascades /\ J.dirty # {}))
       THEN /\ ops' = IF res # NoOp THEN opsBase \cup {res} ELSE opsBase
            /\ jobs' = jobsBase
       ELSE /\ ops' = opsBase
            /\ jobs' = jobsBase \cup {[kind |-> "settle", key |-> KeyOf(o),
                                      seeds |-> IF Cascades THEN J.dirty \ {KeyOf(o)} ELSE {},
                                      pending |-> pend, resume |-> res]}

CommitJ(o, J, opsBase, jobsBase) == CommitWith(o, J, ResumeOf(o, J), Pending(J), opsBase, jobsBase)

\* One round: RUN-JOIN, then COMMIT. Changed by decision 102: the round commits any end state
\* of RUN-JOIN. The old form follows.
\* Round(o, opsBase, jobsBase) == CommitJ(o, RunJoin(o), opsBase, jobsBase)
Round(o, opsBase, jobsBase) == \E J \in RunJoin(o) : CommitJ(o, J, opsBase, jobsBase)

-----------------------------------------------------------------------------
(* The rounds                                                              *)

Init ==
    /\ recs = {}
    /\ wtrs = {}
    /\ cnt = [c \in Colls |-> 0]
    /\ ops = {}
    /\ jobs = {}
    /\ rounds = 0
    /\ hOk = TRUE
    /\ dOk = TRUE
    /\ iOk = TRUE
    /\ mOk = TRUE
    /\ tOk = TRUE
    /\ nOk = TRUE
    /\ fOk = TRUE
    \* Added by decision 103 (decision 118): the ghost of the witness.
    /\ pOk = TRUE

\* A fresh send of v on c (persistent: !!). Its identity is its round.
Produce(c, v, pers) ==
    /\ rounds < MaxOps
    /\ pers => Reissues
    /\ Round([NoOp EXCEPT !.kind = "produce",
                          !.x = [id |-> rounds, coll |-> c, ord |-> NoOrd, val |-> v, pers |-> pers]],
             ops, jobs)

\* A fresh receive of the group g with the patterns ps (persistent: <=).
Consume(g, ps, gd, pers, peek) ==
    /\ rounds < MaxOps
    /\ (pers \/ peek) => Reissues
    /\ (pers /\ peek) => PeekPersist
    /\ Round([NoOp EXCEPT !.kind = "receive",
                          !.w = [id |-> rounds, colls |-> g, pats |-> ps, ords |-> << >>, guard |-> gd,
                                 pers |-> pers, peek |-> peek]],
             ops, jobs)

\* The next round of a re-issued operation.
Reenter(o) ==
    /\ rounds < MaxOps
    /\ Round(o, ops \ {o}, jobs)

\* The round of a re-issued settle job.
SettleJob(j) ==
    /\ rounds < MaxOps
    /\ Round(j, ops, jobs \ {j})

Next ==
    \/ \E c \in Colls, v \in Vals, pers \in BOOLEAN : Produce(c, v, pers)
    \/ \E g \in Groups : \E ps \in [1..Len(g) -> Pats], gd \in Guards, pers \in BOOLEAN, peek \in BOOLEAN :
          Consume(g, ps, gd, pers, peek)
    \/ \E o \in ops : Reenter(o)
    \/ \E j \in jobs : SettleJob(j)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants                                                              *)

TypeOK ==
    /\ \A r \in recs : r.coll \in Colls /\ r.val \in Vals /\ r.pers \in BOOLEAN /\ r.id \in 0..MaxOps
    /\ \A W \in wtrs : W.colls \in Groups /\ W.guard \in Guards /\ W.id \in 0..MaxOps
    /\ cnt \in [Colls -> 0..MaxOps]
    /\ rounds \in 0..MaxOps
    \* Changed by decision 103 (decision 118): the ghost pOk.
    \* /\ {hOk, dOk, iOk, mOk, tOk, nOk, fOk} \subseteq BOOLEAN
    /\ {hOk, dOk, iOk, mOk, tOk, nOk, fOk, pOk} \subseteq BOOLEAN

\* No outstanding re-issue: no re-issued operation and no settle job.
Quiet == ops = {} /\ jobs = {}

\* Changed by decision 102: INV_scan asks for a match of every pick, and Fireable asks that every
\* pick fires. The spec search uses the claims of decision 118 under every control. The old form
\* follows.
\* QuiescentNoPendingMatch ==
\*     Quiet => IF Discipline = "bag"
\*              THEN \A W \in wtrs : W.guard = "none" => ~Greedy(W, recs).ok
\*              ELSE \A W \in wtrs : ~Fireable(W, recs, wtrs)
\* Every pick of the search of W matches (the claims of decision 118).
SureMatch(W, R) == \A g \in Greedy(W, R, FALSE) : g.ok
QuiescentNoPendingMatch ==
    Quiet => IF Discipline = "bag"
             THEN \A W \in wtrs : W.guard = "none" => ~SureMatch(W, recs)
             ELSE \A W \in wtrs : ~Fireable(W, recs, wtrs)

JoinFiresHeadsOnly == hOk

\* Changed by decision 103 (decision 118): DistinctRecordsPerBind is restated as
\* NonPersistentBoundOnce. The old form follows.
\* DistinctRecordsPerBind == dOk
NonPersistentBoundOnce == dOk

\* Changed by decision 103 (decision 118): IncomingBoundOnce is restated as IncomingHeld. The old
\* form follows.
\* IncomingBoundOnce == iOk
IncomingHeld == iOk

\* Added by decision 103 (decision 118): the witness. JoinPersistentTwiceReachable must violate
\* it, so a fire binds a persistent record at two positions.
NoPersistentTwice == pOk

\* Added by decision 103 (decision 118, C6 arbitration): the witness of rule (a), an alias of
\* pOk. FreeClassTwiceReachable replaces PersistentTwice with FreeClassTwice and must violate
\* it, so a fire binds a non-persistent member and a persistent member of one set class.
NoFreeClassTwice == pOk

CascadeMeasureDecreases == mOk

TxnFiresBounded == fOk

NoRecordLost == nOk

\* The re-issued operations: the outstanding ones and the resumes of the
\* settle jobs.
Reissued == ops \cup {j.resume : j \in {j0 \in jobs : j0.resume # NoOp}}

ReissueKeepsBindOrds ==
    \A o \in Reissued :
        /\ o.kind = "produce" => IF o.x.pers THEN o.carried = o.first ELSE o.carried = NoOrd
        /\ o.kind = "receive" => IF o.w.pers THEN o.cords = o.fords ELSE o.cords = << >>

TouchedCountersBumped == tOk

=============================================================================
