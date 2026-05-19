------------------------ MODULE CostAccountingThreats ------------------------
(****************************************************************************)
(* Finite-state threat model for replay tampering, activation downgrade,     *)
(* unauthorized settlement, cost-invalid evidence recording, and fuel        *)
(* immutability across settlement.                                           *)
(****************************************************************************)

EXTENDS Naturals, TLC

CONSTANTS
    GoodDigest,
    BadDigest,
    InitialFuel

VARIABLES
    mode,
    present,
    committedDigest,
    actualDigest,
    committedCount,
    actualCount,
    accepted,
    fuel,
    violation,
    evidence

vars ==
    <<mode, present, committedDigest, actualDigest, committedCount,
      actualCount, accepted, fuel, violation, evidence>>

ModeSet == {"Legacy", "CostAccounted"}
DigestSet == {GoodDigest, BadDigest}
CountSet == 0..2

ReplayPayloadValid ==
    /\ present
    /\ committedDigest = actualDigest
    /\ committedCount = actualCount

Init ==
    /\ mode = "CostAccounted"
    /\ present = TRUE
    /\ committedDigest = GoodDigest
    /\ actualDigest = GoodDigest
    /\ committedCount = 1
    /\ actualCount = 1
    /\ accepted = FALSE
    /\ fuel = InitialFuel
    /\ violation = FALSE
    /\ evidence = FALSE

TypeOK ==
    /\ mode \in ModeSet
    /\ present \in BOOLEAN
    /\ committedDigest \in DigestSet
    /\ actualDigest \in DigestSet
    /\ committedCount \in CountSet
    /\ actualCount \in CountSet
    /\ accepted \in BOOLEAN
    /\ fuel \in 0..InitialFuel
    /\ violation \in BOOLEAN
    /\ evidence \in BOOLEAN

ValidateReplay ==
    /\ accepted' =
        IF mode = "Legacy"
        THEN TRUE
        ELSE ReplayPayloadValid
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, fuel, violation, evidence>>

TamperDigest ==
    /\ actualDigest' = BadDigest
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<mode, present, committedDigest, committedCount,
        actualCount, fuel, evidence>>

TamperCount ==
    /\ actualCount' = IF actualCount = 1 THEN 2 ELSE 1
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, fuel, evidence>>

DropCommitment ==
    /\ present' = FALSE
    /\ committedDigest' = BadDigest
    /\ committedCount' = 0
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<mode, actualDigest, actualCount, fuel, evidence>>

LegacyDowngradeAttempt ==
    /\ mode' = "Legacy"
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<present, committedDigest, actualDigest, committedCount,
        actualCount, fuel, evidence>>

UnauthorizedSettlementAttempt ==
    /\ violation' = TRUE
    /\ accepted' = FALSE
    /\ fuel' = fuel
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, evidence>>

AuthorizedSettlement ==
    /\ fuel' = fuel
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, violation, evidence>>

RecordEvidence ==
    /\ evidence' = violation
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, fuel, violation>>

Next ==
    ValidateReplay \/ TamperDigest \/ TamperCount \/ DropCommitment \/
    LegacyDowngradeAttempt \/ UnauthorizedSettlementAttempt \/
    AuthorizedSettlement \/ RecordEvidence

Spec == Init /\ [][Next]_vars

CostAccountedReplayAcceptsOnlyValidPayload ==
    mode = "CostAccounted" /\ accepted => ReplayPayloadValid

CostAccountedReplayRejectsMissingCommitment ==
    mode = "CostAccounted" /\ ~present => ~accepted

SettlementNeverAddsRuntimeFuel ==
    fuel <= InitialFuel

CostInvalidEvidenceHasViolation ==
    evidence => violation

ReplayTamperCannotStayAccepted ==
    violation /\ mode = "CostAccounted" /\ ~ReplayPayloadValid => ~accepted

=============================================================================
