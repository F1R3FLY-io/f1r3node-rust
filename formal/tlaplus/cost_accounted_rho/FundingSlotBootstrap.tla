---------------------- MODULE FundingSlotBootstrap ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    \* @type: Set(Str);
    Purses,
    \* @type: Str;
    Installer,
    \* @type: Str;
    Sponsor,
    \* @type: Str;
    Outer,
    \* @type: Str;
    Slot,
    \* @type: Str -> Int;
    InitialBalance,
    \* @type: Int;
    InstallScaffoldCost,
    \* @type: Int;
    OuterInstallBound,
    \* @type: Int;
    SlotInstallBound,
    \* @type: Int;
    OuterFundingAmount,
    \* @type: Int;
    SlotFundingAmount,
    \* @type: Int;
    OuterActivationBound,
    \* @type: Int;
    SlotActivationBound,
    \* @type: Int;
    OuterRealizedCost,
    \* @type: Int;
    SlotRealizedCost,
    \* @type: Str;
    Defect

ASSUME /\ Purses = {Installer, Sponsor, Outer, Slot}
       /\ Cardinality(Purses) = 4
       /\ InitialBalance \in [Purses -> Nat]
       /\ InitialBalance[Installer] >= InstallScaffoldCost
       /\ InitialBalance[Sponsor] >= OuterFundingAmount + SlotFundingAmount
       /\ InitialBalance[Outer] = 0
       /\ InitialBalance[Slot] = 0
       /\ InstallScaffoldCost \in Nat \ {0}
       /\ OuterInstallBound \in Nat \ {0}
       /\ SlotInstallBound \in Nat \ {0}
       /\ OuterFundingAmount \in Nat \ {0}
       /\ SlotFundingAmount \in Nat \ {0}
       /\ OuterActivationBound \in Nat \ {0}
       /\ SlotActivationBound \in Nat \ {0}
       /\ OuterRealizedCost \in Nat
       /\ SlotRealizedCost \in Nat
       /\ OuterRealizedCost <= OuterActivationBound
       /\ SlotRealizedCost <= SlotActivationBound
       /\ OuterFundingAmount >= OuterActivationBound
       /\ SlotFundingAmount >= SlotActivationBound
       /\ Defect \in {
            "None",
            "EagerLocatedInstall",
            "CandidateSelfFunding",
            "SlotOnlyFunding",
            "PartialFundingCommit",
            "RejectedTargetCreation"
          }

VARIABLES
    \* @type: Str;
    phase,
    \* @type: Str -> Int;
    balance,
    \* @type: Set(Str);
    vaultExists,
    \* @type: Int;
    burned,
    \* @type: Bool;
    addressesPublished,
    \* @type: Bool;
    gatewayHandlerStored,
    \* @type: Bool;
    locatedContinuationInstantiated,
    \* @type: Bool;
    fundingCommitted,
    \* @type: Bool;
    gatewayAuthenticated,
    \* @type: Bool;
    candidateSupplyUsed,
    \* @type: Str -> Int;
    postInstallBalance,
    \* @type: Set(Str);
    postInstallVaultExists,
    \* @type: Str -> Int;
    preActivationBalance

vars == <<
    phase,
    balance,
    vaultExists,
    burned,
    addressesPublished,
    gatewayHandlerStored,
    locatedContinuationInstantiated,
    fundingCommitted,
    gatewayAuthenticated,
    candidateSupplyUsed,
    postInstallBalance,
    postInstallVaultExists,
    preActivationBalance
>>

Total(function) ==
    function[Installer] + function[Sponsor] + function[Outer] + function[Slot]

InstallNeedsCandidateSupply ==
    OuterInstallBound > InitialBalance[Outer]
      \/ SlotInstallBound > InitialBalance[Slot]

