---------------------- MODULE WalletFundedLollipop ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    \* @type: Set(Str);
    Payers,
    \* @type: Set(Str);
    Validators,
    \* @type: Set(Str);
    Names,
    \* @type: Set(Str);
    Callers,
    \* @type: Str;
    Sponsor,
    \* @type: Str;
    Outer,
    \* @type: Str;
    Slot,
    \* @type: Str;
    Gateway,
    \* @type: Str;
    Attacker,
    \* @type: Str;
    Proposer,
    \* @type: Str;
    DivergentValidator,
    \* @type: Str;
    OuterAddress,
    \* @type: Str;
    SlotAddress,
    \* @type: Str;
    SlotCapability,
    \* @type: Str;
    NoPayer,
    \* @type: Str;
    NoCaller,
    \* @type: Str -> Int;
    InitialBalance,
    \* @type: Int;
    OuterFundingAmount,
    \* @type: Int;
    SlotFundingAmount,
    \* @type: Int;
    OuterCertifiedCostBound,
    \* @type: Int;
    CertifiedCostBound,
    \* @type: Int;
    OuterRealizedCost,
    \* @type: Int;
    RealizedCost,
    \* @type: Int;
    FixedFee,
    \* @type: Bool;
    AllowFundingCopy,
    \* @type: Bool;
    AllowCapabilityLeak,
    \* @type: Bool;
    AllowPayerCollapse,
    \* @type: Bool;
    AllowReplayOmission,
    \* @type: Bool;
    AllowMissingOuter,
    \* @type: Bool;
    AllowGatewayAuthBypass,
    \* @type: Bool;
    AllowActivationBeforeFunding,
    \* @type: Bool;
    ChargeCertifiedBound

ASSUME /\ Payers = {Sponsor, Outer, Slot, Gateway, Proposer}
       /\ Cardinality(Payers) = 5
       /\ Validators # {}
       /\ Callers = {Gateway, Attacker}
       /\ Gateway # Attacker
       /\ Attacker \notin Payers
       /\ DivergentValidator \in Validators
       /\ OuterAddress \in Names
       /\ SlotAddress \in Names
       /\ SlotCapability \in Names
       /\ Cardinality({OuterAddress, SlotAddress, SlotCapability}) = 3
       /\ NoPayer \notin Payers
       /\ NoCaller \notin Callers
       /\ InitialBalance \in [Payers -> Nat]
       /\ OuterFundingAmount \in Nat \ {0}
       /\ SlotFundingAmount \in Nat \ {0}
       /\ OuterFundingAmount + SlotFundingAmount <= InitialBalance[Sponsor]
       /\ OuterCertifiedCostBound \in Nat \ {0}
       /\ CertifiedCostBound \in Nat \ {0}
       /\ OuterRealizedCost \in Nat
       /\ RealizedCost \in Nat
       /\ OuterRealizedCost <= OuterCertifiedCostBound
       /\ RealizedCost <= CertifiedCostBound
       /\ OuterCertifiedCostBound <= InitialBalance[Outer] + OuterFundingAmount
       /\ CertifiedCostBound <= InitialBalance[Slot] + SlotFundingAmount
       /\ FixedFee \in Nat
       /\ FixedFee <= InitialBalance[Gateway]
       /\ \A flag \in {
            AllowFundingCopy,
            AllowCapabilityLeak,
            AllowPayerCollapse,
            AllowReplayOmission,
            AllowMissingOuter,
            AllowGatewayAuthBypass,
            AllowActivationBeforeFunding,
            ChargeCertifiedBound
          } : flag \in BOOLEAN

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Bool;
    outerCommitted,
    \* @type: Bool;
    continuationStored,
    \* @type: Bool;
    addressesPublished,
    \* @type: Bool;
    continuationHasCapability,
    \* @type: Bool;
    gatewayHasCapability,
    \* @type: Bool;
    gatewayAuthenticated,
    \* @type: Str;
    authorizedCaller,
    \* @type: Bool;
    unauthorizedAttempted,
    \* @type: Bool;
    fundingCommitted,
    \* @type: Str -> Int;
    balance,
    \* @type: Int;
    burned,
    \* @type: Str -> Str;
    validationStatus,
    \* @type: Str -> Str;
    validationOuterPayer,
    \* @type: Str -> Str;
    validationContinuationPayer,
    \* @type: Str -> Int;
    preSettlementBalance,
    \* @type: Bool;
    committed,
    \* @type: Str -> Int;
    replayBalance,
    \* @type: Int;
    replayBurned,
    \* @type: Str;
    replayOuterPayer,
    \* @type: Str;
    replayContinuationPayer

