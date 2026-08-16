--------------------------- MODULE RSpaceBagCommit ---------------------------
EXTENDS Integers, Naturals, Sequences, FiniteSets, TLC

(******************************************************************************
Finite-state model of the live `cd01ea721` RSpace datum-commit boundary.

An accepted matcher result is a sequence because candidate order remains part
of matching semantics.  Commit projects that sequence to an exact-key removal
multiset: only stored, nonpersistent candidates contribute; two candidates at
one key contribute multiplicity two.  Play and replay may commit in either
scheduler order, but each publishes one complete Bag or the unchanged input.
There is no state representing a partially published key prefix.
******************************************************************************)

CONSTANTS Keys, MaxCount

ASSUME /\ Keys # {}
       /\ MaxCount \in Nat

CandidateUniverse ==
    [key : Keys, stored : BOOLEAN, persistent : BOOLEAN]

Selections ==
    {<<>>}
    \union {<<candidate>> : candidate \in CandidateUniverse}
    \union {<<first, second>>
              : first \in CandidateUniverse,
                second \in CandidateUniverse}

Bags == [Keys -> 0..MaxCount]
Statuses == {"pending", "committed", "rejected"}

Consumes(candidate, key) ==
    candidate.stored /\ ~candidate.persistent /\ candidate.key = key

RemovalCount(selection, key) ==
    Cardinality({index \in 1..Len(selection) : Consumes(selection[index], key)})

Delta(selection) == [key \in Keys |-> RemovalCount(selection, key)]

Applicable(subject, delta) ==
    \A key \in Keys : delta[key] <= subject[key]

Subtract(subject, delta) ==
    [key \in Keys |-> subject[key] - delta[key]]

VARIABLES initial, selection, playBag, replayBag, playStatus, replayStatus
vars == <<initial, selection, playBag, replayBag, playStatus, replayStatus>>

Init ==
    /\ initial \in Bags
    /\ selection \in Selections
    /\ playBag = initial
    /\ replayBag = initial
    /\ playStatus = "pending"
    /\ replayStatus = "pending"

PlayCommit ==
    /\ playStatus = "pending"
    /\ CASE Applicable(initial, Delta(selection)) ->
                 /\ playBag' = Subtract(initial, Delta(selection))
                 /\ playStatus' = "committed"
            [] ~Applicable(initial, Delta(selection)) ->
                 /\ playBag' = initial
                 /\ playStatus' = "rejected"
    /\ UNCHANGED <<initial, selection, replayBag, replayStatus>>

ReplayCommit ==
    /\ replayStatus = "pending"
    /\ CASE Applicable(initial, Delta(selection)) ->
                 /\ replayBag' = Subtract(initial, Delta(selection))
                 /\ replayStatus' = "committed"
            [] ~Applicable(initial, Delta(selection)) ->
                 /\ replayBag' = initial
                 /\ replayStatus' = "rejected"
    /\ UNCHANGED <<initial, selection, playBag, playStatus>>

Done == playStatus # "pending" /\ replayStatus # "pending"

Next ==
    PlayCommit
    \/ ReplayCommit
    \/ /\ Done
       /\ UNCHANGED vars

TypeInvariant ==
    /\ initial \in Bags
    /\ selection \in Selections
    /\ playBag \in Bags
    /\ replayBag \in Bags
    /\ playStatus \in Statuses
    /\ replayStatus \in Statuses

AtomicPublication(status, bag) ==
    CASE status = "pending"   -> bag = initial
      [] status = "rejected"  -> bag = initial
      [] status = "committed" ->
           /\ Applicable(initial, Delta(selection))
           /\ bag = Subtract(initial, Delta(selection))

PlayPublishesNoPartialState == AtomicPublication(playStatus, playBag)
ReplayPublishesNoPartialState == AtomicPublication(replayStatus, replayBag)

RejectedTransitionIsNonMutating ==
    /\ playStatus = "rejected" => playBag = initial
    /\ replayStatus = "rejected" => replayBag = initial

CommittedMultiplicityIsExact ==
    /\ playStatus = "committed" =>
         \A key \in Keys : initial[key] = playBag[key] + Delta(selection)[key]
    /\ replayStatus = "committed" =>
         \A key \in Keys : initial[key] = replayBag[key] + Delta(selection)[key]

PlayReplayTraceEquivalent ==
    Done => /\ playStatus = replayStatus
            /\ playBag = replayBag

PersistentAndFreshAreExcluded ==
    \A key \in Keys :
      Delta(selection)[key] =
        Cardinality({index \in 1..Len(selection) :
          selection[index].stored
            /\ ~selection[index].persistent
            /\ selection[index].key = key})

Spec == Init /\ [][Next]_vars

=============================================================================
