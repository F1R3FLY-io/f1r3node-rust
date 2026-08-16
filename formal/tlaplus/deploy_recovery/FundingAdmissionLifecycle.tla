---------------------- MODULE FundingAdmissionLifecycle ----------------------
EXTENDS Naturals

CONSTANTS RecordUnderfunded, ValidateFromRecordedState

Demand == 1
MaxSupply == 1
Decisions == {"none", "execute", "reject"}
Statuses == {
    "pending",
    "proposed",
    "executed-recorded",
    "rejected-recorded",
    "executed-finalized",
    "rejected-finalized",
    "invalid"
}

Classify(available) == IF available >= Demand THEN "execute" ELSE "reject"

VARIABLES
    supply,
    status,
    attempted,
    recordedSupply,
    proposerDecision,
    validatorDecision,
    userEffects

vars == <<
    supply,
    status,
    attempted,
    recordedSupply,
    proposerDecision,
    validatorDecision,
    userEffects
>>

Init ==
    /\ supply = 0
    /\ status = "pending"
    /\ attempted = FALSE
    /\ recordedSupply = 0
    /\ proposerDecision = "none"
    /\ validatorDecision = "none"
    /\ userEffects = 0

TopUp ==
    /\ supply < MaxSupply
    /\ supply' = MaxSupply
    /\ UNCHANGED <<status, attempted, recordedSupply, proposerDecision,
                    validatorDecision, userEffects>>

Propose ==
    /\ status = "pending"
    /\ attempted' = TRUE
    /\ recordedSupply' = supply
    /\ proposerDecision' = Classify(supply)
    /\ validatorDecision' = "none"
    /\ status' =
        IF Classify(supply) = "reject" /\ ~RecordUnderfunded
        THEN "pending"
        ELSE "proposed"
    /\ UNCHANGED <<supply, userEffects>>

Validate ==
    /\ status = "proposed"
    /\ LET observedSupply ==
               IF ValidateFromRecordedState THEN recordedSupply ELSE supply
           replayDecision == Classify(observedSupply)
       IN /\ validatorDecision' = replayDecision
          /\ status' =
              IF replayDecision # proposerDecision
              THEN "invalid"
              ELSE IF proposerDecision = "execute"
                   THEN "executed-recorded"
                   ELSE "rejected-recorded"
          /\ userEffects' = IF replayDecision = "execute" THEN 1 ELSE 0
    /\ UNCHANGED <<supply, attempted, recordedSupply, proposerDecision>>

FinalizeExecuted ==
    /\ status = "executed-recorded"
    /\ status' = "executed-finalized"
    /\ UNCHANGED <<supply, attempted, recordedSupply, proposerDecision,
                    validatorDecision, userEffects>>

FinalizeRejected ==
    /\ status = "rejected-recorded"
    /\ status' = "rejected-finalized"
    /\ UNCHANGED <<supply, attempted, recordedSupply, proposerDecision,
                    validatorDecision, userEffects>>

Next == TopUp \/ Propose \/ Validate \/ FinalizeExecuted \/ FinalizeRejected

Spec ==
    Init
    /\ [][Next]_vars
    /\ WF_vars(Validate)
    /\ WF_vars(FinalizeExecuted)
    /\ WF_vars(FinalizeRejected)

TypeOK ==
    /\ supply \in 0..MaxSupply
    /\ status \in Statuses
    /\ attempted \in BOOLEAN
    /\ recordedSupply \in 0..MaxSupply
    /\ proposerDecision \in Decisions
    /\ validatorDecision \in Decisions
    /\ userEffects \in 0..1

Inv_ValidatorUsesProposalPreState == status # "invalid"

Inv_UnderfundedAttemptLeavesPending ==
    attempted /\ recordedSupply < Demand => status # "pending"

Inv_RejectionHasNoUserEffects ==
    status \in {"rejected-recorded", "rejected-finalized"} => userEffects = 0

Inv_TerminalRejectionNeverExecutes ==
    status = "rejected-finalized" => proposerDecision = "reject"

Live_RecordedDecisionFinalizes ==
    status \in {"executed-recorded", "rejected-recorded"}
    ~> status \in {"executed-finalized", "rejected-finalized"}
=============================================================================
