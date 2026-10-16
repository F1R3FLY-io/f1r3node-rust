---------------- MODULE StagedGuestFundingObligations ----------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANT Defect
ASSUME Defect \in {"None", "Omission", "Duplication", "Reordering",
  "StaleIdentity", "CandidateMultiplicity", "FirstCandidateOnly",
  "StaleCapability", "CancelLeak", "RefusalLeak", "ExtraFundingCase",
  "IndependentCandidateSupply"}

VARIABLES nativePhase, nativeCosts, nativePrepared, nativePlay, nativeTrace,
          nativeUsed, nativeReplay, nativeReplayUsed,
          phase, journal, replayJournal, live, funded, initialDurable,
          durable, cache, publishedJournal, publishedLive, cases,
          lastAction, beforeMoney, supplyCapacity

nativeVars == <<nativePhase, nativeCosts, nativePrepared, nativePlay,
                nativeTrace, nativeUsed, nativeReplay, nativeReplayUsed>>
guestVars == <<phase, journal, replayJournal, live, funded, initialDurable,
               durable, cache, publishedJournal, publishedLive, cases>>
vars == <<nativeVars, guestVars, lastAction, beforeMoney, supplyCapacity>>

Native == INSTANCE NativeAttemptReplay
  WITH Workers <- {1, 2}, Limit <- 1,
       RecordDenied <- TRUE, UseRecordedDecision <- TRUE,
       phase <- nativePhase, costs <- nativeCosts, prepared <- nativePrepared,
       play <- nativePlay, trace <- nativeTrace, used <- nativeUsed,
       replay <- nativeReplay, replayUsed <- nativeReplayUsed

Candidates == {1, 2}
ExpectedRow(i) ==
  [session |-> 1, generation |-> 1, root |-> 1, prefix |-> i - 1,
   operation |-> i, invocation |-> i,
   candidates |-> IF i = 1 THEN <<1, 1>> ELSE <<1, 2>>]
ExpectedJournal == <<ExpectedRow(1), ExpectedRow(2)>>
ExpectedRows == {ExpectedRow(1), ExpectedRow(2)}
StaleRows == {
  [ExpectedRow(1) EXCEPT !.session = 2],
  [ExpectedRow(1) EXCEPT !.generation = 2],
  [ExpectedRow(1) EXCEPT !.root = 2],
  [ExpectedRow(1) EXCEPT !.prefix = 2],
  [ExpectedRow(1) EXCEPT !.operation = 2],
  [ExpectedRow(1) EXCEPT !.invocation = 2]}
ShortRoster == [ExpectedRow(1) EXCEPT !.candidates = <<1>>]
Rows == ExpectedRows \cup StaleRows \cup {ShortRoster}
Money == <<nativeUsed, nativeReplayUsed, cases, 7, 9>>

Stamp(kind) == /\ lastAction' = kind /\ beforeMoney' = Money
AllFunded(rows) ==
  \A i \in 1..Len(rows) :
    \A j \in 1..Len(rows[i].candidates) : funded[rows[i].candidates[j]]
CandidateChecks ==
  IF Defect = "FirstCandidateOnly"
  THEN \A i \in 1..Len(journal) : funded[journal[i].candidates[1]]
  ELSE AllFunded(journal)
SupplyWitnesses == {<<left, right>> : left \in 0..supplyCapacity,
                                     right \in 0..supplyCapacity}
Feasible(witness) == witness[1] + witness[2] <= supplyCapacity
RosterHasCommonSupply(row) ==
  \E witness \in SupplyWitnesses :
    /\ Feasible(witness)
    /\ \A j \in 1..Len(row.candidates) : witness[row.candidates[j]] >= 1
IndependentCandidateSupply(row) ==
  \A j \in 1..Len(row.candidates) :
    \E witness \in SupplyWitnesses :
      /\ Feasible(witness)
      /\ witness[row.candidates[j]] >= 1
