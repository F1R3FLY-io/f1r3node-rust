---------------------- MODULE WalletFundedLollipop ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    Payers,
    Validators,
    Names,
    Callers,
    Sponsor,
    Slot,
    Gateway,
    Attacker,
    Proposer,
    DivergentValidator,
    SlotAddress,
    SlotCapability,
    NoPayer,
    NoCaller,
    InitialBalance,
    FundingAmount,
    CertifiedCostBound,
    RealizedCost,
    FixedFee,
    AllowFundingCopy,
    AllowCapabilityLeak,
    AllowPayerCollapse,
    AllowReplayOmission,
    AllowMissingOuter,
    AllowGatewayAuthBypass,
    ChargeCertifiedBound

ASSUME /\ Payers = {Sponsor, Slot, Gateway, Proposer}
       /\ Cardinality(Payers) = 4
       /\ Validators # {}
       /\ Callers = {Gateway, Attacker}
       /\ Gateway # Attacker
       /\ Attacker \notin Payers
       /\ DivergentValidator \in Validators
       /\ SlotAddress \in Names
       /\ SlotCapability \in Names
       /\ SlotAddress # SlotCapability
       /\ NoPayer \notin Payers
       /\ NoCaller \notin Callers
       /\ InitialBalance \in [Payers -> Nat]
       /\ FundingAmount \in Nat \ {0}
       /\ FundingAmount <= InitialBalance[Sponsor]
       /\ CertifiedCostBound \in Nat \ {0}
       /\ RealizedCost \in Nat
       /\ RealizedCost <= CertifiedCostBound
       /\ CertifiedCostBound <= InitialBalance[Slot] + FundingAmount
       /\ FixedFee \in Nat
       /\ FixedFee <= InitialBalance[Gateway]
       /\ \A flag \in {
            AllowFundingCopy,
            AllowCapabilityLeak,
            AllowPayerCollapse,
            AllowReplayOmission,
            AllowMissingOuter,
            AllowGatewayAuthBypass,
            ChargeCertifiedBound
          } : flag \in BOOLEAN

VARIABLES
    phase,
    outerCommitted,
    continuationStored,
    addressPublished,
    continuationHasCapability,
    gatewayHasCapability,
    gatewayAuthenticated,
    authorizedCaller,
    unauthorizedAttempted,
    fundingCommitted,
    balance,
    burned,
    validationStatus,
    validationPayer,
    preSettlementBalance,
    committed,
    replayBalance,
    replayBurned,
    replayPayer

vars == <<phase, outerCommitted, continuationStored, addressPublished,
          continuationHasCapability, gatewayHasCapability, gatewayAuthenticated,
          authorizedCaller, unauthorizedAttempted, fundingCommitted,
          balance, burned, validationStatus, validationPayer,
          preSettlementBalance, committed, replayBalance, replayBurned,
          replayPayer>>

RECURSIVE SumSet(_, _)

SumSet(function, domain) ==
    IF domain = {}
    THEN 0
    ELSE LET element == CHOOSE value \in domain : TRUE
         IN function[element] + SumSet(function, domain \ {element})

CanonicalPayer(validator) ==
    IF AllowPayerCollapse /\ validator = DivergentValidator
    THEN Gateway
    ELSE Slot

ChargedCost ==
    IF ChargeCertifiedBound THEN CertifiedCostBound ELSE RealizedCost

ValidatorCanAdmit(validator) ==
    /\ gatewayAuthenticated
    /\ continuationStored
    /\ continuationHasCapability
    /\ (outerCommitted \/ AllowMissingOuter)
    /\ balance[CanonicalPayer(validator)] >= CertifiedCostBound
    /\ balance[Gateway] >= FixedFee

AllValidated ==
    \A validator \in Validators : validationStatus[validator] # "Unknown"

AllAdmitted ==
    \A validator \in Validators : validationStatus[validator] = "Admitted"

AllCertifiedForSlot ==
    \A validator \in Validators : validationPayer[validator] = Slot

Init ==
    /\ phase = "Install"
    /\ outerCommitted = FALSE
    /\ continuationStored = FALSE
    /\ addressPublished = FALSE
    /\ continuationHasCapability = FALSE
    /\ gatewayHasCapability = FALSE
    /\ gatewayAuthenticated = FALSE
    /\ authorizedCaller = NoCaller
    /\ unauthorizedAttempted = FALSE
    /\ fundingCommitted = FALSE
    /\ balance = InitialBalance
    /\ burned = 0
    /\ validationStatus = [validator \in Validators |-> "Unknown"]
    /\ validationPayer = [validator \in Validators |-> NoPayer]
    /\ preSettlementBalance = InitialBalance
    /\ committed = FALSE
    /\ replayBalance = InitialBalance
    /\ replayBurned = 0
    /\ replayPayer = NoPayer

