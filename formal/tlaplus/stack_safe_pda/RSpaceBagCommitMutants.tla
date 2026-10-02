----------------------- MODULE RSpaceBagCommitMutants -----------------------
EXTENDS RSpaceBagCommit

\* These are deliberately wrong alternatives to the atomic accepted-match
\* transition. Each specification also retains the real transition actions,
\* so the negative control changes exactly one publication boundary.

SplitPublication ==
    /\ playStatus = "pending"
    /\ CanCommit
    /\ CommittedBag # initial
    /\ playBag' = CommittedBag
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   playContinuation, playJoin, playStatus, playEvent,
                   replayBag, replayContinuation, replayJoin,
                   replayStatus, replayEvent>>

EarlyEvent ==
    /\ playStatus = "pending"
    /\ playEvent' = TRUE
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   playBag, playContinuation, playJoin, playStatus,
                   replayBag, replayContinuation, replayJoin,
                   replayStatus, replayEvent>>

DropJoin ==
    /\ playStatus = "pending"
    /\ CanCommit
    /\ ~CommittedJoin
    /\ playBag' = CommittedBag
    /\ playContinuation' = CommittedContinuation
    /\ playJoin' = TRUE
    /\ playStatus' = "committed-unlogged"
    /\ playEvent' = FALSE
    /\ UNCHANGED <<initial, selection, arrival,
                   currentContinuation, currentPersistent, otherContinuation,
                   ticketPersistent, ticketFresh, ticketMetadataMatches,
                   replayBag, replayContinuation, replayJoin,
                   replayStatus, replayEvent>>

SplitPublicationSpec == Init /\ [][Next \/ SplitPublication]_vars
EarlyEventSpec == Init /\ [][Next \/ EarlyEvent]_vars
DropJoinSpec == Init /\ [][Next \/ DropJoin]_vars

=============================================================================