Init ==
    /\ phase = "Install"
    /\ balance = InitialBalance
    /\ vaultExists = {Installer, Sponsor}
    /\ burned = 0
    /\ addressesPublished = FALSE
    /\ gatewayHandlerStored = FALSE
    /\ locatedContinuationInstantiated = FALSE
    /\ fundingCommitted = FALSE
    /\ gatewayAuthenticated = FALSE
    /\ candidateSupplyUsed = FALSE
    /\ postInstallBalance = InitialBalance
    /\ postInstallVaultExists = {Installer, Sponsor}
    /\ preActivationBalance = InitialBalance

Install ==
    /\ phase = "Install"
    /\ IF Defect = "EagerLocatedInstall" /\ InstallNeedsCandidateSupply
       THEN /\ phase' = "InstallRejected"
            /\ UNCHANGED <<balance, vaultExists, burned, addressesPublished,
                            gatewayHandlerStored, locatedContinuationInstantiated,
                            fundingCommitted, gatewayAuthenticated,
                            candidateSupplyUsed, postInstallBalance,
                            postInstallVaultExists,
                            preActivationBalance>>
       ELSE /\ balance' =
                  [balance EXCEPT ![Installer] = @ - InstallScaffoldCost]
            /\ burned' = burned + InstallScaffoldCost
            /\ vaultExists' = vaultExists
            /\ addressesPublished' = TRUE
            /\ gatewayHandlerStored' = TRUE
            /\ locatedContinuationInstantiated' =
                 (Defect = "CandidateSelfFunding")
            /\ candidateSupplyUsed' =
                 (Defect = "CandidateSelfFunding" /\ InstallNeedsCandidateSupply)
            /\ postInstallBalance' = balance'
            /\ postInstallVaultExists' = vaultExists'
            /\ phase' = "Fund"
            /\ UNCHANGED <<fundingCommitted, gatewayAuthenticated,
                            preActivationBalance>>

Fund ==
    /\ phase = "Fund"
    /\ IF Defect = "RejectedTargetCreation"
       THEN /\ balance' = balance
            /\ vaultExists' = vaultExists \cup {Outer}
            /\ fundingCommitted' = FALSE
            /\ phase' = "FundingRejected"
       ELSE IF Defect = "PartialFundingCommit"
       THEN /\ balance' =
                  [balance EXCEPT
                    ![Sponsor] = @ - OuterFundingAmount,
                    ![Outer] = @ + OuterFundingAmount]
            /\ vaultExists' = vaultExists \cup {Outer}
            /\ fundingCommitted' = FALSE
            /\ phase' = "FundingRejected"
       ELSE IF Defect = "SlotOnlyFunding"
       THEN /\ balance' =
                  [balance EXCEPT
                    ![Sponsor] = @ - SlotFundingAmount,
                    ![Slot] = @ + SlotFundingAmount]
            /\ vaultExists' = vaultExists \cup {Slot}
            /\ fundingCommitted' = TRUE
            /\ phase' = "Activate"
       ELSE /\ balance' =
                  [balance EXCEPT
                    ![Sponsor] =
                       @ - OuterFundingAmount - SlotFundingAmount,
                    ![Outer] = @ + OuterFundingAmount,
                    ![Slot] = @ + SlotFundingAmount]
            /\ vaultExists' = vaultExists \cup {Outer, Slot}
            /\ fundingCommitted' = TRUE
            /\ phase' = "Activate"
    /\ UNCHANGED <<burned, addressesPublished, gatewayHandlerStored,
                    locatedContinuationInstantiated, gatewayAuthenticated,
                    candidateSupplyUsed, postInstallBalance,
                    postInstallVaultExists,
                    preActivationBalance>>