vars == <<phase, outerCommitted, continuationStored, addressesPublished,
          continuationHasCapability, gatewayHasCapability, gatewayAuthenticated,
          authorizedCaller, unauthorizedAttempted, fundingCommitted,
          balance, burned, validationStatus, validationOuterPayer,
          validationContinuationPayer, preSettlementBalance, committed,
          replayBalance, replayBurned, replayOuterPayer,
          replayContinuationPayer>>

CanonicalTotal(function) ==
    function[Sponsor] + function[Outer] + function[Slot]
      + function[Gateway] + function[Proposer]

CanonicalOuterPayer(validator) ==
    IF AllowPayerCollapse /\ validator = DivergentValidator
    THEN Gateway
    ELSE Outer

CanonicalContinuationPayer(validator) ==
    IF AllowPayerCollapse /\ validator = DivergentValidator
    THEN Gateway
    ELSE Slot

ChargedOuterCost ==
    IF ChargeCertifiedBound THEN OuterCertifiedCostBound ELSE OuterRealizedCost

ChargedContinuationCost ==
    IF ChargeCertifiedBound THEN CertifiedCostBound ELSE RealizedCost

ValidatorCanAdmit(validator) ==
    /\ gatewayAuthenticated
    /\ fundingCommitted
    /\ continuationStored
    /\ continuationHasCapability
    /\ (outerCommitted \/ AllowMissingOuter)
    /\ balance[CanonicalOuterPayer(validator)] >= OuterCertifiedCostBound
    /\ balance[CanonicalContinuationPayer(validator)] >= CertifiedCostBound
    /\ balance[Gateway] >= FixedFee

AllValidated ==
    \A validator \in Validators : validationStatus[validator] # "Unknown"

AllAdmitted ==
    \A validator \in Validators : validationStatus[validator] = "Admitted"

AllCertifiedForLocatedPurses ==
    \A validator \in Validators :
      /\ validationOuterPayer[validator] = Outer
      /\ validationContinuationPayer[validator] = Slot

Init ==
    /\ phase = "Install"
    /\ outerCommitted = FALSE
    /\ continuationStored = FALSE
    /\ addressesPublished = FALSE
    /\ continuationHasCapability = FALSE
    /\ gatewayHasCapability = FALSE
    /\ gatewayAuthenticated = FALSE
    /\ authorizedCaller = NoCaller
    /\ unauthorizedAttempted = FALSE
    /\ fundingCommitted = FALSE
    /\ balance = InitialBalance
    /\ burned = 0
    /\ validationStatus = [validator \in Validators |-> "Unknown"]
    /\ validationOuterPayer = [validator \in Validators |-> NoPayer]
    /\ validationContinuationPayer = [validator \in Validators |-> NoPayer]
    /\ preSettlementBalance = InitialBalance
    /\ committed = FALSE
    /\ replayBalance = InitialBalance
    /\ replayBurned = 0
    /\ replayOuterPayer = NoPayer
    /\ replayContinuationPayer = NoPayer

Install ==
    /\ phase = "Install"
    /\ outerCommitted' = ~AllowMissingOuter
    /\ continuationStored' = FALSE
    /\ addressesPublished' = TRUE
    /\ continuationHasCapability' = FALSE
    /\ gatewayHasCapability' = AllowCapabilityLeak
    /\ phase' = IF AllowActivationBeforeFunding THEN "Authorize" ELSE "Fund"
    /\ UNCHANGED <<gatewayAuthenticated, authorizedCaller,
                    unauthorizedAttempted, fundingCommitted, balance, burned,
                    validationStatus, validationOuterPayer,
                    validationContinuationPayer, preSettlementBalance, committed,
                    replayBalance, replayBurned, replayOuterPayer,
                    replayContinuationPayer>>

Fund ==
    /\ phase = "Fund"
    /\ balance' =
         [balance EXCEPT
           ![Sponsor] = IF AllowFundingCopy
                         THEN @
                         ELSE @ - OuterFundingAmount - SlotFundingAmount,
           ![Outer] = @ + OuterFundingAmount,
           ![Slot] = @ + SlotFundingAmount]
    /\ fundingCommitted' = TRUE
    /\ phase' = "Authorize"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressesPublished,
                    continuationHasCapability, gatewayHasCapability, burned,
                    gatewayAuthenticated, authorizedCaller, unauthorizedAttempted,
                    validationStatus, validationOuterPayer,
                    validationContinuationPayer, preSettlementBalance, committed,
                    replayBalance, replayBurned, replayOuterPayer,
                    replayContinuationPayer>>

