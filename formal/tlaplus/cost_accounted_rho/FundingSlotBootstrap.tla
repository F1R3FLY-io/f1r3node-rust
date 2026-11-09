---------------------- MODULE FundingSlotBootstrap ----------------------
(***************************************************************************)
(* G3 (DR-135, design v2 rules R1 to R5; DR-68 items 1 to 3): the          *)
(* installer's share at activation.                                        *)
(*                                                                         *)
(* The installed trigger is the stored receive of the installer. Its outer *)
(* layer carries the installer's region (uniform signing, DR-68), as in    *)
(* the gateway form {for (...) body}[installer -o slot] of the user's      *)
(* decision of 2026-10-19 (P1 def:funding-slot, def:sugar-lollipop). When  *)
(* the gateway activates the trigger, the installer pays the share of its  *)
(* stored receive as an owner, from its own purse, at its own price:       *)
(*   share = InstallerActivationUsage * InstallerPrice.                    *)
(* The slot pays the body, and the outer purse pays its realized cost, as  *)
(* before. If the installer purse cannot pay the share, the activation     *)
(* does not fire and has no charge, and play records a refusal (Option 2). *)
(*                                                                         *)
(* G3 invariants:                                                          *)
(*   ActivationChargesTheInstallerShare  a fired activation debits the     *)
(*                                       installer share and burns it.     *)
(*   InstallerShareWithinBudget          the share fit the installer purse *)
(*                                       before the activation.            *)
(*   RefusedActivationIsEffectFree       a refused activation changes no   *)
(*                                       balance, instantiates nothing and *)
(*                                       is recorded.                      *)
(* G3 control (Defect):                                                    *)
(*   "IgnoreInstallerShare"  the activation neither gates nor charges the  *)
(*                           installer share -> ActivationChargesThe-      *)
(*                           InstallerShare                                *)
(* The config FundingSlotBootstrapInstallerRefusal.cfg gives a share that  *)
(* the installer cannot pay. Its property EventuallyRefused shows that the *)
(* refusal is reached.                                                     *)
(*                                                                         *)
(* G3 step S11 (DR-135 item 13): the caller reserves only its own work.    *)
(* The caller purse belongs to the deployment whose send activates the     *)
(* trigger. Its signed limit and its hold before execution cover only the  *)
(* client part of its run:                                                 *)
(*   client part = CallerUsage + ZeroPricedOwnerUsage <= CallerLimit.      *)
(* The owner work at price 0 (ZeroPricedOwnerUsage) still counts against   *)
(* the limit of the caller (S11 decision 2). The priced owner work counts  *)
(* only against the budgets of its owners: the installer share and the     *)
(* located body (BodyOwnerUsage), whose cost the outer and slot purses     *)
(* pay. Who pays does not change. The caller pays only CallerUsage *       *)
(* CallerPrice, in every outcome where its run completes, and the owners   *)
(* pay their shares. A caller whose client part exceeds its limit exhausts *)
(* the limit (DR-113): its run rolls back, nothing fires, and it pays its  *)
(* own work up to its limit. The fixed fee is outside this model. The      *)
(* caller purse holds exactly its hold, CallerLimit * CallerPrice          *)
(* (MCFundingSlotBootstrap.tla).                                           *)
(* S11 invariants:                                                         *)
(*   SponsoredActivationNeedsOnlyCallerWork  the limit of the caller       *)
(*                                           stops the run exactly when    *)
(*                                           the client part exceeds it.   *)
(*   CallerPaysOnlyItsOwnWork                the caller purse pays only    *)
(*                                           its own work, at its own      *)
(*                                           price.                        *)
(* S11 controls (Defect):                                                  *)
(*   "CallerLimitCoversWholeRun"  the limit also counts the priced         *)
(*       owner work, as before S11                                         *)
(*       -> SponsoredActivationNeedsOnlyCallerWork                         *)
(*   "CallerPaysOwnerShares"  the caller also pays the priced owner        *)
(*       work at its own price -> CallerPaysOnlyItsOwnWork                 *)
(* In FundingSlotBootstrap.cfg the whole run (5 units) exceeds the limit   *)
(* of the caller (2), and EventuallyDone shows that the sponsored          *)
(* activation still completes.                                             *)
(***************************************************************************)
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
    Defect,
    \* Added by G3 (DR-135): the usage of the installer's stored receive.
    \* @type: Int;
    InstallerActivationUsage,
    \* Added by G3 (DR-135): the installer's own signed price.
    \* @type: Int;
    InstallerPrice,
    \* Added by G3 step S11 (DR-135 item 13): the caller purse.
    \* @type: Str;
    Caller,
    \* Added by G3 step S11: the caller's own work, its client regions.
    \* @type: Int;
    CallerUsage,
    \* Added by G3 step S11: the caller's own signed price.
    \* @type: Int;
    CallerPrice,
    \* Added by G3 step S11: the caller's signed limit.
    \* @type: Int;
    CallerLimit,
    \* Added by G3 step S11: the owner work at price 0 in the caller's run.
    \* @type: Int;
    ZeroPricedOwnerUsage,
    \* Added by G3 step S11: the priced owner work of the located body.
    \* @type: Int;
    BodyOwnerUsage

\* Added by G3 (DR-135). G3 step S11 adds the controls
\* "CallerLimitCoversWholeRun" and "CallerPaysOwnerShares".
ASSUME /\ InstallerActivationUsage \in Nat
       /\ InstallerPrice \in Nat \ {0}
       /\ Defect \in {
            "None",
            "EagerLocatedInstall",
            "CandidateSelfFunding",
            "SlotOnlyFunding",
            "PartialFundingCommit",
            "RejectedTargetCreation",
            "IgnoreInstallerShare",
            "CallerLimitCoversWholeRun",
            "CallerPaysOwnerShares"
          }

\* Added by G3 step S11 (DR-135 item 13): the caller's run. The gate admits
\* the caller only when its purse covers its hold, CallerLimit * CallerPrice
\* (the hold formula does not change).
ASSUME /\ CallerUsage \in Nat \ {0}
       /\ CallerPrice \in Nat \ {0}
       /\ CallerLimit \in Nat \ {0}
       /\ ZeroPricedOwnerUsage \in Nat
       /\ BodyOwnerUsage \in Nat
       /\ InitialBalance[Caller] >= CallerLimit * CallerPrice

\* Changed by G3 (DR-135): the Defect values move to the ASSUME above, which
\* adds "IgnoreInstallerShare". G3 step S11 adds the caller purse to Purses.
\* ASSUME /\ Purses = {Installer, Sponsor, Outer, Slot}
\*        /\ Cardinality(Purses) = 4
\*        /\ InitialBalance \in [Purses -> Nat]
\*        /\ InitialBalance[Installer] >= InstallScaffoldCost
\*        /\ InitialBalance[Sponsor] >= OuterFundingAmount + SlotFundingAmount
\*        /\ InitialBalance[Outer] = 0
\*        /\ InitialBalance[Slot] = 0
\*        /\ InstallScaffoldCost \in Nat \ {0}
\*        /\ OuterInstallBound \in Nat \ {0}
\*        /\ SlotInstallBound \in Nat \ {0}
\*        /\ OuterFundingAmount \in Nat \ {0}
\*        /\ SlotFundingAmount \in Nat \ {0}
\*        /\ OuterActivationBound \in Nat \ {0}
\*        /\ SlotActivationBound \in Nat \ {0}
\*        /\ OuterRealizedCost \in Nat
\*        /\ SlotRealizedCost \in Nat
\*        /\ OuterRealizedCost <= OuterActivationBound
\*        /\ SlotRealizedCost <= SlotActivationBound
\*        /\ OuterFundingAmount >= OuterActivationBound
\*        /\ SlotFundingAmount >= SlotActivationBound
\*        /\ Defect \in {
\*             "None",
\*             "EagerLocatedInstall",
\*             "CandidateSelfFunding",
\*             "SlotOnlyFunding",
\*             "PartialFundingCommit",
\*             "RejectedTargetCreation"
\*           }
ASSUME /\ Purses = {Installer, Sponsor, Outer, Slot, Caller}
       /\ Cardinality(Purses) = 5
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
    preActivationBalance,
    \* Added by G3 (DR-135): play recorded a refusal of the activation.
    \* @type: Bool;
    refusalRecorded

\* Changed by G3 (DR-135): the refusal record.
\* vars == <<
\*     phase,
\*     balance,
\*     vaultExists,
\*     burned,
\*     addressesPublished,
\*     gatewayHandlerStored,
\*     locatedContinuationInstantiated,
\*     fundingCommitted,
\*     gatewayAuthenticated,
\*     candidateSupplyUsed,
\*     postInstallBalance,
\*     postInstallVaultExists,
\*     preActivationBalance
\* >>
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
    preActivationBalance,
    refusalRecorded
>>

\* Changed by G3 step S11 (DR-135 item 13): the caller purse.
\* Total(function) ==
\*     function[Installer] + function[Sponsor] + function[Outer] + function[Slot]
Total(function) ==
    function[Installer] + function[Sponsor] + function[Outer] + function[Slot]
      + function[Caller]

\* Added by G3 (DR-135): the installer's owner share of the activation COMM,
\* at the installer's own price.
InstallerShare == InstallerActivationUsage * InstallerPrice

\* Added by G3 step S11 (DR-135 item 13): the caller's run.
\* The priced owner work: the installer share and the located body. It
\* counts only against the budgets of its owners.
PricedOwnerUsage == InstallerActivationUsage + BodyOwnerUsage

\* The client part: the caller's own work and the owner work at price 0
\* (S11 decision 2). The signed limit and the hold cover only this part.
CallerClientUsage == CallerUsage + ZeroPricedOwnerUsage

\* The work that the meter counts against the caller's limit. Control
\* "CallerLimitCoversWholeRun" also counts the priced owner work, as the
\* rule before S11 did.
CallerBoundUsage ==
    IF Defect = "CallerLimitCoversWholeRun"
    THEN CallerClientUsage + PricedOwnerUsage
    ELSE CallerClientUsage

\* The charge of a caller whose run completes: its own work at its own
\* price. The owner work at price 0 costs 0 for every party. Control
\* "CallerPaysOwnerShares" also bills the priced owner work of a fired
\* activation to the caller, at the caller's price.
CallerCharge(fired) ==
    IF Defect = "CallerPaysOwnerShares" /\ fired
    THEN (CallerUsage + PricedOwnerUsage) * CallerPrice
    ELSE CallerUsage * CallerPrice

\* The charge of a caller that exhausts its limit (DR-113): its own work up
\* to its limit, the most fresh work that it can have used.
CallerExhaustedCharge ==
    (IF CallerUsage <= CallerLimit THEN CallerUsage ELSE CallerLimit)
      * CallerPrice

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
    \* Added by G3 (DR-135).
    /\ refusalRecorded = FALSE

Install ==
    /\ phase = "Install"
    /\ IF Defect = "EagerLocatedInstall" /\ InstallNeedsCandidateSupply
       THEN /\ phase' = "InstallRejected"
            \* Changed by G3 (DR-135): the refusal record is unchanged.
            \* /\ UNCHANGED <<balance, vaultExists, burned, addressesPublished,
            \*                 gatewayHandlerStored, locatedContinuationInstantiated,
            \*                 fundingCommitted, gatewayAuthenticated,
            \*                 candidateSupplyUsed, postInstallBalance,
            \*                 postInstallVaultExists,
            \*                 preActivationBalance>>
            /\ UNCHANGED <<balance, vaultExists, burned, addressesPublished,
                            gatewayHandlerStored, locatedContinuationInstantiated,
                            fundingCommitted, gatewayAuthenticated,
                            candidateSupplyUsed, postInstallBalance,
                            postInstallVaultExists,
                            preActivationBalance, refusalRecorded>>
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
            \* Changed by G3 (DR-135): the refusal record is unchanged.
            \* /\ UNCHANGED <<fundingCommitted, gatewayAuthenticated,
            \*                 preActivationBalance>>
            /\ UNCHANGED <<fundingCommitted, gatewayAuthenticated,
                            preActivationBalance, refusalRecorded>>

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
    \* Changed by G3 (DR-135): the refusal record is unchanged.
    \* /\ UNCHANGED <<burned, addressesPublished, gatewayHandlerStored,
    \*                 locatedContinuationInstantiated, gatewayAuthenticated,
    \*                 candidateSupplyUsed, postInstallBalance,
    \*                 postInstallVaultExists,
    \*                 preActivationBalance>>
    /\ UNCHANGED <<burned, addressesPublished, gatewayHandlerStored,
                    locatedContinuationInstantiated, gatewayAuthenticated,
                    candidateSupplyUsed, postInstallBalance,
                    postInstallVaultExists,
                    preActivationBalance, refusalRecorded>>

\* Changed by G3 (DR-135): the installer's owner share. A located activation
\* fires only when the installer purse also pays the share. When it cannot,
\* the activation does not fire, has no charge, and play records a refusal
\* (Option 2). Control "IgnoreInstallerShare" neither gates nor charges the
\* share.
\* G3 step S11 (DR-135 item 13): the activation is the run of the caller.
\* The limit of the caller counts only the client part (CallerBoundUsage).
\* A caller whose client part exceeds its limit exhausts it (DR-113): the
\* run rolls back, nothing fires, and the caller pays CallerExhaustedCharge.
\* Otherwise the caller pays its own work in every outcome, because its send
\* runs also when the activation is refused or rejected.
\* Activate ==
\*     /\ phase = "Activate"
\*     /\ gatewayAuthenticated' = TRUE
\*     /\ preActivationBalance' = balance
\*     /\ IF /\ addressesPublished
\*            /\ gatewayHandlerStored
\*            /\ fundingCommitted
\*            /\ balance[Outer] >= OuterActivationBound
\*            /\ balance[Slot] >= SlotActivationBound
\*        THEN /\ balance' =
\*                   [balance EXCEPT
\*                     ![Outer] = @ - OuterRealizedCost,
\*                     ![Slot] = @ - SlotRealizedCost]
\*             /\ burned' = burned + OuterRealizedCost + SlotRealizedCost
\*             /\ locatedContinuationInstantiated' = TRUE
\*             /\ phase' = "Done"
\*        ELSE /\ phase' = "ActivationRejected"
\*             /\ UNCHANGED <<balance, burned,
\*                             locatedContinuationInstantiated>>
\*     /\ UNCHANGED <<vaultExists, addressesPublished, gatewayHandlerStored,
\*                     fundingCommitted, candidateSupplyUsed,
\*                     postInstallBalance, postInstallVaultExists>>
LocatedActivationAdmissible ==
    /\ addressesPublished
    /\ gatewayHandlerStored
    /\ fundingCommitted
    /\ balance[Outer] >= OuterActivationBound
    /\ balance[Slot] >= SlotActivationBound

\* The share that the activation charges to the installer purse.
ChargedInstallerShare == IF Defect = "IgnoreInstallerShare" THEN 0 ELSE InstallerShare

Activate ==
    /\ phase = "Activate"
    /\ gatewayAuthenticated' = TRUE
    /\ preActivationBalance' = balance
    /\ IF CallerBoundUsage > CallerLimit
       THEN /\ phase' = "CallerLimitExhausted"
            /\ balance' =
                  [balance EXCEPT ![Caller] = @ - CallerExhaustedCharge]
            /\ burned' = burned + CallerExhaustedCharge
            /\ refusalRecorded' = FALSE
            /\ UNCHANGED locatedContinuationInstantiated
       ELSE IF LocatedActivationAdmissible
          /\ (Defect = "IgnoreInstallerShare" \/ balance[Installer] >= InstallerShare)
       THEN /\ balance' =
                  [balance EXCEPT
                    ![Outer] = @ - OuterRealizedCost,
                    ![Slot] = @ - SlotRealizedCost,
                    ![Installer] = @ - ChargedInstallerShare,
                    ![Caller] = @ - CallerCharge(TRUE)]
            /\ burned' =
                 burned + OuterRealizedCost + SlotRealizedCost + ChargedInstallerShare
                   + CallerCharge(TRUE)
            /\ locatedContinuationInstantiated' = TRUE
            /\ refusalRecorded' = FALSE
            /\ phase' = "Done"
       ELSE IF LocatedActivationAdmissible
       THEN /\ phase' = "ActivationRefused"
            /\ refusalRecorded' = TRUE
            /\ balance' = [balance EXCEPT ![Caller] = @ - CallerCharge(FALSE)]
            /\ burned' = burned + CallerCharge(FALSE)
            /\ UNCHANGED locatedContinuationInstantiated
       ELSE /\ phase' = "ActivationRejected"
            /\ refusalRecorded' = FALSE
            /\ balance' = [balance EXCEPT ![Caller] = @ - CallerCharge(FALSE)]
            /\ burned' = burned + CallerCharge(FALSE)
            /\ UNCHANGED locatedContinuationInstantiated
    /\ UNCHANGED <<vaultExists, addressesPublished, gatewayHandlerStored,
                    fundingCommitted, candidateSupplyUsed,
                    postInstallBalance, postInstallVaultExists>>

\* Changed by G3 (DR-135): a refused activation is terminal in this model.
\* G3 step S11 adds the exhausted caller.
\* Terminal ==
\*     /\ phase \in {
\*          "Done", "InstallRejected", "FundingRejected", "ActivationRejected"
\*        }
\*     /\ UNCHANGED vars
Terminal ==
    /\ phase \in {
         "Done", "InstallRejected", "FundingRejected", "ActivationRejected",
         "ActivationRefused", "CallerLimitExhausted"
       }
    /\ UNCHANGED vars

Next == Install \/ Fund \/ Activate \/ Terminal

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Install)
    /\ WF_vars(Fund)
    /\ WF_vars(Activate)

\* Changed by G3 (DR-135): the refused phase and the refusal record. G3 step
\* S11 adds the exhausted caller.
\* TypeOK ==
\*     /\ phase \in {
\*          "Install", "Fund", "Activate", "Done", "InstallRejected",
\*          "FundingRejected", "ActivationRejected"
\*        }
\*     /\ balance \in [Purses -> Nat]
TypeOK ==
    /\ phase \in {
         "Install", "Fund", "Activate", "Done", "InstallRejected",
         "FundingRejected", "ActivationRejected", "ActivationRefused",
         "CallerLimitExhausted"
       }
    /\ refusalRecorded \in BOOLEAN
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

\* Changed by G3 (DR-135): the activation also burns the installer share,
\* and (G3 step S11) the caller's own work.
\* SettlementUsesDistinctLocatedPurses ==
\*     phase = "Done" =>
\*       /\ balance[Outer] + OuterRealizedCost = preActivationBalance[Outer]
\*       /\ balance[Slot] + SlotRealizedCost = preActivationBalance[Slot]
\*       /\ burned =
\*            InstallScaffoldCost + OuterRealizedCost + SlotRealizedCost
SettlementUsesDistinctLocatedPurses ==
    phase = "Done" =>
      /\ balance[Outer] + OuterRealizedCost = preActivationBalance[Outer]
      /\ balance[Slot] + SlotRealizedCost = preActivationBalance[Slot]
      /\ burned =
           InstallScaffoldCost + OuterRealizedCost + SlotRealizedCost
             + InstallerShare + CallerUsage * CallerPrice

\* Added by G3 (DR-135): a fired activation debits the installer share from
\* the installer purse and burns it. The installer's stored receive is never
\* free for its owner.
ActivationChargesTheInstallerShare ==
    phase = "Done" =>
      /\ balance[Installer] + InstallerShare = preActivationBalance[Installer]
      /\ balance[Installer] + InstallScaffoldCost + InstallerShare
           = InitialBalance[Installer]

\* Added by G3 (DR-135): the share fit the installer purse before the
\* activation.
InstallerShareWithinBudget ==
    phase = "Done" => InstallerShare <= preActivationBalance[Installer]

\* Added by G3 (DR-135): a refused activation changes no balance, burns
\* nothing, instantiates no continuation, and is recorded. G3 step S11: the
\* caller still pays its own work, because its send runs.
RefusedActivationIsEffectFree ==
    phase = "ActivationRefused" =>
      /\ \A purse \in Purses \ {Caller} :
           balance[purse] = preActivationBalance[purse]
      /\ balance[Caller] + CallerUsage * CallerPrice
           = preActivationBalance[Caller]
      /\ burned = InstallScaffoldCost + CallerUsage * CallerPrice
      /\ ~locatedContinuationInstantiated
      /\ refusalRecorded
      /\ preActivationBalance[Installer] < InstallerShare

\* Added by G3 step S11 (DR-135 item 13): the limit of the caller stops the
\* run exactly when the client part exceeds the limit. So a sponsored
\* activation needs only the caller's own work, with the owner work at
\* price 0, within the limit. The priced owner work never counts against it.
SponsoredActivationNeedsOnlyCallerWork ==
    /\ phase = "Done" => CallerClientUsage <= CallerLimit
    /\ phase = "CallerLimitExhausted" => CallerLimit < CallerClientUsage

\* Added by G3 step S11 (DR-135 item 13): the caller purse pays only the
\* caller's own work, at the caller's own price, and never more than its
\* hold. Who pays does not change: the owners pay their shares
\* (ActivationChargesTheInstallerShare, SettlementUsesDistinctLocatedPurses).
CallerPaysOnlyItsOwnWork ==
    /\ phase \in {"Done", "ActivationRefused", "ActivationRejected"} =>
         balance[Caller] + CallerUsage * CallerPrice = InitialBalance[Caller]
    /\ phase = "CallerLimitExhausted" =>
         balance[Caller] + CallerExhaustedCharge = InitialBalance[Caller]
    /\ InitialBalance[Caller] - balance[Caller] <= CallerLimit * CallerPrice

EventuallyDone == <>(phase = "Done")

\* Added by G3 (DR-135): with a share that the installer cannot pay, the
\* activation ends refused.
EventuallyRefused == <>(phase = "ActivationRefused")

=============================================================================