CommonSupplyChecks ==
  \A i \in 1..Len(journal) :
    IF Defect = "IndependentCandidateSupply"
    THEN IndependentCandidateSupply(journal[i])
    ELSE RosterHasCommonSupply(journal[i])
ReplayChecks ==
  replayJournal = journal \/
  CASE Defect = "Omission" -> replayJournal = <<journal[1]>>
    [] Defect = "Duplication" -> replayJournal = <<journal[1], journal[1]>>
    [] Defect = "Reordering" -> replayJournal = <<journal[2], journal[1]>>
    [] Defect = "StaleIdentity" ->
         \E row \in StaleRows : replayJournal = <<row, journal[2]>>
    [] Defect = "CandidateMultiplicity" ->
         replayJournal = <<ShortRoster, journal[2]>>
    [] OTHER -> FALSE

Init ==
  /\ Native!Init
  /\ phase = "Open"
  /\ journal = <<>>
  /\ replayJournal = <<>>
  /\ live = TRUE
  /\ funded \in [Candidates -> BOOLEAN]
  /\ supplyCapacity \in {1, 2}
  /\ initialDurable \in {0, 9}
  /\ durable = initialDurable
  /\ cache = 0
  /\ publishedJournal = <<>>
  /\ publishedLive = FALSE
  /\ cases = 1
  /\ lastAction = "Init"
  /\ beforeMoney = Money

NativeStep ==
  /\ phase = "Open"
  /\ Native!Next
  /\ Stamp("Native")
  /\ UNCHANGED guestVars

AppendGuest ==
  /\ phase = "Open"
  /\ Len(journal) < Len(ExpectedJournal)
  /\ journal' = Append(journal, ExpectedJournal[Len(journal) + 1])
  /\ Stamp("Append")
  /\ UNCHANGED <<nativeVars, phase, replayJournal, live, funded,
                  initialDurable, durable, cache, publishedJournal,
                  publishedLive, cases>>

Seal ==
  /\ phase = "Open"
  /\ nativePhase = "Done"
  /\ journal = ExpectedJournal
  /\ phase' = "Replay"
  /\ Stamp("Seal")
  /\ UNCHANGED <<nativeVars, journal, replayJournal, live, funded,
                  initialDurable, durable, cache, publishedJournal,
                  publishedLive, cases>>

ReplayGuest ==
  /\ phase = "Replay"
  /\ Len(replayJournal) < Len(journal)
  /\ replayJournal' = Append(replayJournal, journal[Len(replayJournal) + 1])
  /\ Stamp("ReplayGuest")
  /\ UNCHANGED <<nativeVars, phase, journal, live, funded,
                  initialDurable, durable, cache, publishedJournal,
                  publishedLive, cases>>

CorruptReplay ==
  /\ phase = "Replay"
  /\ replayJournal = <<>>
  /\ CASE Defect = "Omission" -> replayJournal' = <<journal[1]>>
       [] Defect = "Duplication" -> replayJournal' = <<journal[1], journal[1]>>
       [] Defect = "Reordering" -> replayJournal' = <<journal[2], journal[1]>>
       [] Defect = "StaleIdentity" ->
            \E row \in StaleRows : replayJournal' = <<row, journal[2]>>
       [] Defect = "CandidateMultiplicity" ->
            replayJournal' = <<ShortRoster, journal[2]>>
       [] OTHER -> FALSE
  /\ Stamp("CorruptReplay")
  /\ UNCHANGED <<nativeVars, phase, journal, live, funded,
                  initialDurable, durable, cache, publishedJournal,
                  publishedLive, cases>>

Verify ==
  /\ phase = "Replay"
  /\ journal = ExpectedJournal
  /\ ReplayChecks
  /\ CandidateChecks
  /\ CommonSupplyChecks
  /\ phase' = "Verified"
  /\ Stamp("Verify")
  /\ UNCHANGED <<nativeVars, journal, replayJournal, live, funded,
                  initialDurable, durable, cache, publishedJournal,
                  publishedLive, cases>>