Authorize(caller) ==
    /\ phase = "Authorize"
    /\ caller \in Callers
    /\ IF caller = Gateway \/ AllowGatewayAuthBypass
       THEN /\ gatewayAuthenticated' = TRUE
            /\ authorizedCaller' = caller
            /\ unauthorizedAttempted' = (unauthorizedAttempted \/ caller = Attacker)
            /\ continuationStored' = TRUE
            /\ continuationHasCapability' = TRUE
            /\ phase' = "Validate"
       ELSE /\ ~unauthorizedAttempted
            /\ gatewayAuthenticated' = FALSE
            /\ authorizedCaller' = NoCaller
            /\ unauthorizedAttempted' = TRUE
            /\ continuationStored' = FALSE
            /\ continuationHasCapability' = FALSE
            /\ phase' = "Authorize"
    /\ UNCHANGED <<outerCommitted, addressesPublished, gatewayHasCapability,
                    fundingCommitted, balance, burned, validationStatus,
                    validationOuterPayer, validationContinuationPayer,
                    preSettlementBalance, committed, replayBalance,
                    replayBurned, replayOuterPayer, replayContinuationPayer>>

Validate(validator) ==
    /\ phase = "Validate"
    /\ validator \in Validators
    /\ validationStatus[validator] = "Unknown"
    /\ validationOuterPayer' =
         [validationOuterPayer EXCEPT ![validator] = CanonicalOuterPayer(validator)]
    /\ validationContinuationPayer' =
         [validationContinuationPayer EXCEPT
            ![validator] = CanonicalContinuationPayer(validator)]
    /\ validationStatus' =
         [validationStatus EXCEPT
           ![validator] =
             IF ValidatorCanAdmit(validator) THEN "Admitted" ELSE "Rejected"]
    /\ UNCHANGED <<phase, outerCommitted, continuationStored,
                    addressesPublished, continuationHasCapability,
                    gatewayHasCapability, gatewayAuthenticated,
                    authorizedCaller, unauthorizedAttempted,
                    fundingCommitted, balance, burned,
                    preSettlementBalance, committed, replayBalance,
                    replayBurned, replayOuterPayer, replayContinuationPayer>>

Commit ==
    /\ phase = "Validate"
    /\ AllValidated
    /\ AllAdmitted
    /\ AllCertifiedForLocatedPurses
    /\ balance[Outer] >= ChargedOuterCost
    /\ balance[Slot] >= ChargedContinuationCost
    /\ balance[Gateway] >= FixedFee
    /\ preSettlementBalance' = balance
    /\ balance' =
         [balance EXCEPT
           ![Outer] = @ - ChargedOuterCost,
           ![Slot] = @ - ChargedContinuationCost,
           ![Gateway] = @ - FixedFee,
           ![Proposer] = @ + FixedFee]
    /\ burned' = burned + ChargedOuterCost + ChargedContinuationCost
    /\ committed' = TRUE
    /\ phase' = "Replay"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressesPublished,
                    continuationHasCapability, gatewayHasCapability,
                    gatewayAuthenticated, authorizedCaller, unauthorizedAttempted,
                    fundingCommitted, validationStatus, validationOuterPayer,
                    validationContinuationPayer, replayBalance, replayBurned,
                    replayOuterPayer, replayContinuationPayer>>

Replay ==
    /\ phase = "Replay"
    /\ IF AllowReplayOmission
       THEN /\ replayBalance' = preSettlementBalance
            /\ replayBurned' = 0
            /\ replayOuterPayer' = NoPayer
            /\ replayContinuationPayer' = NoPayer
       ELSE /\ replayBalance' =
                  [preSettlementBalance EXCEPT
                    ![Outer] = @ - ChargedOuterCost,
                    ![Slot] = @ - ChargedContinuationCost,
                    ![Gateway] = @ - FixedFee,
                    ![Proposer] = @ + FixedFee]
            /\ replayBurned' = ChargedOuterCost + ChargedContinuationCost
            /\ replayOuterPayer' = Outer
            /\ replayContinuationPayer' = Slot
    /\ phase' = "Done"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressesPublished,
                    continuationHasCapability, gatewayHasCapability,
                    gatewayAuthenticated, authorizedCaller, unauthorizedAttempted,
                    fundingCommitted, balance, burned, validationStatus,
                    validationOuterPayer, validationContinuationPayer,
                    preSettlementBalance, committed>>

ValidateAny == \E validator \in Validators : Validate(validator)
AuthorizeAny == \E caller \in Callers : Authorize(caller)
Done == phase = "Done" /\ UNCHANGED vars

Next == Install \/ Fund \/ AuthorizeAny \/ ValidateAny \/ Commit \/ Replay \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Install)
    /\ WF_vars(Fund)
    /\ WF_vars(AuthorizeAny)
    /\ WF_vars(ValidateAny)
    /\ WF_vars(Commit)
    /\ WF_vars(Replay)

