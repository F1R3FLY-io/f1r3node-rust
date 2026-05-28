------------------------ MODULE CostAccountingThreats ------------------------
(****************************************************************************)
(* Finite-state threat model for replay tampering, activation downgrade,     *)
(* unauthorized settlement, cost-invalid evidence recording, and fuel        *)
(* immutability across settlement.                                           *)
(****************************************************************************)

EXTENDS Naturals, TLC

CONSTANTS
    \* @type: Str;
    GoodDigest,
    \* @type: Str;
    BadDigest,
    \* @type: Int;
    InitialFuel

VARIABLES
    \* @type: Str;
    mode,
    \* @type: Bool;
    present,
    \* @type: Str;
    committedDigest,
    \* @type: Str;
    actualDigest,
    \* @type: Int;
    committedCount,
    \* @type: Int;
    actualCount,
    \* @type: Bool;
    accepted,
    \* @type: Int;
    fuel,
    \* @type: Bool;
    violation,
    \* @type: Bool;
    evidence,
    \* @type: Int;
    evidenceEpoch,
    \* @type: Int;
    targetActivationEpoch,
    \* @type: Int;
    currentEpoch,
    \* @type: Int;
    parentBond,
    \* @type: Int;
    ambientBond,
    \* @type: Int;
    executionBond,
    \* @type: Bool;
    recoveredSlash,
    \* @type: Bool;
    slashAccepted,
    \* @type: Bool;
    slashNoop,
    \* @type: Int;
    costBoundary

vars ==
    <<mode, present, committedDigest, actualDigest, committedCount,
      actualCount, accepted, fuel, violation, evidence, evidenceEpoch,
      targetActivationEpoch, currentEpoch, parentBond, ambientBond,
      executionBond, recoveredSlash, slashAccepted, slashNoop, costBoundary>>

ModeSet == {"Legacy", "CostAccounted"}
DigestSet == {GoodDigest, BadDigest}
CountSet == 0..2
EpochSet == 0..1
BondSet == 0..1

(* ─────────────────────────────────────────────────────────────────────────
   TM-CA-151 — DIAGNOSTIC-REFINEMENT LEVEL (not the production consensus surface).
   This model authenticates a digest-INCLUSIVE replay payload: ReplayPayloadValid
   below requires committedDigest = actualDigest /\ committedCount = actualCount,
   and TamperDigest / TamperCount / ReplayTamperCannotStayAccepted treat per-op
   digest/count tampering as rejection-causing. Per TM-CA-151
   (docs/theory/cost-accounting-threat-model.md) the per-operation cost-trace
   digest / event-count / presence are DIAGNOSTIC ONLY and were removed from
   production consensus; the production consensus surface is total_cost (clamped
   on OOP) + status + post-state hash. The predicates/actions/invariants here
   remain TRUE statements about the strictly-finer digest-inclusive refinement
   level and are retained as diagnostic-refinement checks, NOT as claims that the
   per-operation digest is a production consensus quantity. (Cf. the re-aimed
   RuntimeBudgetReplay.tla ConsumedAndVerdictScheduleIndependent, which is the
   consensus-surface invariant.)
   ───────────────────────────────────────────────────────────────────────── *)
ReplayPayloadValid ==
    /\ present
    /\ committedDigest = actualDigest
    /\ committedCount = actualCount

CurrentSlashEvidence ==
    /\ evidenceEpoch = currentEpoch
    /\ targetActivationEpoch = currentEpoch

ParentPreStateAuthorizesSlash ==
    /\ CurrentSlashEvidence
    /\ parentBond > 0

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
    /\ evidenceEpoch = 0
    /\ targetActivationEpoch = 0
    /\ currentEpoch = 0
    /\ parentBond = 1
    /\ ambientBond = 0
    /\ executionBond = 1
    /\ recoveredSlash = FALSE
    /\ slashAccepted = FALSE
    /\ slashNoop = FALSE
    /\ costBoundary = InitialFuel

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
    /\ evidenceEpoch \in EpochSet
    /\ targetActivationEpoch \in EpochSet
    /\ currentEpoch \in EpochSet
    /\ parentBond \in BondSet
    /\ ambientBond \in BondSet
    /\ executionBond \in BondSet
    /\ recoveredSlash \in BOOLEAN
    /\ slashAccepted \in BOOLEAN
    /\ slashNoop \in BOOLEAN
    /\ costBoundary \in 0..InitialFuel

ValidateReplay ==
    /\ accepted' =
        IF mode = "Legacy"
        THEN TRUE
        ELSE ReplayPayloadValid
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, fuel, violation, evidence,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, recoveredSlash, slashAccepted,
        slashNoop, costBoundary>>