Install ==
    /\ phase = "Install"
    /\ outerCommitted' = ~AllowMissingOuter
    /\ continuationStored' = TRUE
    /\ addressPublished' = TRUE
    /\ continuationHasCapability' = TRUE
    /\ gatewayHasCapability' = AllowCapabilityLeak
    /\ phase' = "Fund"
    /\ UNCHANGED <<gatewayAuthenticated, authorizedCaller,
                    unauthorizedAttempted, fundingCommitted, balance, burned,
                    validationStatus,
                    validationPayer, preSettlementBalance, committed,
                    replayBalance, replayBurned, replayPayer>>

Fund ==
    /\ phase = "Fund"
    /\ balance' =
         [balance EXCEPT
           ![Sponsor] = IF AllowFundingCopy THEN @ ELSE @ - FundingAmount,
           ![Slot] = @ + FundingAmount]
    /\ fundingCommitted' = TRUE
    /\ phase' = "Authorize"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressPublished,
                    continuationHasCapability, gatewayHasCapability, burned,
                    gatewayAuthenticated, authorizedCaller, unauthorizedAttempted,
                    validationStatus, validationPayer, preSettlementBalance,
                    committed, replayBalance, replayBurned, replayPayer>>

Authorize(caller) ==
    /\ phase = "Authorize"
    /\ caller \in Callers
    /\ IF caller = Gateway \/ AllowGatewayAuthBypass
       THEN /\ gatewayAuthenticated' = TRUE
            /\ authorizedCaller' = caller
            /\ unauthorizedAttempted' = (unauthorizedAttempted \/ caller = Attacker)
            /\ phase' = "Validate"
       ELSE /\ ~unauthorizedAttempted
            /\ gatewayAuthenticated' = FALSE
            /\ authorizedCaller' = NoCaller
            /\ unauthorizedAttempted' = TRUE
            /\ phase' = "Authorize"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressPublished,
                    continuationHasCapability, gatewayHasCapability,
                    fundingCommitted, balance, burned, validationStatus,
                    validationPayer, preSettlementBalance, committed,
                    replayBalance, replayBurned, replayPayer>>

Validate(validator) ==
    /\ phase = "Validate"
    /\ validator \in Validators
    /\ validationStatus[validator] = "Unknown"
    /\ validationPayer' =
         [validationPayer EXCEPT ![validator] = CanonicalPayer(validator)]
    /\ validationStatus' =
         [validationStatus EXCEPT
           ![validator] =
             IF ValidatorCanAdmit(validator) THEN "Admitted" ELSE "Rejected"]
    /\ UNCHANGED <<phase, outerCommitted, continuationStored,
                    addressPublished, continuationHasCapability,
                    gatewayHasCapability, gatewayAuthenticated,
                    authorizedCaller, unauthorizedAttempted,
                    fundingCommitted, balance, burned,
                    preSettlementBalance, committed, replayBalance,
                    replayBurned, replayPayer>>

Commit ==
    /\ phase = "Validate"
    /\ AllValidated
    /\ AllAdmitted
    /\ AllCertifiedForSlot
    /\ balance[Slot] >= ChargedCost
    /\ balance[Gateway] >= FixedFee
    /\ preSettlementBalance' = balance
    /\ balance' =
         [balance EXCEPT
           ![Slot] = @ - ChargedCost,
           ![Gateway] = @ - FixedFee,
           ![Proposer] = @ + FixedFee]
    /\ burned' = burned + ChargedCost
    /\ committed' = TRUE
    /\ phase' = "Replay"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressPublished,
                    continuationHasCapability, gatewayHasCapability,
                    gatewayAuthenticated, authorizedCaller, unauthorizedAttempted,
                    fundingCommitted, validationStatus, validationPayer,
                    replayBalance, replayBurned, replayPayer>>

