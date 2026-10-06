------------------------ MODULE MintedEpochFrontier -------------------------
EXTENDS Integers, Naturals, FiniteSets, TLC, Apalache

CONSTANTS
    \* @type: Str;
    Defect,
    \* @type: Int;
    MintAmount

ASSUME
    /\ Defect \in {
         "None",
         "InitAtZero",
         "RejectEpochOneBootstrap",
         "AllowGap",
         "DoubleSibling",
         "DropAllSibling",
         "CatchupBond",
         "ClearOnRedemption",
         "RetroactiveMint"
       }
    /\ MintAmount \in Nat

Validators == {"v1", "v2"}
Epochs == 0..3
EpochPairs == Validators \X Epochs
Actions == {
    "Init", "NewClose", "DuplicateClose", "GapClose", "SiblingClose",
    "RejectedBootstrap", "Bond", "Slash", "Redeem", "Restart", "Noop"
}

VARIABLES
    \* @type: Int;
    frontier,
    \* @type: Int;
    persistedFrontier,
    \* @type: Int;
    maxFrontierSeen,
    \* @type: Str -> Int;
    balance,
    \* @type: Set(Str);
    active,
    \* @type: Set(Str);
    halted,
    \* @type: Set(<<Str, Int>>);
    receipts,
    \* @type: Bool;
    started,
    \* @type: Str;
    lastAction,
    \* @type: Int;
    lastEpoch,
    \* @type: Int;
    lastFrontierBefore,
    \* @type: Int;
    lastFrontierAfter,
    \* @type: Int;
    lastSupplyBefore,
    \* @type: Int;
    lastSupplyAfter,
    \* @type: Int;
    lastEligibleCount

vars ==
    <<frontier, persistedFrontier, maxFrontierSeen, balance, active, halted,
      receipts, started, lastAction, lastEpoch, lastFrontierBefore,
      lastFrontierAfter, lastSupplyBefore, lastSupplyAfter, lastEligibleCount>>

\* @type: (Str -> Int) => Int;
SumBalance(candidate) ==
    LET Add(total, validator) == total + candidate[validator]
    IN ApaFoldSet(Add, 0, Validators)

Eligible == active \ halted

\* @type: Int => Set(<<Str, Int>>);
ReceiptSet(epoch) == {<<validator, epoch>> : validator \in Eligible}

\* @type: Int => (Str -> Int);
CreditBalance(multiplier) ==
    [validator \in Validators |->
        balance[validator]
          + IF validator \in Eligible THEN multiplier * MintAmount ELSE 0]

\* @type: (Int, Int) => Bool;
AllowedFrom(candidateFrontier, epoch) ==
    \/ /\ candidateFrontier = -1
       /\ epoch \in {0, 1}
    \/ /\ candidateFrontier >= 0
       /\ epoch = candidateFrontier + 1

Init ==
    /\ frontier = IF Defect = "InitAtZero" THEN 0 ELSE -1
    /\ persistedFrontier = frontier
    /\ maxFrontierSeen = frontier
    /\ balance = [validator \in Validators |-> 0]
    /\ active = Validators
    /\ halted = {}
    /\ receipts = {}
    /\ started = FALSE
    /\ lastAction = "Init"
    /\ lastEpoch = -1
    /\ lastFrontierBefore = frontier
    /\ lastFrontierAfter = frontier
    /\ lastSupplyBefore = 0
    /\ lastSupplyAfter = 0
    /\ lastEligibleCount = 0

NewClose(epoch) ==
    /\ epoch \in Epochs
    /\ AllowedFrom(frontier, epoch)
    /\ ~(Defect = "RejectEpochOneBootstrap" /\ frontier = -1 /\ epoch = 1)
    /\ LET nextBalance == CreditBalance(1)
           nextReceipts == receipts \cup ReceiptSet(epoch)
       IN /\ balance' = nextBalance
          /\ receipts' = nextReceipts
          /\ lastSupplyAfter' = SumBalance(nextBalance)
    /\ frontier' = epoch
    /\ persistedFrontier' = epoch
    /\ maxFrontierSeen' = IF epoch > maxFrontierSeen THEN epoch ELSE maxFrontierSeen
    /\ started' = TRUE
    /\ lastAction' = "NewClose"
    /\ lastEpoch' = epoch
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = epoch
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<active, halted>>

DuplicateClose(epoch) ==
    /\ epoch \in Epochs
    /\ frontier >= 0
    /\ epoch <= frontier
    /\ started' = TRUE
    /\ lastAction' = "DuplicateClose"
    /\ lastEpoch' = epoch
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastSupplyAfter' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<frontier, persistedFrontier, maxFrontierSeen, balance,
                    active, halted, receipts>>

