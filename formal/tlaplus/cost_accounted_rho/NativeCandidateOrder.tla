------------------------- MODULE NativeCandidateOrder -------------------------
(***************************************************************************)
(* I1 (epic 8946, B1; DR-75): RSpace candidate order from precomputed      *)
(* source hashes. DR-129 (2026-10-13): play picks at random again, as on   *)
(* dev, and replay follows the recorded pick.                              *)
(*                                                                         *)
(* A candidate payload is [src, extra]. src is the precomputed source hash *)
(* (Produce.hash or Consume.hash). extra stands for the fields that the    *)
(* source hash omits (peeks and pattern order for a continuation, the      *)
(* non-determinism metadata for a datum). The full canonical digest is a   *)
(* pure function of the payload. Only the order of digests among           *)
(* candidates with an equal source matters, so the model uses              *)
(* Digest(c) = c.extra.                                                    *)
(*                                                                         *)
(* The play node and the replay node hold the same multiset of candidates  *)
(* in different insertion orders. Each Consume step lets play pick any     *)
(* candidate that satisfies the pattern (dev's random shuffle) and records *)
(* the payload of the pick. Replay then retires the first candidate, in    *)
(* its canonical order, that satisfies the pattern and equals the recorded *)
(* payload. When play finds no candidate, the record holds no payload, and *)
(* replay's greedy pass must find none either (the Stored check). The      *)
(* canonical order is the two-phase order:                                 *)
(*   1. sort by (src, index);                                              *)
(*   2. re-sort each run of two or more equal sources by (digest, index).  *)
(*                                                                         *)
(* Superseded by DR-129 (restored dev's randomized candidate order, user   *)
(* decision 2026-10-12): the model of DR-75 had both nodes pick "the first *)
(* candidate in that node's order", so play and replay agreed only through *)
(* one shared order, and replay never followed a record. That text and its *)
(* definitions stay below as comments.                                     *)
(*                                                                         *)
(* Rust correspondence:                                                    *)
(*   TwoPhase           rspace++/src/rspace/candidate_order.rs             *)
(*   PlayPicks          ops_produce.rs `shuffle_with_index` (play, dev's   *)
(*                      random shuffle)                                    *)
(*   ReplayOrder        native_candidate.rs and native_candidate/metered.rs*)
(*   Follows            the recorded identity of native replay             *)
(*   Digested           digests computed only for tie-run members          *)
(*   FilterCommutes     the metered path filters by candidate identity     *)
(*                      before it sorts                                    *)
(* Rocq correspondence: theories/CandidateSourceOrder.v                    *)
(*   (two_phase_is_canonical, candidate_order_insertion_independent,       *)
(*    digests_only_for_ties, filter_commutes_with_canonical_sort).         *)
(*                                                                         *)
(* Scope. The model covers one pattern for each step. A join that names    *)
(* one channel twice (E1 of DR-129) and the Stored check of a continuation *)
(* whose commit can veto (E3-a of DR-129) are covered by the Rust tests    *)
(* that DR-129 lists. The Rust identity of a datum compares the source     *)
(* hash and ignores the metadata in extra (E4 of DR-129). Two data with    *)
(* one source hash have equal pars and random state, so they cannot occur  *)
(* in a Rholang run; the "replaysourcematch" control shows what a src-only *)
(* identity would lose if they did.                                        *)
(*                                                                         *)
(* Mutation selects one negative control. Each control must violate the    *)
(* named invariant in its *Unsafe.cfg:                                     *)
(*   "indexonly"        the canonical order is the store index only        *)
(*                      -> InsertionIndependent                            *)
(*   "sourceonly"       the canonical order is (src, index) with no digest *)
(*                      -> InsertionIndependent                            *)
(*   "replaysourcematch" replay follows the recorded src only, not the     *)
(*                      whole recorded payload -> PlayReplayAgree          *)
(* Superseded by DR-129: the control below no longer violates anything,    *)
(* because replay follows the recorded payload in every order. Its cfg     *)
(* stays with a superseded header and is not registered in the gate.       *)
(*   "replaynotiebreak" replay orders by (src, index), play uses the       *)
(*                      two-phase order -> PlayReplayAgree                 *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS Sources, Extras, MaxCandidates, MaxConsumes, Mutation

\* Changed by DR-129: the new control "replaysourcematch".
\* ASSUME /\ Sources \subseteq Nat /\ Sources # {}
\*        /\ Extras \subseteq Nat /\ Extras # {}
\*        /\ MaxCandidates \in Nat
\*        /\ MaxConsumes \in Nat
\*        /\ Mutation \in {"none", "indexonly", "sourceonly", "replaynotiebreak"}
ASSUME /\ Sources \subseteq Nat /\ Sources # {}
       /\ Extras \subseteq Nat /\ Extras # {}
       /\ MaxCandidates \in Nat
       /\ MaxConsumes \in Nat
       /\ Mutation \in {"none", "indexonly", "sourceonly", "replaynotiebreak",
                        "replaysourcematch"}

Payloads == [src : Sources, extra : Extras]
None == [src |-> -1, extra |-> -1]

Stores == UNION {[1..n -> Payloads] : n \in 0..MaxCandidates}

Perms(n) == {f \in [1..n -> 1..n] : \A i, j \in 1..n : i # j => f[i] # f[j]}

Permute(s, f) == [i \in 1..Len(s) |-> s[f[i]]]

Arrangements(s) == {Permute(s, f) : f \in Perms(Len(s))}

(* A candidate record carries its payload and its index in the store read. *)
Indexed(s) == [i \in 1..Len(s) |-> [src |-> s[i].src, extra |-> s[i].extra, idx |-> i]]

Payload(c) == [src |-> c.src, extra |-> c.extra]

PayloadSeq(o) == [k \in 1..Len(o) |-> Payload(o[k])]

Digest(c) == c.extra

KeyCanonical(c) == <<c.src, Digest(c), c.idx>>
KeySourceIndex(c) == <<c.src, c.idx>>
KeyDigestIndex(c) == <<Digest(c), c.idx>>
KeyIndex(c) == <<c.idx>>

LexLess(a, b) ==
    \E i \in 1..Len(a) : /\ a[i] < b[i]
                         /\ \A j \in 1..(i - 1) : a[j] = b[j]

(* Every key ends with the store index, so keys are distinct and the rank   *)
(* of each element is its position in the sorted sequence.                 *)
SortBy(s, K(_)) ==
    LET n == Len(s)
        Rank(i) == Cardinality({j \in 1..n : LexLess(K(s[j]), K(s[i]))})
    IN [k \in 1..n |-> s[CHOOSE i \in 1..n : Rank(i) = k - 1]]

SameSource(p, a, b) == \A j \in a..b : p[j].src = p[a].src

RunStart(p, k) ==
    CHOOSE a \in 1..k : /\ SameSource(p, a, k)
                        /\ (a = 1 \/ p[a - 1].src # p[k].src)

RunEnd(p, k) ==
    CHOOSE b \in k..Len(p) : /\ SameSource(p, k, b)
                             /\ (b = Len(p) \/ p[b + 1].src # p[k].src)

(* Phase two leaves a singleton run unchanged and computes no digest for it. *)
PhaseTwo(p) ==
    [k \in 1..Len(p) |->
        LET a == RunStart(p, k)
            b == RunEnd(p, k)
        IN IF a = b THEN p[k]
           ELSE SortBy(SubSeq(p, a, b), KeyDigestIndex)[k - a + 1]]

PhaseOne(s) == SortBy(s, KeySourceIndex)

TwoPhase(s) == PhaseTwo(PhaseOne(s))

Canonical(s) == SortBy(s, KeyCanonical)

(* The store indices of the candidates whose digest phase two computes. *)
Digested(s) ==
    LET p == PhaseOne(s)
    IN {p[k].idx : k \in {k \in 1..Len(p) : RunStart(p, k) # RunEnd(p, k)}}

\* Superseded by DR-129: play no longer uses an order. It picks any candidate
\* that satisfies the pattern (PlayPicks below).
\* PlayOrder(s) ==
\*     CASE Mutation = "indexonly" -> SortBy(s, KeyIndex)
\*       [] Mutation = "sourceonly" -> SortBy(s, KeySourceIndex)
\*       [] OTHER -> TwoPhase(s)

(* Since DR-129 the canonical order serves replay only, so the order        *)
(* controls "indexonly" and "sourceonly" mutate replay's order.            *)
ReplayOrder(s) ==
    CASE Mutation = "indexonly" -> SortBy(s, KeyIndex)
      [] Mutation \in {"sourceonly", "replaynotiebreak"} -> SortBy(s, KeySourceIndex)
      [] OTHER -> TwoPhase(s)

Matches(o, pattern) == {k \in 1..Len(o) : Payload(o[k]) \in pattern}

FirstMatch(o, pattern) ==
    IF Matches(o, pattern) = {} THEN 0
    ELSE CHOOSE k \in Matches(o, pattern) : \A j \in Matches(o, pattern) : k <= j

ChosenPayload(o, k) == IF k = 0 THEN None ELSE Payload(o[k])

RemoveAt(s, i) == [j \in 1..(Len(s) - 1) |-> IF j < i THEN s[j] ELSE s[j + 1]]

RemoveChosen(s, o, k) == IF k = 0 THEN s ELSE RemoveAt(s, o[k].idx)

IsArrangement(s, t) == t \in Arrangements(s)

(* DR-129: play picks any candidate that satisfies the pattern, or nothing *)
(* (0) when none does. The picks are positions in Indexed(s).              *)
PlayPicks(s, pattern) ==
    LET o == Indexed(s)
    IN IF Matches(o, pattern) = {} THEN {0} ELSE Matches(o, pattern)

(* DR-129: whether replay candidate c follows the recorded payload. The    *)
(* "replaysourcematch" control compares the source hash only.               *)
Follows(c, recorded) ==
    IF Mutation = "replaysourcematch" THEN c.src = recorded.src
    ELSE Payload(c) = recorded

(* DR-129: replay retires the first candidate, in its canonical order,     *)
(* that satisfies the pattern and follows the record. With no recorded     *)
(* payload it runs the greedy pass of the Stored check.                    *)
ReplayPick(o, pattern, recorded) ==
    IF recorded = None THEN FirstMatch(o, pattern)
    ELSE LET ks == {k \in Matches(o, pattern) : Follows(o[k], recorded)}
         IN IF ks = {} THEN 0 ELSE CHOOSE k \in ks : \A j \in ks : k <= j

VARIABLES playStore, replayStore, choices

vars == <<playStore, replayStore, choices>>

Init ==
    /\ playStore \in Stores
    /\ replayStore \in Arrangements(playStore)
    /\ choices = <<>>

\* Superseded by DR-129: both nodes picked the first candidate in their order.
\* Consume(pattern) ==
\*     /\ Len(choices) < MaxConsumes
\*     /\ LET po == PlayOrder(Indexed(playStore))
\*            ro == ReplayOrder(Indexed(replayStore))
\*            pk == FirstMatch(po, pattern)
\*            rk == FirstMatch(ro, pattern)
\*        IN /\ choices' = Append(choices, <<ChosenPayload(po, pk), ChosenPayload(ro, rk)>>)
\*           /\ playStore' = RemoveChosen(playStore, po, pk)
\*           /\ replayStore' = RemoveChosen(replayStore, ro, rk)
Consume(pattern) ==
    /\ Len(choices) < MaxConsumes
    /\ LET po == Indexed(playStore)
           ro == ReplayOrder(Indexed(replayStore))
       IN \E pk \in PlayPicks(playStore, pattern) :
            LET recorded == ChosenPayload(po, pk)
                rk == ReplayPick(ro, pattern, recorded)
            IN /\ choices' = Append(choices, <<recorded, ChosenPayload(ro, rk)>>)
               /\ playStore' = RemoveChosen(playStore, po, pk)
               /\ replayStore' = RemoveChosen(replayStore, ro, rk)

Next == \E pattern \in SUBSET Payloads : Consume(pattern)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ playStore \in Stores
    /\ replayStore \in Stores
    /\ choices \in Seq((Payloads \cup {None}) \X (Payloads \cup {None}))
    /\ Len(choices) <= MaxConsumes

(* The two-phase order is the canonical (src, digest, index) order. *)
TwoPhaseIsCanonical ==
    /\ TwoPhase(Indexed(playStore)) = Canonical(Indexed(playStore))
    /\ TwoPhase(Indexed(replayStore)) = Canonical(Indexed(replayStore))

\* Changed by DR-129: the canonical order is replay's order, so insertion
\* independence is stated for ReplayOrder.
\* InsertionIndependent ==
\*     IsArrangement(playStore, replayStore) =>
\*         PayloadSeq(PlayOrder(Indexed(playStore))) = PayloadSeq(PlayOrder(Indexed(replayStore)))
(* Two stores that hold the same multiset give the same payload order. *)
InsertionIndependent ==
    IsArrangement(playStore, replayStore) =>
        PayloadSeq(ReplayOrder(Indexed(playStore))) = PayloadSeq(ReplayOrder(Indexed(replayStore)))

\* Changed by DR-129: agreement now means that replay retires the payload that
\* play recorded, and that the two stores keep one multiset.
\* PlayReplayAgree == \A k \in 1..Len(choices) : choices[k][1] = choices[k][2]
(* Replay retires the payload that play recorded at every consume, so the  *)
(* two stores keep holding the same multiset.                              *)
PlayReplayAgree ==
    /\ \A k \in 1..Len(choices) : choices[k][1] = choices[k][2]
    /\ IsArrangement(playStore, replayStore)

(* A digest is computed only for a candidate whose source occurs twice. *)
DigestsOnlyForTies ==
    \A i \in Digested(Indexed(playStore)) :
        Cardinality({j \in 1..Len(playStore) : playStore[j].src = playStore[i].src}) >= 2

(* Filtering before the sort selects the same candidates in the same order *)
(* as filtering after it. Survivors keep their original store indices.    *)
FilterCommutes ==
    \A f \in SUBSET Payloads :
        LET Keep(c) == Payload(c) \in f
        IN SelectSeq(TwoPhase(Indexed(playStore)), Keep)
               = TwoPhase(SelectSeq(Indexed(playStore), Keep))
=============================================================================
