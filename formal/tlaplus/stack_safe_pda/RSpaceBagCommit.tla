--------------------------- MODULE RSpaceBagCommit ---------------------------
EXTENDS Integers, Naturals, Sequences, FiniteSets, TLC

(******************************************************************************
Finite-state model of the `cd6099f50` RSpace accepted-match boundary.

The matcher returns an ordered candidate sequence, but an accepted commit
projects it to a normalized exact-key BagDelta.  Stored nonpersistent data are
removed, persistent stored data become presence witnesses, fresh data are not
removed, and an arriving datum that the COMM did not select is added in the
same delta.  An opaque continuation epoch plus immutable metadata validates
the named continuation before the datum, continuation, and join state are
published together.  A rejected proposal leaves every public component
unchanged.  COMM logging is a later action enabled only by a complete commit.

Play and replay may linearize independently in either scheduler order.  Each
must reach the same accepted or rejected state and no execution contains a
partially published key prefix or a log-before-state transition.
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

Arrivals == [present : BOOLEAN, key : Keys]
Bags == [Keys -> 0..MaxCount]
Statuses == {"pending", "committed-unlogged", "committed", "rejected"}

SelectedFresh(selection) ==
    \E index \in 1..Len(selection) : ~selection[index].stored

Consumes(candidate, key) ==
    candidate.stored /\ ~candidate.persistent /\ candidate.key = key

RemovalCount(selection, key) ==
    Cardinality({index \in 1..Len(selection) : Consumes(selection[index], key)})

AdditionCount(arrival, selection, key) ==
    IF arrival.present /\ ~SelectedFresh(selection) /\ arrival.key = key
    THEN 1
    ELSE 0

NetRemoval(selection, arrival, key) ==
    LET remove == RemovalCount(selection, key)
        add == AdditionCount(arrival, selection, key)
    IN IF remove >= add THEN remove - add ELSE 0

NetAddition(selection, arrival, key) ==
    LET remove == RemovalCount(selection, key)
        add == AdditionCount(arrival, selection, key)
    IN IF add >= remove THEN add - remove ELSE 0

PersistentWitnessesPresent(subject, selection) ==
    \A index \in 1..Len(selection) :
      (selection[index].stored /\ selection[index].persistent)
        => subject[selection[index].key] > 0

DeltaApplicable(subject, selection, arrival) ==
    \A key \in Keys :
      /\ NetRemoval(selection, arrival, key) <= subject[key]
      /\ subject[key]
           - NetRemoval(selection, arrival, key)
           + NetAddition(selection, arrival, key) <= MaxCount

ApplyDelta(subject, selection, arrival) ==
    [key \in Keys |->
      subject[key]
        - NetRemoval(selection, arrival, key)
        + NetAddition(selection, arrival, key)]

VARIABLES initial, selection, arrival,
          currentContinuation, currentPersistent, otherContinuation,
          ticketPersistent, ticketFresh, ticketMetadataMatches,
          playBag, playContinuation, playJoin, playStatus, playEvent,
          replayBag, replayContinuation, replayJoin, replayStatus, replayEvent

vars == <<initial, selection, arrival,
          currentContinuation, currentPersistent, otherContinuation,
          ticketPersistent, ticketFresh, ticketMetadataMatches,
          playBag, playContinuation, playJoin, playStatus, playEvent,
          replayBag, replayContinuation, replayJoin, replayStatus, replayEvent>>

ContinuationValid ==
    /\ currentContinuation
    /\ ticketFresh
    /\ ticketMetadataMatches
    /\ ticketPersistent = currentPersistent

CanCommit ==
    /\ ContinuationValid
    /\ PersistentWitnessesPresent(initial, selection)
    /\ DeltaApplicable(initial, selection, arrival)

CommittedBag == ApplyDelta(initial, selection, arrival)
CommittedContinuation == currentPersistent
CommittedJoin == currentPersistent \/ otherContinuation

Init ==
    /\ initial \in Bags
    /\ selection \in Selections
    /\ arrival \in Arrivals
    /\ currentContinuation \in BOOLEAN
    /\ currentPersistent \in BOOLEAN
    /\ otherContinuation \in BOOLEAN
    /\ ticketPersistent \in BOOLEAN
    /\ ticketFresh \in BOOLEAN
    /\ ticketMetadataMatches \in BOOLEAN
    /\ playBag = initial
    /\ playContinuation = currentContinuation
    /\ playJoin = TRUE
    /\ playStatus = "pending"
    /\ playEvent = FALSE
    /\ replayBag = initial
    /\ replayContinuation = currentContinuation
    /\ replayJoin = TRUE
    /\ replayStatus = "pending"
    /\ replayEvent = FALSE

PlayDecide ==
    /\ playStatus = "pending"
    /\ IF CanCommit
       THEN /\ playBag' = CommittedBag
            /\ playContinuation' = CommittedContinuation
            /\ playJoin' = CommittedJoin
            /\ playStatus' = "committed-unlogged"
       ELSE /\ playBag' = initial
            /\ playContinuation' = currentContinuation
            /\ playJoin' = TRUE
            /\ playStatus' = "rejected"
    /\ playEvent' = FALSE
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   replayBag, replayContinuation, replayJoin,
                   replayStatus, replayEvent>>

PlayLog ==
    /\ playStatus = "committed-unlogged"
    /\ playStatus' = "committed"
    /\ playEvent' = TRUE
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   playBag, playContinuation, playJoin,
                   replayBag, replayContinuation, replayJoin,
                   replayStatus, replayEvent>>

ReplayDecide ==
    /\ replayStatus = "pending"
    /\ IF CanCommit
       THEN /\ replayBag' = CommittedBag
            /\ replayContinuation' = CommittedContinuation
            /\ replayJoin' = CommittedJoin
            /\ replayStatus' = "committed-unlogged"
       ELSE /\ replayBag' = initial
            /\ replayContinuation' = currentContinuation
            /\ replayJoin' = TRUE
            /\ replayStatus' = "rejected"
    /\ replayEvent' = FALSE
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   playBag, playContinuation, playJoin, playStatus, playEvent>>

ReplayLog ==
    /\ replayStatus = "committed-unlogged"
    /\ replayStatus' = "committed"
    /\ replayEvent' = TRUE
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   playBag, playContinuation, playJoin, playStatus, playEvent,
                   replayBag, replayContinuation, replayJoin>>

Done ==
    /\ playStatus \in {"committed", "rejected"}
    /\ replayStatus \in {"committed", "rejected"}

Next == PlayDecide \/ PlayLog \/ ReplayDecide \/ ReplayLog

TypeInvariant ==
    /\ initial \in Bags
    /\ selection \in Selections
    /\ arrival \in Arrivals
    /\ currentContinuation \in BOOLEAN
    /\ currentPersistent \in BOOLEAN
    /\ otherContinuation \in BOOLEAN
    /\ ticketPersistent \in BOOLEAN
    /\ ticketFresh \in BOOLEAN
    /\ ticketMetadataMatches \in BOOLEAN
    /\ playBag \in Bags
    /\ playContinuation \in BOOLEAN
    /\ playJoin \in BOOLEAN
    /\ playStatus \in Statuses
    /\ playEvent \in BOOLEAN
    /\ replayBag \in Bags
    /\ replayContinuation \in BOOLEAN
    /\ replayJoin \in BOOLEAN
    /\ replayStatus \in Statuses
    /\ replayEvent \in BOOLEAN

AtomicPublication(status, bag, continuation, join) ==
    CASE status = "pending" ->
           /\ bag = initial
           /\ continuation = currentContinuation
           /\ join = TRUE
      [] status = "rejected" ->
           /\ ~CanCommit
           /\ bag = initial
           /\ continuation = currentContinuation
           /\ join = TRUE
      [] status \in {"committed-unlogged", "committed"} ->
           /\ CanCommit
           /\ bag = CommittedBag
           /\ continuation = CommittedContinuation
           /\ join = CommittedJoin

PlayPublishesOneCompleteState ==
    AtomicPublication(playStatus, playBag, playContinuation, playJoin)

ReplayPublishesOneCompleteState ==
    AtomicPublication(replayStatus, replayBag, replayContinuation, replayJoin)

RejectedTransitionIsNonMutating ==
    /\ playStatus = "rejected" =>
         /\ playBag = initial
         /\ playContinuation = currentContinuation
         /\ playJoin = TRUE
         /\ ~playEvent
    /\ replayStatus = "rejected" =>
         /\ replayBag = initial
         /\ replayContinuation = currentContinuation
         /\ replayJoin = TRUE
         /\ ~replayEvent

EventPublicationFollowsCommit ==
    /\ playEvent <=> playStatus = "committed"
    /\ replayEvent <=> replayStatus = "committed"

StaleOrForgedTicketNeverCommits ==
    ~ContinuationValid =>
      /\ playStatus \notin {"committed-unlogged", "committed"}
      /\ replayStatus \notin {"committed-unlogged", "committed"}

PersistentWitnessesAreRequired ==
    ~PersistentWitnessesPresent(initial, selection) =>
      /\ playStatus \notin {"committed-unlogged", "committed"}
      /\ replayStatus \notin {"committed-unlogged", "committed"}

CommittedMultiplicityAndArrivalAreExact ==
    /\ playStatus \in {"committed-unlogged", "committed"} =>
         \A key \in Keys :
           playBag[key] = initial[key]
             - NetRemoval(selection, arrival, key)
             + NetAddition(selection, arrival, key)
    /\ replayStatus \in {"committed-unlogged", "committed"} =>
         \A key \in Keys :
           replayBag[key] = initial[key]
             - NetRemoval(selection, arrival, key)
             + NetAddition(selection, arrival, key)

ContinuationAndJoinCommitTogether ==
    /\ playStatus \in {"committed-unlogged", "committed"} =>
         /\ playContinuation = currentPersistent
         /\ playJoin = (currentPersistent \/ otherContinuation)
    /\ replayStatus \in {"committed-unlogged", "committed"} =>
         /\ replayContinuation = currentPersistent
         /\ replayJoin = (currentPersistent \/ otherContinuation)

PlayReplayTraceEquivalent ==
    Done =>
      /\ playStatus = replayStatus
      /\ playBag = replayBag
      /\ playContinuation = replayContinuation
      /\ playJoin = replayJoin
      /\ playEvent = replayEvent

Spec == Init /\ [][Next]_vars

=============================================================================