TypeOK ==
    /\ phase \in {"Install", "Fund", "Authorize", "Validate", "Replay", "Done"}
    /\ outerCommitted \in BOOLEAN
    /\ continuationStored \in BOOLEAN
    /\ addressesPublished \in BOOLEAN
    /\ continuationHasCapability \in BOOLEAN
    /\ gatewayHasCapability \in BOOLEAN
    /\ gatewayAuthenticated \in BOOLEAN
    /\ authorizedCaller \in Callers \cup {NoCaller}
    /\ unauthorizedAttempted \in BOOLEAN
    /\ fundingCommitted \in BOOLEAN
    /\ balance \in [Payers -> Nat]
    /\ burned \in Nat
    /\ validationStatus \in [Validators -> {"Unknown", "Admitted", "Rejected"}]
    /\ validationOuterPayer \in [Validators -> Payers \cup {NoPayer}]
    /\ validationContinuationPayer \in [Validators -> Payers \cup {NoPayer}]
    /\ preSettlementBalance \in [Payers -> Nat]
    /\ committed \in BOOLEAN
    /\ replayBalance \in [Payers -> Nat]
    /\ replayBurned \in Nat
    /\ replayOuterPayer \in Payers \cup {NoPayer}
    /\ replayContinuationPayer \in Payers \cup {NoPayer}

CanonicalCustodyConserved ==
    CanonicalTotal(balance) + burned = CanonicalTotal(InitialBalance)

PublicAddressesAreNotCapability ==
    Cardinality({OuterAddress, SlotAddress, SlotCapability}) = 3

FundingUsesAddressesWithoutDelegatingDraw ==
    /\ addressesPublished => ~gatewayHasCapability
    /\ (fundingCommitted /\ phase = "Authorize") =>
         /\ ~continuationStored
         /\ ~continuationHasCapability

FundingCommitsBothPursesAtomically ==
    (fundingCommitted /\ ~committed) =>
      /\ balance[Sponsor] =
           InitialBalance[Sponsor] - OuterFundingAmount - SlotFundingAmount
      /\ balance[Outer] = InitialBalance[Outer] + OuterFundingAmount
      /\ balance[Slot] = InitialBalance[Slot] + SlotFundingAmount

ContinuationActivationRequiresFunding == continuationStored => fundingCommitted

ContinuationRequiresOuter == continuationStored => outerCommitted

OnlyGatewayAuthorizesContinuation ==
    gatewayAuthenticated => authorizedCaller = Gateway

UnauthorizedAttemptPreservesPreActivationState ==
    (unauthorizedAttempted /\ ~gatewayAuthenticated) =>
      /\ phase = "Authorize"
      /\ ~continuationStored
      /\ ~continuationHasCapability
      /\ ~committed

CertifiedPayersAreLocatedPurses ==
    \A validator \in Validators :
      validationStatus[validator] # "Unknown" =>
        /\ validationOuterPayer[validator] = Outer
        /\ validationContinuationPayer[validator] = Slot

ValidatorsAgreeOnAdmission ==
    \A left \in Validators :
      \A right \in Validators :
        /\ validationStatus[left] # "Unknown"
        /\ validationStatus[right] # "Unknown"
        => /\ validationStatus[left] = validationStatus[right]
           /\ validationOuterPayer[left] = validationOuterPayer[right]
           /\ validationContinuationPayer[left] = validationContinuationPayer[right]

CommittedContinuationUsesLocatedPurses ==
    committed =>
      /\ gatewayAuthenticated
      /\ authorizedCaller = Gateway
      /\ fundingCommitted
      /\ outerCommitted
      /\ continuationStored
      /\ continuationHasCapability
      /\ AllCertifiedForLocatedPurses

RealizedCostsAndFeeAreSeparated ==
    committed =>
      /\ balance[Sponsor] = preSettlementBalance[Sponsor]
      /\ balance[Outer] + ChargedOuterCost = preSettlementBalance[Outer]
      /\ balance[Slot] + ChargedContinuationCost = preSettlementBalance[Slot]
      /\ balance[Gateway] + FixedFee = preSettlementBalance[Gateway]
      /\ balance[Proposer] = preSettlementBalance[Proposer] + FixedFee
      /\ burned = ChargedOuterCost + ChargedContinuationCost

UnusedCertifiedBoundsAreRefunded ==
    committed =>
      /\ balance[Outer]
           = preSettlementBalance[Outer] - OuterCertifiedCostBound
               + (OuterCertifiedCostBound - OuterRealizedCost)
      /\ balance[Slot]
           = preSettlementBalance[Slot] - CertifiedCostBound
               + (CertifiedCostBound - RealizedCost)

NoProtocolMintDuringFundingOrSettlement ==
    CanonicalTotal(balance) + burned = CanonicalTotal(InitialBalance)

ReplayUsesCanonicalLocatedPayers ==
    phase = "Done" =>
      /\ replayOuterPayer = Outer
      /\ replayContinuationPayer = Slot

ReplayMatchesCommit ==
    phase = "Done" =>
      /\ replayBalance = balance
      /\ replayBurned = burned

EventuallyDone == <>(phase = "Done")

=============================================================================
