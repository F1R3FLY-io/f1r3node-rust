------------- MODULE RSpaceReceiveAcceleratorCoherenceMutants -------------
EXTENDS RSpaceReceiveAcceleratorCoherence

(***************************************************************************
Negative controls RAC-TMUT-01 to RAC-TMUT-10 for the receive accelerator
lifecycle.  Each specification keeps every real action and adds exactly one
deliberately wrong alternative, so each control moves exactly one boundary.
Each mutant configuration checks TypeInvariant and its one target
invariant; the comment above each control names the target and the
shortest trace that violates it.  In those traces "seed record" is
SeedRecord and "other record" is the second element of Records.
***************************************************************************)

(***************************************************************************
RAC-TMUT-01 SplitPublication; target PublishedArtifactsShareOneRevision.
A build publishes its catalog and occurrence artifacts at its base revision
but leaves the automaton artifact at the revision of the accelerator it
replaces.  Shortest violation: BeginBuild, then SplitPublication, which
publishes an automaton revision of <<0, 0, 0>> beside <<1, 1, 1>>.
***************************************************************************)
SplitPublication ==
    /\ Tick
    /\ staging.active
    /\ StagingIsCurrent
    /\ acc' = [PublishedFrom(staging) EXCEPT !.autRev = acc.autRev]
    /\ staging' = NoStaging
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, snap, ticket, commits>>

(***************************************************************************
RAC-TMUT-02 PartialRebuild; target CoherentAcceleratorEqualsDerivation.
The maintained update of an insert re-indexes only the channel at the first
position of the group.  Shortest violation: Reset("seed") adopts the
persisted accelerator, then PartialRebuild inserts the other record into
"aba"; the accelerator stays coherent but lacks the channel-"b" occurrence
of the new target.
***************************************************************************)
PartialRebuild ==
    \E g \in Groups, r \in Records :
      LET partial == {o \in TargetOcc(RestTarget(g, r)) :
                        o.chan = Members(g)[1]}
      IN  /\ Tick
          /\ r \notin catalog[g]
          /\ GroupMutation(g, [catalog EXCEPT ![g] = @ \union {r}],
                           installed, AddJoins(joins, g), {}, partial,
                           FALSE, loaded \union {g})
          /\ ClearObservations
          /\ UNCHANGED <<staging, snap, ticket, commits>>

(***************************************************************************
RAC-TMUT-03 UnsoundAnswer; target AnswerIsSound.
READ returns the derived relation united with every entry of the held
snapshot.  Shortest violation: Reset("seed"), AcquireSnapshot, then
UnsoundAnswer on channel "a", whose result also holds the channel-"b"
entries of the snapshot.
***************************************************************************)
UnsoundAnswer ==
    \E c \in Channels :
      /\ Tick
      /\ LET k == Class(snap)
             expected == Occ(c)
         IN  answer' = [chan |-> c,
                        src |-> IF k = "coherent" THEN "acc" ELSE "derive",
                        cls |-> k,
                        result |-> expected \union snap.entries,
                        expected |-> expected]
      /\ lastFault' = {}
      /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

(***************************************************************************
RAC-TMUT-04 AbsentAsEmpty; target NoEnabledReceiveHidden.
READ with an absent accelerator answers the empty set instead of deriving.
Shortest violation: AbsentAsEmpty on channel "a" in the initial state,
where group "aba" has an installed and a resting target.
***************************************************************************)
AbsentAsEmpty ==
    \E c \in Channels :
      /\ Tick
      /\ Class(snap) = "absent"
      /\ answer' = [chan |-> c, src |-> "acc", cls |-> "absent",
                    result |-> {}, expected |-> Occ(c)]
      /\ lastFault' = {}
      /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

(***************************************************************************
RAC-TMUT-05 StaleAnswer; target StaleAcceleratorNeverAnswers.
READ answers from a held snapshot whose store or installed-version token is
behind the current one.  Shortest violation: Reset("seed"),
AcquireSnapshot, an Insert into "b", then StaleAnswer.
***************************************************************************)
StaleAnswer ==
    \E c \in Channels :
      /\ Tick
      /\ Class(snap) = "stale"
      /\ answer' = [chan |-> c, src |-> "acc", cls |-> "stale",
                    result |-> {o \in snap.entries : o.chan = c},
                    expected |-> Occ(c)]
      /\ lastFault' = {}
      /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

