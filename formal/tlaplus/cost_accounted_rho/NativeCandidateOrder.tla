------------------------- MODULE NativeCandidateOrder -------------------------
(***************************************************************************)
(* I1 (epic 8946, B1; DR-75): RSpace candidate order from precomputed      *)
(* source hashes.                                                          *)
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
(* in different insertion orders. Each Consume step chooses, on each node, *)
(* the first candidate in that node's order that satisfies a pattern, and  *)
(* removes it. The implementation order is the two-phase order:            *)
(*   1. sort by (src, index);                                              *)
(*   2. re-sort each run of two or more equal sources by (digest, index).  *)
(*                                                                         *)
(* Rust correspondence:                                                    *)
(*   TwoPhase           rspace++/src/rspace/candidate_order.rs             *)
(*   PlayOrder          ops_produce.rs / ops_consume.rs (play)             *)
(*   ReplayOrder        native_candidate.rs and native_candidate/metered.rs*)
(*   Digested           digests computed only for tie-run members          *)
(*   FilterCommutes     the metered path filters by candidate identity     *)
(*                      before it sorts                                    *)
(* Rocq correspondence: theories/CandidateSourceOrder.v                    *)
(*   (two_phase_is_canonical, candidate_order_insertion_independent,       *)
(*    digests_only_for_ties, filter_commutes_with_canonical_sort).         *)
(*                                                                         *)
(* Mutation selects one negative control. Each control must violate the    *)
(* named invariant in its *Unsafe.cfg:                                     *)
(*   "indexonly"        both nodes order by store index only               *)
(*                      -> InsertionIndependent                            *)
(*   "sourceonly"       both nodes order by (src, index) with no digest    *)
(*                      -> InsertionIndependent                            *)
(*   "replaynotiebreak" replay orders by (src, index), play uses the       *)
(*                      two-phase order -> PlayReplayAgree                 *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS Sources, Extras, MaxCandidates, MaxConsumes, Mutation

ASSUME /\ Sources \subseteq Nat /\ Sources # {}
       /\ Extras \subseteq Nat /\ Extras # {}
       /\ MaxCandidates \in Nat
       /\ MaxConsumes \in Nat
       /\ Mutation \in {"none", "indexonly", "sourceonly", "replaynotiebreak"}

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

PlayOrder(s) ==
    CASE Mutation = "indexonly" -> SortBy(s, KeyIndex)
      [] Mutation = "sourceonly" -> SortBy(s, KeySourceIndex)
      [] OTHER -> TwoPhase(s)

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

VARIABLES playStore, replayStore, choices

vars == <<playStore, replayStore, choices>>

Init ==
    /\ playStore \in Stores
    /\ replayStore \in Arrangements(playStore)
    /\ choices = <<>>

Consume(pattern) ==
    /\ Len(choices) < MaxConsumes
    /\ LET po == PlayOrder(Indexed(playStore))
           ro == ReplayOrder(Indexed(replayStore))
           pk == FirstMatch(po, pattern)
           rk == FirstMatch(ro, pattern)
       IN /\ choices' = Append(choices, <<ChosenPayload(po, pk), ChosenPayload(ro, rk)>>)
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

(* Two stores that hold the same multiset give the same payload order. *)
InsertionIndependent ==
    IsArrangement(playStore, replayStore) =>
        PayloadSeq(PlayOrder(Indexed(playStore))) = PayloadSeq(PlayOrder(Indexed(replayStore)))

(* Play and replay choose the same candidate payload at every consume. *)
PlayReplayAgree == \A k \in 1..Len(choices) : choices[k][1] = choices[k][2]

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