\* TM-CA-151: digest/count are diagnostic; this models the digest-inclusive refinement level, not a production consensus rejection.
TamperDigest ==
    /\ actualDigest' = BadDigest
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<mode, present, committedDigest, committedCount,
        actualCount, fuel, evidence, evidenceEpoch, targetActivationEpoch,
        currentEpoch, parentBond, ambientBond, executionBond,
        recoveredSlash, slashAccepted, slashNoop, costBoundary>>

\* TM-CA-151: digest/count are diagnostic; this models the digest-inclusive refinement level, not a production consensus rejection.
TamperCount ==
    /\ actualCount' = IF actualCount = 1 THEN 2 ELSE 1
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, fuel, evidence, evidenceEpoch, targetActivationEpoch,
        currentEpoch, parentBond, ambientBond, executionBond,
        recoveredSlash, slashAccepted, slashNoop, costBoundary>>

DropCommitment ==
    /\ present' = FALSE
    /\ committedDigest' = BadDigest
    /\ committedCount' = 0
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<mode, actualDigest, actualCount, fuel, evidence,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, recoveredSlash, slashAccepted,
        slashNoop, costBoundary>>

LegacyDowngradeAttempt ==
    /\ mode' = "Legacy"
    /\ accepted' = FALSE
    /\ violation' = TRUE
    /\ UNCHANGED <<present, committedDigest, actualDigest, committedCount,
        actualCount, fuel, evidence, evidenceEpoch, targetActivationEpoch,
        currentEpoch, parentBond, ambientBond, executionBond,
        recoveredSlash, slashAccepted, slashNoop, costBoundary>>

UnauthorizedSettlementAttempt ==
    /\ violation' = TRUE
    /\ accepted' = FALSE
    /\ fuel' = fuel
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, evidence, evidenceEpoch,
        targetActivationEpoch, currentEpoch, parentBond, ambientBond,
        executionBond, recoveredSlash, slashAccepted, slashNoop,
        costBoundary>>

AuthorizedSettlement ==
    /\ fuel' = fuel
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, violation, evidence,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, recoveredSlash, slashAccepted,
        slashNoop, costBoundary>>

RecordEvidence ==
    /\ evidence' = violation
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, fuel, violation,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, recoveredSlash, slashAccepted,
        slashNoop, costBoundary>>

SelectSlashView ==
    /\ evidenceEpoch' \in EpochSet
    /\ targetActivationEpoch' \in EpochSet
    /\ currentEpoch' \in EpochSet
    /\ parentBond' \in BondSet
    /\ ambientBond' \in BondSet
    /\ executionBond' \in BondSet
    /\ recoveredSlash' = FALSE
    /\ slashAccepted' = FALSE
    /\ slashNoop' = FALSE
    /\ costBoundary' = costBoundary
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, fuel, violation, evidence>>

RecoverRejectedSlash ==
    /\ recoveredSlash' = CurrentSlashEvidence
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, fuel, violation, evidence,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, slashAccepted, slashNoop, costBoundary>>

AuthorizeSlash ==
    /\ slashAccepted' = ParentPreStateAuthorizesSlash
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, fuel, violation, evidence,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, recoveredSlash, slashNoop, costBoundary>>

ExecuteSlashNoop ==
    /\ slashAccepted
    /\ executionBond = 0
    /\ slashNoop' = TRUE
    /\ fuel' = fuel
    /\ costBoundary' = costBoundary
    /\ UNCHANGED <<mode, present, committedDigest, actualDigest,
        committedCount, actualCount, accepted, violation, evidence,
        evidenceEpoch, targetActivationEpoch, currentEpoch, parentBond,
        ambientBond, executionBond, recoveredSlash, slashAccepted>>

Next ==
    ValidateReplay \/ TamperDigest \/ TamperCount \/ DropCommitment \/
    LegacyDowngradeAttempt \/ UnauthorizedSettlementAttempt \/
    AuthorizedSettlement \/ RecordEvidence \/ SelectSlashView \/
    RecoverRejectedSlash \/ AuthorizeSlash \/ ExecuteSlashNoop

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

RecoveredSlashRequiresCurrentEvidence ==
    recoveredSlash => CurrentSlashEvidence

SlashAuthorizationUsesParentPreState ==
    slashAccepted => ParentPreStateAuthorizesSlash

AmbientBondDoesNotAuthorizeWithoutParent ==
    (parentBond = 0 /\ ambientBond > 0) => ~slashAccepted

ParentPositiveAmbientZeroCanAuthorize ==
    (CurrentSlashEvidence /\ parentBond > 0 /\ ambientBond = 0) =>
        ParentPreStateAuthorizesSlash

SlashNoopPreservesCostBoundary ==
    slashNoop => (costBoundary = InitialFuel /\ fuel <= InitialFuel)

=============================================================================