GapClose(epoch) ==
    /\ epoch \in Epochs
    /\ epoch > frontier
    /\ ~AllowedFrom(frontier, epoch)
    /\ LET accepts == Defect = "AllowGap"
           nextBalance == IF accepts THEN CreditBalance(1) ELSE balance
           nextFrontier == IF accepts THEN epoch ELSE frontier
           nextReceipts == IF accepts THEN receipts \cup ReceiptSet(epoch) ELSE receipts
       IN /\ balance' = nextBalance
          /\ receipts' = nextReceipts
          /\ frontier' = nextFrontier
          /\ persistedFrontier' = nextFrontier
          /\ maxFrontierSeen' =
               IF nextFrontier > maxFrontierSeen THEN nextFrontier ELSE maxFrontierSeen
          /\ lastFrontierAfter' = nextFrontier
          /\ lastSupplyAfter' = SumBalance(nextBalance)
    /\ started' = TRUE
    /\ lastAction' = "GapClose"
    /\ lastEpoch' = epoch
    /\ lastFrontierBefore' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<active, halted>>

SiblingClose(epoch) ==
    /\ epoch \in Epochs
    /\ AllowedFrom(frontier, epoch)
    /\ LET drops == Defect = "DropAllSibling"
           multiplier == IF Defect = "DoubleSibling" THEN 2 ELSE 1
           nextBalance == IF drops THEN balance ELSE CreditBalance(multiplier)
           nextFrontier == IF drops THEN frontier ELSE epoch
           nextReceipts == IF drops THEN receipts ELSE receipts \cup ReceiptSet(epoch)
       IN /\ balance' = nextBalance
          /\ receipts' = nextReceipts
          /\ frontier' = nextFrontier
          /\ persistedFrontier' = nextFrontier
          /\ maxFrontierSeen' =
               IF nextFrontier > maxFrontierSeen THEN nextFrontier ELSE maxFrontierSeen
          /\ lastFrontierAfter' = nextFrontier
          /\ lastSupplyAfter' = SumBalance(nextBalance)
    /\ started' = TRUE
    /\ lastAction' = "SiblingClose"
    /\ lastEpoch' = epoch
    /\ lastFrontierBefore' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<active, halted>>

RejectEpochOneBootstrap ==
    /\ Defect = "RejectEpochOneBootstrap"
    /\ frontier = -1
    /\ started' = TRUE
    /\ lastAction' = "RejectedBootstrap"
    /\ lastEpoch' = 1
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastSupplyAfter' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<frontier, persistedFrontier, maxFrontierSeen, balance,
                    active, halted, receipts>>

Bond(validator) ==
    /\ validator \in Validators
    /\ LET catchesUp == Defect = "CatchupBond" /\ frontier >= 0
           nextBalance ==
             [balance EXCEPT ![validator] =
               @ + IF catchesUp THEN MintAmount ELSE 0]
           nextReceipts ==
             IF catchesUp THEN receipts \cup {<<validator, frontier>>} ELSE receipts
       IN /\ balance' = nextBalance
          /\ receipts' = nextReceipts
          /\ lastSupplyAfter' = SumBalance(nextBalance)
    /\ active' = active \cup {validator}
    /\ started' = TRUE
    /\ lastAction' = "Bond"
    /\ lastEpoch' = frontier
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<frontier, persistedFrontier, maxFrontierSeen, halted>>

Slash(validator) ==
    /\ validator \in Validators
    /\ halted' = halted \cup {validator}
    /\ started' = TRUE
    /\ lastAction' = "Slash"
    /\ lastEpoch' = frontier
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastSupplyAfter' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<frontier, persistedFrontier, maxFrontierSeen, balance,
                    active, receipts>>

Redeem(validator) ==
    /\ validator \in Validators
    /\ LET retroactive == Defect = "RetroactiveMint" /\ frontier >= 0
           clears == Defect = "ClearOnRedemption"
           nextBalance ==
             [balance EXCEPT ![validator] =
               @ + IF retroactive THEN MintAmount ELSE 0]
           nextReceipts ==
             IF retroactive THEN receipts \cup {<<validator, frontier>>} ELSE receipts
           nextFrontier == IF clears THEN -1 ELSE frontier
       IN /\ balance' = nextBalance
          /\ receipts' = nextReceipts
          /\ frontier' = nextFrontier
          /\ persistedFrontier' = nextFrontier
          /\ lastFrontierAfter' = nextFrontier
          /\ lastSupplyAfter' = SumBalance(nextBalance)
    /\ halted' = halted \ {validator}
    /\ started' = TRUE
    /\ lastAction' = "Redeem"
    /\ lastEpoch' = frontier
    /\ lastFrontierBefore' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<maxFrontierSeen, active>>

Restart ==
    /\ frontier' = persistedFrontier
    /\ started' = TRUE
    /\ lastAction' = "Restart"
    /\ lastEpoch' = frontier
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = persistedFrontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastSupplyAfter' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<persistedFrontier, maxFrontierSeen, balance, active,
                    halted, receipts>>

