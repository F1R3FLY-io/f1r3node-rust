------------------------- MODULE V6MergeSerialization -------------------------
(***************************************************************************)
(* DR-120 (gap G9): the interim merge rule of an offered-funded v6 shard,  *)
(* as a bounded model (v4 section 6).                                      *)
(*                                                                         *)
(* A scenario is one merge scope: deploy chains grouped into branches, the *)
(* base value of one IntegerAdd number channel L, and whether the base     *)
(* holds one number on a single-value cell V. Every validator computes the *)
(* slow path and the fast path as deterministic operators of the scenario  *)
(* and of one arrival permutation of the branches. Init stores both        *)
(* results for every arrival permutation, and the invariants compare them. *)
(*                                                                         *)
(* The slow path is dev's settled and base partitions, the S3 pre-check    *)
(* and availability walk, the ordered pass (S5 in the order K, then the    *)
(* S6 repair loop) and compose. The fast path is the predicate F5 to F13   *)
(* and compose of every chain.                                             *)
(*                                                                         *)
(* Each boolean constant enables one check of the rule. The safe           *)
(* configuration enables all of them. Each Unsafe configuration disables   *)
(* one check and must violate the invariant that the check protects.       *)
(*                                                                         *)
(* Values are scaled: MAX = 1 and MIN = -1 stand for i64::MAX and          *)
(* i64::MIN, and every diff and base value lies in -1..1.                  *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS Scenarios,
          UseKOrder,              \* S5 walks in K, not in arrival order
          UseClaims,              \* S5 checks conflicts with kept candidates
          UseRecheck,             \* S6.2 re-checks conflicts after a shrink
          UseMixingCheck,         \* S5 checks folded/plain mixing (C1)
          NormalizedOverfill,     \* the overfill dry run normalizes (LOW-3)
          RawDuplicateCheck,      \* F5 reads the raw chain list (C2)
          SignSplitPrecheck,      \* S3 bounds sign splits, not net totals (C4)
          UserSystemSplit,        \* S3 bounds user and system parts (LOW-4)
          UsePrecheck,            \* S3 runs the pre-check at all
          AnyContributorFallback, \* S6.4 may drop any contributor (HIGH-1)
          MergeTypeCheck,         \* S5 rejects a second merge type (LOW-4)
          UseRepair,              \* S6.4 repairs the final balances
          UseBasePartition,       \* dev's base partition runs
          StampFastPath,          \* F4 stamps chains with prior losses (MEDIUM-2)
          ChainLevelP5,           \* F10 uses the chain-level map (LOW-5)
          FullLedgerFastPath,     \* F13 uses the full-set ledger (C3)
          PinnedFirst,            \* K puts settled candidates first (#341)
          LossesFirst             \* K puts prior losses next (#294)

Flags == <<UseKOrder, UseClaims, UseRecheck, UseMixingCheck, NormalizedOverfill,
           RawDuplicateCheck, SignSplitPrecheck, UserSystemSplit, UsePrecheck,
           AnyContributorFallback, MergeTypeCheck, UseRepair, UseBasePartition,
           StampFastPath, ChainLevelP5, FullLedgerFastPath, PinnedFirst, LossesFirst>>
ASSUME \A i \in 1..Len(Flags) : Flags[i] \in BOOLEAN

MAX == 1
MIN == -1
NONE == 9

VARIABLES s, derived, slow, fast
vars == <<s, derived, slow, fast>>

(***************************************************************************)
(* Chains. A chain record has these fields:                                *)
(*   id        its rank in the DeployChainIndex order (distinct)          *)
(*   cand      its branch                                                 *)
(*   pinned    settled in the floor                                       *)
(*   loss      prior-loss count; height: source height                    *)
(*   deploy, cost   its user deploy id and cost (chain equality)          *)
(*   u, y      user and system diff on L, or NONE; mt: "add" or "or"      *)
(*   claim     "none", "folded" or "plain" on a claim channel (C1)         *)
(*   vadd, vrm datums it adds to and removes from cell V                  *)
(*   prod, cons     a produce and a consume on a channel P                *)
(*   ancs      ids of chains whose source block is a strict ancestor of   *)
(*             this chain's source block (transitively closed)           *)
(*   baseConf  conflicts with the base's own content                      *)
(***************************************************************************)
HasU(c) == c.u # NONE
HasY(c) == c.y # NONE
HasL(c) == HasU(c) \/ HasY(c)
UVal(c) == IF HasU(c) THEN c.u ELSE 0
YVal(c) == IF HasY(c) THEN c.y ELSE 0
CVal(c) == UVal(c) + YVal(c)
Pos(x) == IF x > 0 THEN x ELSE 0
Neg(x) == IF x < 0 THEN x ELSE 0

Part(c, k) ==
    CASE k = "u+" -> Pos(UVal(c)) [] k = "u-" -> Neg(UVal(c))
      [] k = "y+" -> Pos(YVal(c)) [] k = "y-" -> Neg(YVal(c))
      [] k = "c+" -> Pos(CVal(c)) [] k = "c-" -> Neg(CVal(c))
      [] k = "u" -> UVal(c) [] k = "y" -> YVal(c) [] k = "c" -> CVal(c)
      [] k = "loss" -> c.loss

RECURSIVE Sum(_, _)
Sum(S, k) == IF S = {} THEN 0
             ELSE LET c == CHOOSE x \in S : TRUE IN Part(c, k) + Sum(S \ {c}, k)

Fits(p, n) == p <= MAX /\ n >= MIN

Max(S) == CHOOSE m \in S : \A x \in S : x <= m
Min(S) == CHOOSE m \in S : \A x \in S : m <= x

RECURSIVE SortNat(_)
SortNat(S) == IF S = {} THEN << >> ELSE LET m == Min(S) IN <<m>> \o SortNat(S \ {m})

SortedIds(A) == SortNat({c.id : c \in A})

RECURSIVE LexLess(_, _)
LexLess(a, b) ==
    IF a = << >> THEN b # << >>
    ELSE IF b = << >> THEN FALSE
    ELSE IF Head(a) < Head(b) THEN TRUE
    ELSE IF Head(a) > Head(b) THEN FALSE
    ELSE LexLess(Tail(a), Tail(b))

(* ----- conflicts ----- *)

\* A race on one IO event: a listed race, or two removals of one datum.
Race(a, b) == {a.id, b.id} \in s.races \/ (a.vrm \cap b.vrm # {})

\* dev's conflict test on two chains as units (merging_logic.rs:262-470).
ChainConflict(a, b) ==
    \/ Race(a, b)
    \/ (a.prod /\ ~a.cons /\ b.cons /\ ~b.prod)
    \/ (b.prod /\ ~b.cons /\ a.cons /\ ~a.prod)

\* A branch's own COMMs destroy its produce when it also consumes on P.
NetProd(A) == (\E a \in A : a.prod) /\ ~(\E a \in A : a.cons)
NetCons(A) == (\E a \in A : a.cons) /\ ~(\E a \in A : a.prod)

\* The event-log part of dev's branch conflict map.
EventConflict(A, B) ==
    \/ \E a \in A, b \in B : Race(a, b)
    \/ (NetProd(A) /\ NetCons(B))
    \/ (NetProd(B) /\ NetCons(A))

\* dev's branch conflict map with its same-user-deploy-id pass
\* (dag_merger.rs:1732-1760).
CandConflict(A, B) == EventConflict(A, B) \/ \E a \in A, b \in B : a.deploy = b.deploy

\* C1: folded and plain claims on one channel mix.
CrossMix(A, B) ==
    \E a \in A, b \in B : (a.claim = "folded" /\ b.claim = "plain")
                       \/ (a.claim = "plain" /\ b.claim = "folded")

(* ----- the number channel L ----- *)

AddChains(S) == {c \in S : HasL(c) /\ c.mt = "add"}
TypesAgree(S) == \A a, b \in {c \in S : HasL(c)} : a.mt = b.mt

UserFits(B) == Fits(Sum(AddChains(B), "u+"), Sum(AddChains(B), "u-"))
SystemFits(B) == Fits(Sum(AddChains(B), "y+"), Sum(AddChains(B), "y-"))
CombinedFits(B) == Fits(Sum(AddChains(B), "c+"), Sum(AddChains(B), "c-"))
SplitsFit(B) ==
    /\ UserFits(B) /\ SystemFits(B)
    /\ Fits(Sum(AddChains(B), "u+") + Sum(AddChains(B), "y+"),
            Sum(AddChains(B), "u-") + Sum(AddChains(B), "y-"))
TotalsFit(B) ==
    /\ MIN <= Sum(AddChains(B), "u") /\ Sum(AddChains(B), "u") <= MAX
    /\ MIN <= Sum(AddChains(B), "y") /\ Sum(AddChains(B), "y") <= MAX
    /\ MIN <= Sum(AddChains(B), "c") /\ Sum(AddChains(B), "c") <= MAX

\* S3: ledger::branch_valid and its controls.
Precheck(B) ==
    \/ ~UsePrecheck
    \/ /\ TypesAgree(B)
       /\ IF ~SignSplitPrecheck THEN TotalsFit(B)
          ELSE IF UserSystemSplit THEN SplitsFit(B) ELSE CombinedFits(B)

\* Every fold order of compute_branch_derived (user parts, system parts,
\* their two totals) and of the availability walk (combined parts) stays in
\* range exactly when these sign splits fit.
FoldsFit(B) ==
    /\ UserFits(B) /\ SystemFits(B) /\ CombinedFits(B)
    /\ MIN <= Sum(AddChains(B), "u") + Sum(AddChains(B), "y")
    /\ Sum(AddChains(B), "u") + Sum(AddChains(B), "y") <= MAX
FoldsAbort(B) == ~TypesAgree(B) \/ ~FoldsFit(B)

\* S5: PurseLedger::try_add on the kept chains and a candidate.
LedgerOK(S) ==
    /\ Fits(Sum(AddChains(S), "c+"), Sum(AddChains(S), "c-"))
    /\ MergeTypeCheck => TypesAgree(S)

\* HIGH-1: the final balance of L over its contributors.
Failure(S) ==
    LET A == AddChains(S)
        t == s.baseL + Sum(A, "c")
    IN IF A = {} THEN "none" ELSE IF t < 0 THEN "neg" ELSE IF t > MAX THEN "over" ELSE "none"

PurseOK(S) ==
    LET A == AddChains(S) IN
    A # {} => (0 <= s.baseL + Sum(A, "c") /\ s.baseL + Sum(A, "c") <= MAX)

(* ----- the single-value cell V ----- *)

BaseCell == IF s.baseCell THEN {"b"} ELSE {}

Prio(c, stamped) == IF stamped THEN c.loss ELSE 0

\* dependency_ordered_branch_items (dag_merger.rs:83-120) for chains with no
\* dependency between them: more prior losses first, then the chain order.
RECURSIVE WalkOrder(_, _)
WalkOrder(S, stamped) ==
    IF S = {} THEN << >>
    ELSE LET m == CHOOSE m \in S : \A x \in S \ {m} :
                     \/ Prio(m, stamped) > Prio(x, stamped)
                     \/ (Prio(m, stamped) = Prio(x, stamped) /\ m.id < x.id)
         IN <<m>> \o WalkOrder(S \ {m}, stamped)

\* numeric_cell_would_overfill (dag_merger.rs:62-80); every datum is a number.
WouldOverfill(cur, added) ==
    /\ added # {}
    /\ IF s.baseCell THEN Cardinality(cur) + Cardinality(added) > 1
       ELSE cur # {} /\ Cardinality(cur) + Cardinality(added) > 1

\* dev's availability walk on V (dag_merger.rs:165-350): the rejected chains.
RECURSIVE CellWalk(_, _)
CellWalk(seq, cur) ==
    IF seq = << >> THEN {}
    ELSE LET c == Head(seq) IN
         IF c.vrm \subseteq cur /\ ~WouldOverfill(cur \ c.vrm, c.vadd)
         THEN CellWalk(Tail(seq), (cur \ c.vrm) \cup c.vadd)
         ELSE {c} \cup CellWalk(Tail(seq), cur)

NoChange == [has |-> FALSE, add |-> {}, rm |-> {}]

\* StateChange::combine on V (state_change.rs:455-500, channel_change.rs:17-71):
\* the first change is kept as it is; later changes join and, when [norm],
\* cancel the datums that one adds and another removes.
RECURSIVE CellFold(_, _, _)
CellFold(seq, acc, norm) ==
    IF seq = << >> THEN acc
    ELSE LET c == Head(seq)
             a == acc.add \cup c.vadd
             r == acc.rm \cup c.vrm
             nxt == IF c.vadd = {} /\ c.vrm = {} THEN acc
                    ELSE IF ~acc.has THEN [has |-> TRUE, add |-> c.vadd, rm |-> c.vrm]
                    ELSE IF norm THEN [has |-> TRUE, add |-> a \ r, rm |-> r \ a]
                    ELSE [has |-> TRUE, add |-> a, rm |-> r]
         IN CellFold(Tail(seq), nxt, norm)

\* More than one kept chain adds on V (dag_merger.rs:2007-2023).
Multi(K) == Cardinality({c \in UNION K : c.vadd # {}}) > 1

\* check_single_value_cell_not_overfilled behind guarded_channel_action.
GuardFails(chg, multi) ==
    /\ chg.has /\ chg.add # {}
    /\ s.baseCell \/ multi
    /\ Cardinality(BaseCell \ chg.rm) + Cardinality(chg.add) > 1

\* Compose's canonical order: branches by compare_branches, then chains.
\* Sorts the candidates of a set of <<candidate, key>> pairs by key.
RECURSIVE SortKeyed(_)
SortKeyed(P) ==
    IF P = {} THEN << >>
    ELSE LET m == CHOOSE m \in P : \A x \in P \ {m} : LexLess(m[2], x[2])
         IN <<m[1]>> \o SortKeyed(P \ {m})

SortCB(K) == SortKeyed({<<A, <<Cardinality(A)>> \o SortedIds(A)>> : A \in K})
ChainSeq(A) == LET ids == SortedIds(A) IN [i \in 1..Len(ids) |-> CHOOSE c \in A : c.id = ids[i]]
RECURSIVE Flatten(_)
Flatten(seq) == IF seq = << >> THEN << >> ELSE ChainSeq(Head(seq)) \o Flatten(Tail(seq))
Canon(K) == Flatten(SortCB(K))

\* LOW-3: the dry run (compose.rs first_overfill) and compose's guard.
DryRunOverfill(K) == GuardFails(CellFold(Canon(K), NoChange, NormalizedOverfill), Multi(K))
ComposeOverfill(K) == GuardFails(CellFold(Canon(K), NoChange, TRUE), Multi(K))

(* ----- compose ----- *)

\* compute_merged_state folds L with checked addition in canonical order
\* (conflict_set_merger.rs:547-579); NONE is an overflow.
RECURSIVE CheckedFold(_, _)
CheckedFold(seq, acc) ==
    IF seq = << >> \/ acc = NONE THEN acc
    ELSE LET c == Head(seq) IN
         IF ~HasL(c) THEN CheckedFold(Tail(seq), acc)
         ELSE IF acc + CVal(c) > MAX \/ acc + CVal(c) < MIN THEN NONE
         ELSE CheckedFold(Tail(seq), acc + CVal(c))

\* compose aborts on a merge-type mismatch, an overflowing fold, a balance
\* outside [0, MAX] (rholang_merging_logic.rs:116-136), or an overfill.
ComposeAborts(K) ==
    LET ch == UNION K
        total == CheckedFold(Canon(K), 0)
    IN \/ ~TypesAgree(ch)
       \/ (AddChains(ch) # {} /\ (total = NONE \/ s.baseL + total < 0 \/ s.baseL + total > MAX))
       \/ ComposeOverfill(K)

(* ----- the order K ----- *)

Pinned(A) == \E c \in A : c.pinned
LossMax(A) == Max({c.loss : c \in A})
MinHeight(A) == Min({c.height : c \in A})

KKey(A) ==
    << IF PinnedFirst THEN (IF Pinned(A) THEN 0 ELSE 1) ELSE 0,
       IF LossesFirst THEN -LossMax(A) ELSE 0,
       IF LossesFirst THEN -Sum(A, "loss") ELSE 0,
       MinHeight(A), Cardinality(A) >> \o SortedIds(A)
KLess(A, B) == LexLess(KKey(A), KKey(B))

\* sort_by_k; the keys are computed once per sort.
SortK(K) == SortKeyed({<<A, KKey(A)>> : A \in K})

\* last_by_k: the last index whose candidate is in [P], unpinned first.
LastByK(seq, P) ==
    LET un == {i \in 1..Len(seq) : seq[i] \in P /\ ~Pinned(seq[i])}
        pn == {i \in 1..Len(seq) : seq[i] \in P /\ Pinned(seq[i])}
    IN IF un # {} THEN Max(un) ELSE IF pn # {} THEN Max(pn) ELSE 0

(* ----- the slow path ----- *)

\* A HashSet of chains keeps one chain of each (deploy, cost) class.
Collapse(S) == {c \in S : ~\E d \in S : d.id < c.id /\ d.deploy = c.deploy /\ d.cost = c.cost}

Branches(S) == {{c \in S : c.cand = k} : k \in {c.cand : c \in S}}
WalkRejected(B, stamped) == CellWalk(WalkOrder(B, stamped), BaseCell)

\* The part of the slow path that no arrival order changes, computed once
\* per scenario into [derived]: dev's settled partition (dag_merger.rs:1225-1281),
\* the base partition, and S3 (the pre-check, then the availability walk;
\* the survivors of each branch form one candidate). [pre] holds the ids of
\* the base- and settled-conflicting chains.
Derive ==
    LET all == Collapse(s.chains)
        settled == {c \in all : c.pinned}
        sconf == {c \in all \ settled : \E q \in settled : ChainConflict(c, q)}
        bconf == IF UseBasePartition THEN {c \in all \ (settled \cup sconf) : c.baseConf} ELSE {}
        actual == all \ (sconf \cup bconf)
        brs == Branches(actual)
        walked == {<<B, WalkRejected(B, TRUE)>> : B \in {X \in brs : Precheck(X)}}
    IN [cands |-> {w[1] \ w[2] : w \in walked} \ {{}},
        s3rej |-> UNION {B \in brs : ~Precheck(B)} \cup UNION {w[2] : w \in walked},
        pre |-> {c.id : c \in sconf \cup bconf}]

\* The candidates in one arrival order.
RECURSIVE Arrive(_, _)
Arrive(p, K) ==
    IF p = << >> THEN << >>
    ELSE LET here == {x \in K : \E c \in x : c.cand = Head(p)}
         IN (IF here = {} THEN << >> ELSE <<CHOOSE x \in here : TRUE>>) \o Arrive(Tail(p), K)

\* S5: one walk with the three monotone checks.
RECURSIVE S5(_, _, _)
S5(order, kept, rej) ==
    IF order = << >> THEN [kept |-> kept, rej |-> rej]
    ELSE LET c == Head(order)
             keptChains == UNION {kept[i] : i \in 1..Len(kept)}
             bad == \/ (UseClaims /\ \E i \in 1..Len(kept) : CandConflict(c, kept[i]))
                    \/ (UseMixingCheck /\ \E i \in 1..Len(kept) : CrossMix(c, kept[i]))
                    \/ ~LedgerOK(keptChains \cup c)
         IN IF bad THEN S5(Tail(order), kept, rej \cup c)
            ELSE S5(Tail(order), Append(kept, c), rej)

\* S6: lineage closure, the conflict re-check, the overfill dry run and the
\* final balances, until none of them changes the kept set. [pre] holds the
\* ids of the base- and settled-conflicting chains.
RECURSIVE S6(_, _, _, _, _)
S6(fuel, recheck, kept, rej, pre) ==
    IF fuel = 0 THEN [abort |-> TRUE, kept |-> {}, rej |-> rej]
    ELSE LET R == {c.id : c \in rej} \cup pre
             staleCh == {c \in UNION kept : ~c.pinned /\ c.ancs \cap R # {}}
             kept1 == {k \ staleCh : k \in kept} \ {{}}
             rej1 == rej \cup staleCh
             seq == SortK(kept1)
             pairs == {pr \in (1..Len(seq)) \X (1..Len(seq)) :
                         pr[1] < pr[2] /\ CandConflict(seq[pr[1]], seq[pr[2]])}
             first == CHOOSE pr \in pairs : \A q \in pairs :
                         pr[1] < q[1] \/ (pr[1] = q[1] /\ pr[2] <= q[2])
             adders == {k \in kept1 : \E c \in k : c.vadd # {}}
             f == Failure(UNION kept1)
             negs == {k \in kept1 : \E c \in AddChains(k) : CVal(c) < 0}
             anys == {k \in kept1 : AddChains(k) # {}}
             poss == {k \in kept1 : \E c \in AddChains(k) : CVal(c) > 0}
             bv == IF f = "neg"
                   THEN IF LastByK(seq, negs) # 0 THEN LastByK(seq, negs)
                        ELSE IF AnyContributorFallback THEN LastByK(seq, anys) ELSE 0
                   ELSE LastByK(seq, poss)
         IN IF UseRecheck /\ (recheck \/ staleCh # {}) /\ pairs # {}
            THEN S6(fuel - 1, TRUE, kept1 \ {seq[first[2]]}, rej1 \cup seq[first[2]], pre)
            ELSE IF DryRunOverfill(kept1)
            THEN IF LastByK(seq, adders) = 0 THEN [abort |-> TRUE, kept |-> {}, rej |-> rej1]
                 ELSE S6(fuel - 1, FALSE, kept1 \ {seq[LastByK(seq, adders)]},
                         rej1 \cup seq[LastByK(seq, adders)], pre)
            ELSE IF UseRepair /\ f # "none"
            THEN IF bv = 0 THEN [abort |-> TRUE, kept |-> {}, rej |-> rej1]
                 ELSE S6(fuel - 1, FALSE, kept1 \ {seq[bv]}, rej1 \cup seq[bv], pre)
            ELSE [abort |-> FALSE, kept |-> kept1, rej |-> rej1]

\* The slow result in one arrival order. It stores the chain ids that the
\* merge composes (none when it aborts) and the properties of the set that
\* the repair loop returns, before compose: an invalid set there is also an
\* abort in compose, and the invariants must see the set itself.
SlowResult(p) ==
    LET order == IF UseKOrder THEN SortK(derived.cands) ELSE Arrive(p, derived.cands)
        r5 == S5(order, << >>, {})
        kept5 == {r5.kept[i] : i \in 1..Len(r5.kept)}
        r6 == S6(Cardinality(s.chains) + 1, FALSE, kept5, derived.s3rej \cup r5.rej, derived.pre)
        abort == (\E B \in derived.cands : FoldsAbort(B)) \/ r6.abort \/ ComposeAborts(r6.kept)
        live == IF abort THEN {} ELSE r6.kept
    IN [abort |-> abort,
        kept |-> {c.id : c \in UNION live},
        conflictFree |-> \A A, B \in r6.kept : A # B => ~CandConflict(A, B),
        mixFree |-> \A A, B \in r6.kept : A # B => ~CrossMix(A, B),
        overfillFree |-> ~ComposeOverfill(r6.kept),
        purseOK |-> PurseOK(UNION r6.kept)]

(* ----- the fast path ----- *)


\* F13 with a prefix fold in arrival order (the FastPathPrefix control).
RECURSIVE Prefix(_, _)
Prefix(seq, bal) ==
    IF seq = << >> THEN TRUE
    ELSE LET c == Head(seq) IN
         IF ~HasL(c) THEN Prefix(Tail(seq), bal)
         ELSE IF bal + CVal(c) < 0 \/ bal + CVal(c) > MAX THEN FALSE
         ELSE Prefix(Tail(seq), bal + CVal(c))

FastResult(p) ==
    LET all == s.chains
        brs == Branches(all)
        dup == IF RawDuplicateCheck
               THEN \E a, b \in all : a.id # b.id /\ a.deploy = b.deploy
               ELSE \E a, b \in Collapse(all) : a.id # b.id /\ a.deploy = b.deploy
        p5 == IF ChainLevelP5
              THEN \E a, b \in all : a.id # b.id /\ ChainConflict(a, b)
              ELSE \E A, B \in brs : A # B /\ EventConflict(A, B)
        walk == \E B \in brs : WalkRejected(B, StampFastPath) # {}
        mixing == \E A, B \in brs : A # B /\ CrossMix(A, B)
        ledger == IF FullLedgerFastPath
                  THEN LedgerOK(all) /\ TypesAgree(all) /\ Failure(all) = "none"
                  ELSE TypesAgree(all) /\ Prefix(Flatten(Arrive(p, brs)), s.baseL)
        P == /\ ~dup
             /\ ~\E c \in all : c.baseConf
             /\ \A B \in brs : Precheck(B)
             /\ ~p5 /\ ~walk /\ ~mixing
             /\ ~DryRunOverfill(brs)
             /\ ledger
        composeAborts == ComposeAborts(brs)
    IN [fires |-> P,
        abort |-> P /\ composeAborts,
        kept |-> IF P /\ ~composeAborts THEN {c.id : c \in all} ELSE {}]

(* ----- the specification ----- *)

PermsOf(S) == {f \in [1..Cardinality(S) -> S] : \A i, j \in 1..Cardinality(S) : i # j => f[i] # f[j]}
Arrivals == PermsOf({c.cand : c \in s.chains})

\* With K, the slow path does not read the arrival order, and with the
\* full-set ledger neither does the fast path, so each result is computed
\* once per scenario.
Init == /\ s \in Scenarios
        /\ derived = Derive
        /\ IF UseKOrder
           THEN \E r \in {SlowResult(<< >>)} : slow = [p \in Arrivals |-> r]
           ELSE slow = [p \in Arrivals |-> SlowResult(p)]
        /\ IF FullLedgerFastPath
           THEN \E r \in {FastResult(<< >>)} : fast = [p \in Arrivals |-> r]
           ELSE fast = [p \in Arrivals |-> FastResult(p)]

Next == UNCHANGED vars

Spec == Init /\ [][Next]_vars

(* ----- invariants ----- *)

TypeOK == /\ DOMAIN slow = Arrivals /\ DOMAIN fast = Arrivals

\* Every arrival order gives one result.
Agreement == \A p, q \in DOMAIN slow : slow[p] = slow[q]

NoConflictingSurvivors == \A p \in DOMAIN slow : slow[p].conflictFree

NoFoldedMixing == \A p \in DOMAIN slow : slow[p].mixFree

NoOverfill == \A p \in DOMAIN slow : slow[p].overfillFree

\* The dry run flags exactly the sets on which compose's guard fails.
OverfillMatchesGuard == \A K \in SUBSET derived.cands : DryRunOverfill(K) = ComposeOverfill(K)

\* No fold of a candidate's parts leaves the i64 range, in any order.
ChainFoldNoOverflow == \A B \in derived.cands : FoldsFit(B)

PurseValid == \A p \in DOMAIN slow : slow[p].purseOK

\* A scope with conflicts only: no number, claim, cell, lineage or base
\* content.
PureConflict ==
    \A c \in s.chains :
        /\ ~HasL(c) /\ c.claim = "none" /\ c.vadd = {} /\ c.vrm = {}
        /\ c.ancs = {} /\ ~c.baseConf /\ ~c.prod /\ ~c.cons

\* #341: a settled chain that conflicts with no other settled chain is kept.
SettledKept ==
    PureConflict =>
        \A p \in DOMAIN slow : \A c \in s.chains :
            (c.pinned /\ \A d \in s.chains : (d.pinned /\ d.id # c.id) => ~ChainConflict(c, d))
            => c.id \in slow[p].kept

\* The base wins: no kept chain conflicts with the base's own content.
BaseWins ==
    \A p \in DOMAIN slow : \A c \in s.chains : (c.id \in slow[p].kept /\ c.baseConf) => c.pinned

\* Every deploy id is composed at most once, by either path.
ExactlyOncePerDeployId ==
    \A p \in DOMAIN slow :
        /\ \A a, b \in s.chains :
              (a.id \in slow[p].kept /\ b.id \in slow[p].kept /\ a.id # b.id) => a.deploy # b.deploy
        /\ \A a, b \in s.chains :
              (a.id \in fast[p].kept /\ b.id \in fast[p].kept /\ a.id # b.id) => a.deploy # b.deploy

\* L5: when the fast path fires, it equals the slow path.
FastEqualsSlow ==
    \A p \in DOMAIN fast : fast[p].fires => (fast[p].abort = slow[p].abort /\ fast[p].kept = slow[p].kept)

\* #294: the unpinned chain with the strictly highest loss is kept when it
\* conflicts with no settled chain.
HighestLossKeptQualified ==
    PureConflict =>
        \A p \in DOMAIN slow : \A c \in s.chains :
            ( /\ ~c.pinned
              /\ \A d \in s.chains : (d.id # c.id /\ ~d.pinned) => d.loss < c.loss
              /\ \A d \in s.chains : d.pinned => ~ChainConflict(c, d) )
            => c.id \in slow[p].kept

NoMergeAbort == \A p \in DOMAIN slow : ~slow[p].abort
=============================================================================