(***************************************************************************
RAC-TMUT-06 IncompleteAnswer; target IncompleteAcceleratorNeverAnswers.
READ answers from a current snapshot whose scope misses a group that has
targets.  Shortest violation: BeginBuild, PublishBuild with an empty scope
(no group is hydrated yet), AcquireSnapshot, then IncompleteAnswer.
***************************************************************************)
IncompleteAnswer ==
    \E c \in Channels :
      /\ Tick
      /\ Class(snap) = "incomplete"
      /\ answer' = [chan |-> c, src |-> "acc", cls |-> "incomplete",
                    result |-> {o \in snap.entries : o.chan = c},
                    expected |-> Occ(c)]
      /\ lastFault' = {}
      /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

(***************************************************************************
RAC-TMUT-07 ForeignAdoption; target ForeignAcceleratorNeverAdopted.
A reset adopts the persisted accelerator although its manifest differs from
the reset target.  Shortest violation: ForeignAdoption to "empty" in the
initial state binds the "seed" accelerator to the new generation.
***************************************************************************)
ForeignAdoption ==
    \E t \in ResetTargets :
      /\ PersistedManifest # t
      /\ ResetTo(t, TRUE)

(***************************************************************************
RAC-TMUT-08 StaleCandidateCommit; target StaleCandidateNeverCommits.
A resting candidate commits without comparing its ticket with the epoch of
its group.  Shortest violation: TakeCandidate of the seed record's resting
target, an Insert of the other record into "aba", then
StaleCandidateCommit with ticket 1 against epoch 2.
***************************************************************************)
StaleCandidateCommit ==
    /\ Tick
    /\ \E k \in ticket :
         /\ k.epoch # gepoch[k.group]
         /\ k.target.kind = "rest"
         /\ ConsumeResting(k.group, k.target.rec)
         /\ commits' = commits \union {CommitRecord(k)}
         /\ ticket' = {}
         /\ ClearObservations
         /\ UNCHANGED <<staging, snap>>

(***************************************************************************
RAC-TMUT-09 FailedUpdateLeak; target FailedUpdateIsNonMutating.
A failed insert leaks its record and joins into the semantic state while
the revision, the epochs and the accelerator stay as they were.  Shortest
violation: FailedUpdateLeak in the initial state.
***************************************************************************)
FailedUpdateLeak ==
    \E g \in Groups, r \in Records :
      LET newCatalog == [catalog EXCEPT ![g] = @ \union {r}]
          newJoins == AddJoins(joins, g)
      IN  /\ Tick
          /\ r \notin catalog[g]
          /\ catalog' = newCatalog
          /\ joins' = newJoins
          /\ semid' = SemanticIdentity(newCatalog, newJoins)
          /\ lastFault' = {FaultView}
          /\ answer' = NoAnswer
          /\ UNCHANGED <<installed, loaded, gen, store, instv, issued,
                         gepoch, lineage, acc, staging, snap, ticket,
                         commits>>

(***************************************************************************
RAC-TMUT-10 SemanticPollution; target SemanticIdentityIsAcceleratorFree.
An insert computes the semantic identity by walking the accelerator's scope
instead of the semantic roots, so equal roots under different accelerators
get different identities.  Shortest violation: SemanticPollution in the
initial state, where no accelerator exists and the scope is empty.
***************************************************************************)
SemanticPollution ==
    \E g \in Groups, r \in Records :
      LET newCatalog == [catalog EXCEPT ![g] = @ \union {r}]
          newJoins == AddJoins(joins, g)
          viaScope == [h \in Groups |-> IF h \in acc.scope
                                        THEN newCatalog[h]
                                        ELSE {}]
      IN  /\ Tick
          /\ r \notin catalog[g]
          /\ GroupStateChange(g, newCatalog, installed, newJoins, {},
                              TargetOcc(RestTarget(g, r)), FALSE,
                              loaded \union {g})
          /\ semid' = SemanticIdentity(viaScope, newJoins)
          /\ ClearObservations
          /\ UNCHANGED <<staging, snap, ticket, commits>>

SplitPublicationSpec == Init /\ [][Next \/ SplitPublication]_vars
PartialRebuildSpec == Init /\ [][Next \/ PartialRebuild]_vars
UnsoundAnswerSpec == Init /\ [][Next \/ UnsoundAnswer]_vars
AbsentAsEmptySpec == Init /\ [][Next \/ AbsentAsEmpty]_vars
StaleAnswerSpec == Init /\ [][Next \/ StaleAnswer]_vars
IncompleteAnswerSpec == Init /\ [][Next \/ IncompleteAnswer]_vars
ForeignAdoptionSpec == Init /\ [][Next \/ ForeignAdoption]_vars
StaleCandidateCommitSpec == Init /\ [][Next \/ StaleCandidateCommit]_vars
FailedUpdateLeakSpec == Init /\ [][Next \/ FailedUpdateLeak]_vars
SemanticPollutionSpec == Init /\ [][Next \/ SemanticPollution]_vars

=============================================================================