Noop ==
    /\ started' = TRUE
    /\ lastAction' = "Noop"
    /\ lastEpoch' = frontier
    /\ lastFrontierBefore' = frontier
    /\ lastFrontierAfter' = frontier
    /\ lastSupplyBefore' = SumBalance(balance)
    /\ lastSupplyAfter' = SumBalance(balance)
    /\ lastEligibleCount' = Cardinality(Eligible)
    /\ UNCHANGED <<frontier, persistedFrontier, maxFrontierSeen, balance,
                    active, halted, receipts>>

Next ==
    \/ \E epoch \in Epochs : NewClose(epoch)
    \/ \E epoch \in Epochs : DuplicateClose(epoch)
    \/ \E epoch \in Epochs : GapClose(epoch)
    \/ \E epoch \in Epochs : SiblingClose(epoch)
    \/ RejectEpochOneBootstrap
    \/ \E validator \in Validators : Bond(validator)
    \/ \E validator \in Validators : Slash(validator)
    \/ \E validator \in Validators : Redeem(validator)
    \/ Restart
    \/ Noop

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ frontier \in -1..3
    /\ persistedFrontier \in -1..3
    /\ maxFrontierSeen \in -1..3
    /\ balance \in [Validators -> Nat]
    /\ active \subseteq Validators
    /\ halted \subseteq Validators
    /\ receipts \subseteq EpochPairs
    /\ started \in BOOLEAN
    /\ lastAction \in Actions
    /\ lastEpoch \in -1..3
    /\ lastFrontierBefore \in -1..3
    /\ lastFrontierAfter \in -1..3
    /\ lastSupplyBefore \in Nat
    /\ lastSupplyAfter \in Nat
    /\ lastEligibleCount \in 0..Cardinality(Validators)

InitialFrontierIsUnminted == started \/ frontier = -1

FrontierMatchesPersistence == frontier = persistedFrontier

FrontierNeverRegresses == frontier = maxFrontierSeen

SupplyMatchesLogicalReceipts ==
    SumBalance(balance) = Cardinality(receipts) * MintAmount

ReceiptsDoNotExceedFrontier ==
    \A receipt \in receipts : receipt[2] <= frontier

CommittedCloseWasContiguous ==
    lastAction \in {"NewClose", "SiblingClose"} =>
      AllowedFrom(lastFrontierBefore, lastEpoch)

NewCloseAdvancesExactly ==
    lastAction = "NewClose" => lastFrontierAfter = lastEpoch

DuplicateCloseDoesNotMint ==
    lastAction = "DuplicateClose" =>
      /\ lastFrontierAfter = lastFrontierBefore
      /\ lastSupplyAfter = lastSupplyBefore

GapCloseIsRejected ==
    lastAction = "GapClose" =>
      /\ lastFrontierAfter = lastFrontierBefore
      /\ lastSupplyAfter = lastSupplyBefore

SiblingCloseKeepsExactlyOne ==
    lastAction = "SiblingClose" =>
      /\ lastFrontierAfter = lastEpoch
      /\ lastSupplyAfter =
           lastSupplyBefore + lastEligibleCount * MintAmount

LifecycleDoesNotMint ==
    lastAction \in {"Bond", "Slash", "Redeem"} =>
      lastSupplyAfter = lastSupplyBefore

LifecyclePreservesFrontier ==
    lastAction \in {"Bond", "Slash", "Redeem"} =>
      lastFrontierAfter = lastFrontierBefore

RestartPreservesFrontier ==
    lastAction = "Restart" => lastFrontierAfter = lastFrontierBefore

RequiredBootstrapIsAccepted == lastAction # "RejectedBootstrap"

Inv ==
    /\ TypeOK
    /\ InitialFrontierIsUnminted
    /\ FrontierMatchesPersistence
    /\ FrontierNeverRegresses
    /\ SupplyMatchesLogicalReceipts
    /\ ReceiptsDoNotExceedFrontier
    /\ CommittedCloseWasContiguous
    /\ NewCloseAdvancesExactly
    /\ DuplicateCloseDoesNotMint
    /\ GapCloseIsRejected
    /\ SiblingCloseKeepsExactlyOne
    /\ LifecycleDoesNotMint
    /\ LifecyclePreservesFrontier
    /\ RestartPreservesFrontier
    /\ RequiredBootstrapIsAccepted

InductiveInit ==
    /\ frontier \in Int
    /\ persistedFrontier \in Int
    /\ maxFrontierSeen \in Int
    /\ balance \in [Validators -> Int]
    /\ active \in SUBSET Validators
    /\ halted \in SUBSET Validators
    /\ receipts \in SUBSET EpochPairs
    /\ started \in BOOLEAN
    /\ lastAction \in Actions
    /\ lastEpoch \in Int
    /\ lastFrontierBefore \in Int
    /\ lastFrontierAfter \in Int
    /\ lastSupplyBefore \in Int
    /\ lastSupplyAfter \in Int
    /\ lastEligibleCount \in Int
    /\ Inv

=============================================================================
