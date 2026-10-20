----------------------- MODULE RestingCommCommit -----------------------
\* Changed by the merge of feature/cost-accounted-rho (DR-131, plan step 1.8):
\* DR-131 removes host work, so a resting COMM has no host-work credit to
\* refuse. The model drops the variable credit, the refusal HostWorkRejected
\* and the action HostRefusal. Each old line stays below as a comment. The
\* per-COMM charge (playCharge, replayCharge) stays, because COMMs are the
\* metered compute.
EXTENDS Naturals, Sequences

CONSTANT CompareOrdered
ASSUME CompareOrdered \in BOOLEAN

Epochs == {"Legacy", "CostV1", "CostV2"}
CandidateOrders == {<<"p", "q">>, <<"q", "p">>}
Guards == {"Accept", "Deny", "Unknown"}
Observers == {"Accept", "Reject"}
Refusals == {"EpochRefused", "Stale", "GuardDenied",
\*           "GuardUndecidable", "ObserverDenied", "HostWorkRejected",
             "GuardUndecidable", "ObserverDenied",
             "InstalledLinearRefused"}

VARIABLES epoch, candidate, candidateWaiter, candidateIndex,
\*        guard, observer, credit, firstLinear, secondLinear,
          guard, observer, firstLinear, secondLinear,
          waiterPersistent, installedWaiter, playData, replayData, playWaiter,
          replayWaiter, playLog, replayLog, playCharge, replayCharge,
          playCounter, replayCounter, phase, result
vars == <<epoch, candidate, candidateWaiter, candidateIndex,
\*        guard, observer, credit, firstLinear, secondLinear,
          guard, observer, firstLinear, secondLinear,
          waiterPersistent, installedWaiter, playData, replayData, playWaiter,
          replayWaiter, playLog, replayLog, playCharge, replayCharge,
          playCounter, replayCounter, phase, result>>

InitialData == <<"p", "q">>
OrderedSources(s) == {s[i] : i \in 1..Len(s)}
CandidateMatches ==
  /\ candidateWaiter = playWaiter
  /\ candidateIndex = 1
  /\ IF CompareOrdered
        THEN candidate = playData
        ELSE OrderedSources(candidate) = OrderedSources(playData)

RemainingData ==
  IF firstLinear
    THEN IF secondLinear THEN <<>> ELSE <<"q">>
    ELSE IF secondLinear THEN <<"p">> ELSE InitialData
RemainingWaiter == IF waiterPersistent THEN "w" ELSE "none"
RestingEvent == [kind |-> "RestingCommV2", waiter |-> "w",
                 ordered |-> candidate]

Init ==
  /\ epoch \in Epochs
  /\ candidate \in CandidateOrders
  /\ candidateWaiter \in {"w", "stale-w"}
  /\ candidateIndex \in {1, 2}
  /\ guard \in Guards
  /\ observer \in Observers
\*  /\ credit \in 0..1
  /\ firstLinear \in BOOLEAN
  /\ secondLinear \in BOOLEAN
  /\ waiterPersistent \in BOOLEAN
  /\ installedWaiter \in BOOLEAN
  /\ playData = InitialData
  /\ replayData = InitialData
  /\ playWaiter = "w"
  /\ replayWaiter = "w"
  /\ playLog = <<>>
  /\ replayLog = <<>>
  /\ playCharge = 0
  /\ replayCharge = 0
  /\ playCounter = 0
  /\ replayCounter = 0
  /\ phase = "Ready"
  /\ result = "Pending"

Refuse(reason) ==
  /\ phase = "Ready"
  /\ phase' = "Done"
  /\ result' = reason
  /\ UNCHANGED <<epoch, candidate, candidateWaiter, candidateIndex,
\*     guard, observer, credit, firstLinear, secondLinear,
       guard, observer, firstLinear, secondLinear,
       waiterPersistent, installedWaiter, playData, replayData, playWaiter,
       replayWaiter, playLog, replayLog, playCharge, replayCharge,
       playCounter, replayCounter>>

EpochRefusal == /\ epoch # "CostV2" /\ Refuse("EpochRefused")
StaleRefusal ==
  /\ epoch = "CostV2"
  /\ ~CandidateMatches
  /\ Refuse("Stale")
GuardRefusal ==
  /\ epoch = "CostV2"
  /\ CandidateMatches
  /\ guard = "Deny"
  /\ Refuse("GuardDenied")