Replay ==
    /\ phase = "Replay"
    /\ IF AllowReplayOmission
       THEN /\ replayBalance' = preSettlementBalance
            /\ replayBurned' = 0
            /\ replayPayer' = NoPayer
       ELSE /\ replayBalance' =
                  [preSettlementBalance EXCEPT
                    ![Slot] = @ - ChargedCost,
                    ![Gateway] = @ - FixedFee,
                    ![Proposer] = @ + FixedFee]
            /\ replayBurned' = ChargedCost
            /\ replayPayer' = Slot
    /\ phase' = "Done"
    /\ UNCHANGED <<outerCommitted, continuationStored, addressPublished,
                    continuationHasCapability, gatewayHasCapability,
                    gatewayAuthenticated, authorizedCaller, unauthorizedAttempted,
                    fundingCommitted, balance, burned, validationStatus,
                    validationPayer, preSettlementBalance, committed>>

ValidateAny == \E validator \in Validators : Validate(validator)
AuthorizeAny == \E caller \in Callers : Authorize(caller)

Next == Install \/ Fund \/ AuthorizeAny \/ ValidateAny \/ Commit \/ Replay

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
    /\ addressPublished \in BOOLEAN
    /\ continuationHasCapability \in BOOLEAN
    /\ gatewayHasCapability \in BOOLEAN
    /\ gatewayAuthenticated \in BOOLEAN
    /\ authorizedCaller \in Callers \cup {NoCaller}
    /\ unauthorizedAttempted \in BOOLEAN
    /\ fundingCommitted \in BOOLEAN
    /\ balance \in [Payers -> Nat]
    /\ burned \in Nat
    /\ validationStatus \in [Validators -> {"Unknown", "Admitted", "Rejected"}]
    /\ validationPayer \in [Validators -> Payers \cup {NoPayer}]
    /\ preSettlementBalance \in [Payers -> Nat]
    /\ committed \in BOOLEAN
    /\ replayBalance \in [Payers -> Nat]
    /\ replayBurned \in Nat
    /\ replayPayer \in Payers \cup {NoPayer}

CanonicalCustodyConserved ==
    SumSet(balance, Payers) + burned = SumSet(InitialBalance, Payers)

PublicAddressIsNotCapability == SlotAddress # SlotCapability

FundingUsesAddressWithoutDelegatingDraw ==
    fundingCommitted =>
      /\ addressPublished
      /\ ~gatewayHasCapability
      /\ continuationHasCapability

ContinuationRequiresOuter == continuationStored => outerCommitted

OnlyGatewayAuthorizesContinuation ==
    gatewayAuthenticated => authorizedCaller = Gateway

UnauthorizedAttemptPreservesContinuation ==
    (unauthorizedAttempted /\ ~gatewayAuthenticated) =>
      /\ phase = "Authorize"
      /\ continuationStored
      /\ continuationHasCapability
      /\ ~committed

CertifiedPayerIsSlot ==
    \A validator \in Validators :
      validationStatus[validator] # "Unknown" => validationPayer[validator] = Slot

ValidatorsAgreeOnAdmission ==
    \A left \in Validators :
      \A right \in Validators :
        /\ validationStatus[left] # "Unknown"
        /\ validationStatus[right] # "Unknown"
        => /\ validationStatus[left] = validationStatus[right]
           /\ validationPayer[left] = validationPayer[right]

CommittedContinuationUsesSlot ==
    committed =>
      /\ gatewayAuthenticated
      /\ authorizedCaller = Gateway
      /\ outerCommitted
      /\ continuationStored
      /\ continuationHasCapability
      /\ AllCertifiedForSlot

RealizedCostAndFeeAreSeparated ==
    committed =>
      /\ balance[Sponsor] = preSettlementBalance[Sponsor]
      /\ balance[Slot] + ChargedCost = preSettlementBalance[Slot]
      /\ balance[Gateway] + FixedFee = preSettlementBalance[Gateway]
      /\ balance[Proposer] = preSettlementBalance[Proposer] + FixedFee
      /\ burned = ChargedCost

UnusedCertifiedBoundIsRefunded ==
    committed =>
      balance[Slot]
        = preSettlementBalance[Slot] - CertifiedCostBound
            + (CertifiedCostBound - RealizedCost)

NoProtocolMintDuringFundingOrSettlement ==
    SumSet(balance, Payers) + burned = SumSet(InitialBalance, Payers)

ReplayUsesCanonicalSlotPayer == phase = "Done" => replayPayer = Slot

ReplayMatchesCommit ==
    phase = "Done" =>
      /\ replayBalance = balance
      /\ replayBurned = burned

EventuallyDone == <>(phase = "Done")

=============================================================================