Revoke ==
  /\ phase \in {"Open", "Replay", "Verified"}
  /\ live
  /\ live' = FALSE
  /\ Stamp("Revoke")
  /\ UNCHANGED <<nativeVars, phase, journal, replayJournal, funded,
                  initialDurable, durable, cache, publishedJournal,
                  publishedLive, cases>>

Publish ==
  /\ phase = "Verified"
  /\ live \/ Defect = "StaleCapability"
  /\ durable = 0
  /\ phase' = "Published"
  /\ durable' = 1
  /\ cache' = 1
  /\ publishedJournal' = replayJournal
  /\ publishedLive' = live
  /\ cases' = IF Defect = "ExtraFundingCase" THEN cases + 1 ELSE cases
  /\ Stamp("Publish")
  /\ UNCHANGED <<nativeVars, journal, replayJournal, live, funded, initialDurable>>

Rollback(reason) ==
  /\ phase \in {"Open", "Replay", "Verified"}
  /\ reason \in {"Cancelled", "Refused"}
  /\ phase' = reason
  /\ journal' = <<>>
  /\ replayJournal' = <<>>
  /\ LET leak == Len(journal) > 0 /\
          ((reason = "Cancelled" /\ Defect = "CancelLeak") \/
           (reason = "Refused" /\ Defect = "RefusalLeak"))
     IN /\ durable' = IF leak THEN 1 ELSE initialDurable
        /\ cache' = IF leak THEN 1 ELSE 0
        /\ publishedJournal' = IF leak THEN journal ELSE <<>>
  /\ Stamp(reason)
  /\ UNCHANGED <<nativeVars, live, funded, initialDurable, publishedLive, cases>>

Next ==
  /\ (NativeStep \/ AppendGuest \/ Seal \/ ReplayGuest \/ CorruptReplay \/
      Verify \/ Revoke \/ Publish \/ Rollback("Cancelled") \/ Rollback("Refused"))
  /\ UNCHANGED supplyCapacity
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ Native!TypeOK
  /\ phase \in {"Open", "Replay", "Verified", "Published", "Cancelled", "Refused"}
  /\ journal \in Seq(Rows)
  /\ replayJournal \in Seq(Rows)
  /\ publishedJournal \in Seq(Rows)
  /\ live \in BOOLEAN
  /\ publishedLive \in BOOLEAN
  /\ funded \in [Candidates -> BOOLEAN]
  /\ supplyCapacity \in {1, 2}
  /\ initialDurable \in {0, 9}
  /\ durable \in {0, 1, 9}
  /\ cache \in {0, 1}
  /\ cases \in 1..2
  /\ lastAction \in {"Init", "Native", "Append", "Seal", "ReplayGuest",
                       "CorruptReplay", "Verify", "Revoke", "Publish",
                       "Cancelled", "Refused"}

NativeSafety == Native!ExactUsage /\ Native!HardCeiling /\
  Native!RecordedAttemptCoverage /\ Native!ReplayDecisionAgreement /\
  Native!ReplayUsageAgreement /\ Native!IndependentReplayEnabled
CompleteAppend == journal = SubSeq(ExpectedJournal, 1, Len(journal))
PublishedRosterExact == phase = "Published" => publishedJournal = ExpectedJournal
AllCandidatesFundedAtPublication == phase = "Published" => AllFunded(publishedJournal)
CommonSupplyAtPublication == phase = "Published" =>
  \A i \in 1..Len(publishedJournal) : RosterHasCommonSupply(publishedJournal[i])
AuthorityAtPublication == phase = "Published" => publishedLive
PublicationRequiresVerification == durable = 1 => phase = "Published"
RollbackNoGuestLeak == phase \in {"Cancelled", "Refused"} =>
  /\ journal = <<>> /\ replayJournal = <<>> /\ publishedJournal = <<>>
  /\ durable = initialDurable /\ cache = 0
NoExtraMonetaryCharge == lastAction \notin {"Init", "Native"} => Money = beforeMoney
OneExistingFundingCase == cases = 1
=============================================================================