Activate ==
    /\ phase = "Activate"
    /\ gatewayAuthenticated' = TRUE
    /\ preActivationBalance' = balance
    /\ IF /\ addressesPublished
           /\ gatewayHandlerStored
           /\ fundingCommitted
           /\ balance[Outer] >= OuterActivationBound
           /\ balance[Slot] >= SlotActivationBound
       THEN /\ balance' =
                  [balance EXCEPT
                    ![Outer] = @ - OuterRealizedCost,
                    ![Slot] = @ - SlotRealizedCost]
            /\ burned' = burned + OuterRealizedCost + SlotRealizedCost
            /\ locatedContinuationInstantiated' = TRUE
            /\ phase' = "Done"
       ELSE /\ phase' = "ActivationRejected"
            /\ UNCHANGED <<balance, burned,
                            locatedContinuationInstantiated>>
    /\ UNCHANGED <<vaultExists, addressesPublished, gatewayHandlerStored,
                    fundingCommitted, candidateSupplyUsed,
                    postInstallBalance, postInstallVaultExists>>

Terminal ==
    /\ phase \in {
         "Done", "InstallRejected", "FundingRejected", "ActivationRejected"
       }
    /\ UNCHANGED vars

Next == Install \/ Fund \/ Activate \/ Terminal

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Install)
    /\ WF_vars(Fund)
    /\ WF_vars(Activate)

TypeOK ==
    /\ phase \in {
         "Install", "Fund", "Activate", "Done", "InstallRejected",
         "FundingRejected", "ActivationRejected"
       }
    /\ balance \in [Purses -> Nat]
    /\ vaultExists \in SUBSET Purses
    /\ burned \in Nat
    /\ addressesPublished \in BOOLEAN
    /\ gatewayHandlerStored \in BOOLEAN
    /\ locatedContinuationInstantiated \in BOOLEAN
    /\ fundingCommitted \in BOOLEAN
    /\ gatewayAuthenticated \in BOOLEAN
    /\ candidateSupplyUsed \in BOOLEAN
    /\ postInstallBalance \in [Purses -> Nat]
    /\ postInstallVaultExists \in SUBSET Purses
    /\ preActivationBalance \in [Purses -> Nat]

CustodyConserved ==
    Total(balance) + burned = Total(InitialBalance)

InstallWorkflowIsAdmissible ==
    phase # "InstallRejected"

RejectedInstallIsEffectFree ==
    phase = "InstallRejected" =>
      /\ balance = InitialBalance
      /\ vaultExists = {Installer, Sponsor}
      /\ burned = 0
      /\ ~addressesPublished
      /\ ~gatewayHandlerStored
      /\ ~locatedContinuationInstantiated

InstallPublishesOnlyTheScaffold ==
    phase = "Fund" =>
      /\ addressesPublished
      /\ gatewayHandlerStored
      /\ ~locatedContinuationInstantiated
      /\ vaultExists = postInstallVaultExists
      /\ balance[Installer] + InstallScaffoldCost = InitialBalance[Installer]
      /\ balance[Sponsor] <= InitialBalance[Sponsor]

CandidateCreatedSupplyNeverFundsItsCreator ==
    ~candidateSupplyUsed

LocatedContinuationRequiresPriorFunding ==
    locatedContinuationInstantiated => fundingCommitted

FundingCommitCoversEveryLocatedPurse ==
    (fundingCommitted /\ phase = "Activate") =>
      /\ Outer \in vaultExists
      /\ Slot \in vaultExists
      /\ balance[Outer] >= OuterActivationBound
      /\ balance[Slot] >= SlotActivationBound

RejectedFundingIsEffectFree ==
    phase = "FundingRejected" =>
      /\ balance = postInstallBalance
      /\ vaultExists = postInstallVaultExists

ActivationRequiresAuthenticatedLocalSufficiency ==
    phase = "Done" =>
      /\ gatewayAuthenticated
      /\ preActivationBalance[Outer] >= OuterActivationBound
      /\ preActivationBalance[Slot] >= SlotActivationBound

SettlementUsesDistinctLocatedPurses ==
    phase = "Done" =>
      /\ balance[Outer] + OuterRealizedCost = preActivationBalance[Outer]
      /\ balance[Slot] + SlotRealizedCost = preActivationBalance[Slot]
      /\ burned =
           InstallScaffoldCost + OuterRealizedCost + SlotRealizedCost

EventuallyDone == <>(phase = "Done")

=============================================================================
