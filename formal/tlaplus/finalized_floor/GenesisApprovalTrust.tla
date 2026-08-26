-------------------------- MODULE GenesisApprovalTrust --------------------------
EXTENDS Integers, TLC

CONSTANT
    \* @type: Int;
    MaxThreshold,
    \* @type: Str;
    Defect

ASSUME /\ MaxThreshold \in Nat \ {0}
       /\ Defect \in {"None", "UseLocalMinimum", "PermitDowngrade",
                       "CountAllSignatures", "MutateOnReject"}

VARIABLES
    \* @type: Int;
    localMinimum,
    \* @type: Int;
    candidateThreshold,
    \* @type: Int;
    bondedValidatorCount,
    \* @type: Int;
    validDistinctSignatureCount,
    \* @type: Int;
    suppliedSignatureCount,
    \* @type: Bool;
    attempted,
    \* @type: Bool;
    installed,
    \* @type: Int;
    installationWrites

vars == <<localMinimum, candidateThreshold, bondedValidatorCount,
          validDistinctSignatureCount, suppliedSignatureCount,
          attempted, installed, installationWrites>>

ProtocolAuthorized(local, candidate, bonded, valid) ==
    /\ local >= 0
    /\ candidate >= local
    /\ candidate <= bonded
    /\ candidate <= valid

ImplementationAuthorized(local, candidate, bonded, valid, supplied) ==
    /\ IF Defect = "PermitDowngrade"
          THEN candidate >= 0
          ELSE candidate >= local
    /\ candidate <= bonded
    /\ IF Defect = "UseLocalMinimum"
          THEN valid >= local
          ELSE IF Defect = "CountAllSignatures"
                 THEN supplied >= candidate
                 ELSE valid >= candidate

Init ==
    /\ localMinimum \in 0..MaxThreshold
    /\ candidateThreshold = 0
    /\ bondedValidatorCount = 0
    /\ validDistinctSignatureCount = 0
    /\ suppliedSignatureCount = 0
    /\ attempted = FALSE
    /\ installed = FALSE
    /\ installationWrites = 0

Attempt(candidate, bonded, valid, supplied) ==
    /\ ~attempted
    /\ candidate \in 0..MaxThreshold
    /\ bonded \in 0..MaxThreshold
    /\ valid \in 0..MaxThreshold
    /\ supplied \in 0..MaxThreshold
    /\ valid <= bonded
    /\ valid <= supplied
    /\ candidateThreshold' = candidate
    /\ bondedValidatorCount' = bonded
    /\ validDistinctSignatureCount' = valid
    /\ suppliedSignatureCount' = supplied
    /\ attempted' = TRUE
    /\ IF ImplementationAuthorized(localMinimum, candidate, bonded, valid, supplied)
          THEN /\ installed' = TRUE
               /\ installationWrites' = 1
          ELSE /\ installed' = FALSE
               /\ installationWrites' = IF Defect = "MutateOnReject" THEN 1 ELSE 0
    /\ UNCHANGED localMinimum

Next ==
    \E candidate, bonded, valid, supplied \in 0..MaxThreshold :
        Attempt(candidate, bonded, valid, supplied)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ localMinimum \in 0..MaxThreshold
    /\ candidateThreshold \in 0..MaxThreshold
    /\ bondedValidatorCount \in 0..MaxThreshold
    /\ validDistinctSignatureCount \in 0..MaxThreshold
    /\ suppliedSignatureCount \in 0..MaxThreshold
    /\ attempted \in BOOLEAN
    /\ installed \in BOOLEAN
    /\ installationWrites \in 0..1

Inv_InstalledIsProtocolAuthorized ==
    installed =>
      ProtocolAuthorized(localMinimum, candidateThreshold,
                         bondedValidatorCount, validDistinctSignatureCount)

Inv_ZeroSignatureRequiresZeroMinimum ==
    (installed /\ candidateThreshold = 0 /\ validDistinctSignatureCount = 0)
      => localMinimum = 0

Inv_RejectionDoesNotMutate ==
    (attempted /\
     ~ImplementationAuthorized(localMinimum, candidateThreshold,
                               bondedValidatorCount, validDistinctSignatureCount,
                               suppliedSignatureCount))
      => installationWrites = 0

Safety ==
    /\ TypeOK
    /\ Inv_InstalledIsProtocolAuthorized
    /\ Inv_ZeroSignatureRequiresZeroMinimum
    /\ Inv_RejectionDoesNotMutate

=============================================================================