GuardUnknown ==
  /\ epoch = "CostV2"
  /\ CandidateMatches
  /\ guard = "Unknown"
  /\ Refuse("GuardUndecidable")
ObserverRefusal ==
  /\ epoch = "CostV2"
  /\ CandidateMatches
  /\ guard = "Accept"
  /\ observer = "Reject"
  /\ Refuse("ObserverDenied")
InstalledLinearRefusal ==
  /\ epoch = "CostV2"
  /\ CandidateMatches
  /\ installedWaiter
  /\ ~waiterPersistent
  /\ Refuse("InstalledLinearRefused")
\* HostRefusal ==
\*   /\ epoch = "CostV2"
\*   /\ CandidateMatches
\*   /\ guard = "Accept"
\*   /\ observer = "Accept"
\*   /\ ~(installedWaiter /\ ~waiterPersistent)
\*   /\ credit = 0
\*   /\ Refuse("HostWorkRejected")

PlayCommit ==
  /\ phase = "Ready"
  /\ epoch = "CostV2"
  /\ CandidateMatches
  /\ guard = "Accept"
  /\ observer = "Accept"
  /\ ~(installedWaiter /\ ~waiterPersistent)
\*  /\ credit = 1
  /\ playData' = RemainingData
  /\ playWaiter' = RemainingWaiter
  /\ playLog' = Append(playLog, RestingEvent)
  /\ playCharge' = playCharge + 1
  /\ phase' = "PlayCommitted"
  /\ result' = "Committed"
  /\ UNCHANGED <<epoch, candidate, candidateWaiter, candidateIndex,
\*     guard, observer, credit, firstLinear, secondLinear,
       guard, observer, firstLinear, secondLinear,
       waiterPersistent, installedWaiter, replayData, replayWaiter, replayLog,
       replayCharge, playCounter, replayCounter>>

ReplayCommit ==
  /\ phase = "PlayCommitted"
  /\ replayData = InitialData
  /\ replayWaiter = "w"
  /\ Len(playLog) = 1
  /\ Head(playLog) = RestingEvent
  /\ Head(playLog).ordered = replayData
  /\ replayData' = RemainingData
  /\ replayWaiter' = RemainingWaiter
  /\ replayLog' = Append(replayLog, Head(playLog))
  /\ replayCharge' = replayCharge + 1
  /\ phase' = "Done"
  /\ result' = "Replayed"
  /\ UNCHANGED <<epoch, candidate, candidateWaiter, candidateIndex,
\*     guard, observer, credit, firstLinear, secondLinear,
       guard, observer, firstLinear, secondLinear,
       waiterPersistent, installedWaiter, playData, playWaiter, playLog, playCharge,
       playCounter, replayCounter>>

Next == EpochRefusal \/ StaleRefusal \/ GuardRefusal \/ GuardUnknown
\*   \/ ObserverRefusal \/ InstalledLinearRefusal \/ HostRefusal
     \/ ObserverRefusal \/ InstalledLinearRefusal
     \/ PlayCommit \/ ReplayCommit
Spec == Init /\ [][Next]_vars

NoMutationOnRefusal ==
  (phase = "Done" /\ result \in Refusals) =>
    /\ playData = InitialData
    /\ replayData = InitialData
    /\ playWaiter = "w"
    /\ replayWaiter = "w"
    /\ playLog = <<>>
    /\ replayLog = <<>>
    /\ playCounter = 0
    /\ replayCounter = 0
OrderedAcceptance ==
  (phase = "PlayCommitted" \/ result = "Replayed") =>
    /\ candidate = InitialData
    /\ candidateWaiter = "w"
    /\ candidateIndex = 1
DistinctRestingIdentity ==
  Len(playLog) > 0 =>
    /\ Head(playLog).kind = "RestingCommV2"
    /\ Head(playLog).kind # "Comm"
    /\ Head(playLog).ordered = InitialData
ReplayAgreement ==
  result = "Replayed" =>
    /\ playData = replayData
    /\ playWaiter = replayWaiter
    /\ playLog = replayLog
    /\ playCharge = replayCharge
NoProduceCounterChange == playCounter = 0 /\ replayCounter = 0
OldEpochsCannotCommit ==
  epoch \in {"Legacy", "CostV1"} => playLog = <<>>
InstalledLinearNeverCommits ==
  (phase = "PlayCommitted" \/ result = "Replayed") =>
    ~(installedWaiter /\ ~waiterPersistent)
=============================================================================
