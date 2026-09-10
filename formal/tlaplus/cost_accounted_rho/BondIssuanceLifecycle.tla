-------------------- MODULE BondIssuanceLifecycle --------------------
EXTENDS Integers, FiniteSets

CONSTANTS
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "FreshBondSubsidy",
  "RebondSubsidy",
  "InactiveValidatorIssuance",
  "OffBoundaryIssuance",
  "HaltedValidatorIssuance",
  "DuplicateEpochIssuance",
  "GenerationDuringEpoch",
  "PlayOnlySubsidy",
  "ReplayOnlySubsidy"
}

Validators == {"A", "B"}
Epochs == 0..2
Phases == {"Absent", "Bonded", "Active", "Withdrawing", "Quarantined", "Burned"}
InitialAmount == 2
EpochAmount == 1
BondAmount == 1
MaxEpoch == 2

BaseFunds == [v \in Validators |-> IF v = "B" THEN 2 ELSE 0]
GenesisGrant == [v \in Validators |-> IF v = "A" THEN InitialAmount ELSE 0]
InitialPhase == [v \in Validators |-> IF v = "A" THEN "Active" ELSE "Absent"]
InitialGeneration == [v \in Validators |-> IF v = "A" THEN 0 ELSE -1]

MintKey(v, epoch) == [validator |-> v, epoch |-> epoch]
MintKeyType == [validator : Validators, epoch : Epochs]

VARIABLES
  \* @type: Int;
  currentEpoch,
  \* @type: Bool;
  atBoundary,
  \* @type: Str -> Str;
  phase,
  \* @type: Str -> Bool;
  halted,
  \* @type: Str -> Int;
  generation,
  \* @type: Str -> Int;
  custody,
  \* @type: Str -> Int;
  stake,
  \* @type: Str -> Int;
  authorizedIssued,
  \* @type: Set({validator: Str, epoch: Int});
  minted,
  \* @type: Str -> Int;
  playInitialCredit,
  \* @type: Str -> Int;
  replayInitialCredit,
  \* @type: Str -> (Int -> Int);
  playEpochCredit,
  \* @type: Str -> (Int -> Int);
  replayEpochCredit,
  \* @type: Bool;
  offBoundaryCredit,
  \* @type: Bool;
  inactiveCredit,
  \* @type: Bool;
  haltedCredit,
  \* @type: Bool;
  generationViolation

vars == <<
  currentEpoch, atBoundary, phase, halted, generation, custody, stake,
  authorizedIssued, minted, playInitialCredit, replayInitialCredit,
  playEpochCredit, replayEpochCredit, offBoundaryCredit, inactiveCredit,
  haltedCredit, generationViolation
>>

Init ==
  /\ currentEpoch = 0
  /\ atBoundary = FALSE
  /\ phase = InitialPhase
  /\ halted = [v \in Validators |-> FALSE]
  /\ generation = InitialGeneration
  /\ custody = [v \in Validators |-> BaseFunds[v] + GenesisGrant[v]]
  /\ stake = [v \in Validators |-> 0]
  /\ authorizedIssued = GenesisGrant
  /\ minted = {}
  /\ playInitialCredit = GenesisGrant
  /\ replayInitialCredit = GenesisGrant
  /\ playEpochCredit = [v \in Validators |-> [epoch \in Epochs |-> 0]]
  /\ replayEpochCredit = [v \in Validators |-> [epoch \in Epochs |-> 0]]
  /\ offBoundaryCredit = FALSE
  /\ inactiveCredit = FALSE
  /\ haltedCredit = FALSE
  /\ generationViolation = FALSE

FreshBond(v) ==
  LET firstBond == generation[v] = -1
      subsidyEnabled ==
        (Defect = "FreshBondSubsidy" /\ firstBond)
        \/ (Defect = "RebondSubsidy" /\ ~firstBond)
      playOnly == Defect = "PlayOnlySubsidy"
      replayOnly == Defect = "ReplayOnlySubsidy"
      playGrant == IF subsidyEnabled \/ playOnly THEN InitialAmount ELSE 0
      replayGrant == IF subsidyEnabled \/ replayOnly THEN InitialAmount ELSE 0
  IN
  /\ v \in Validators
  /\ phase[v] = "Absent"
  /\ generation[v] < 1
  /\ custody[v] >= BondAmount
  /\ phase' = [phase EXCEPT ![v] = "Bonded"]
  /\ generation' = [generation EXCEPT ![v] = @ + 1]
  /\ custody' = [custody EXCEPT ![v] = @ - BondAmount + playGrant]
  /\ stake' = [stake EXCEPT ![v] = BondAmount]
  /\ playInitialCredit' = [playInitialCredit EXCEPT ![v] = @ + playGrant]
  /\ replayInitialCredit' = [replayInitialCredit EXCEPT ![v] = @ + replayGrant]
  /\ UNCHANGED <<currentEpoch, atBoundary, halted, authorizedIssued, minted,
                  playEpochCredit, replayEpochCredit, offBoundaryCredit,
                  inactiveCredit, haltedCredit, generationViolation>>

BeginEpoch ==
  /\ ~atBoundary
  /\ currentEpoch < MaxEpoch
  /\ currentEpoch' = currentEpoch + 1
  /\ atBoundary' = TRUE
  /\ UNCHANGED <<phase, halted, generation, custody, stake, authorizedIssued,
                  minted, playInitialCredit, replayInitialCredit,
                  playEpochCredit, replayEpochCredit, offBoundaryCredit,
                  inactiveCredit, haltedCredit, generationViolation>>

Activate(v) ==
  /\ atBoundary
  /\ phase[v] = "Bonded"
  /\ phase' = [phase EXCEPT ![v] = "Active"]
  /\ UNCHANGED <<currentEpoch, atBoundary, halted, generation, custody, stake,
                  authorizedIssued, minted, playInitialCredit,
                  replayInitialCredit, playEpochCredit, replayEpochCredit,
                  offBoundaryCredit, inactiveCredit, haltedCredit,
                  generationViolation>>

SafeEpochEligible(v) ==
  /\ atBoundary
  /\ phase[v] = "Active"
  /\ ~halted[v]
  /\ MintKey(v, currentEpoch) \notin minted

UnsafeEpochEligible(v) ==
  \/ /\ Defect = "InactiveValidatorIssuance"
     /\ atBoundary
     /\ phase[v] /= "Active"
     /\ MintKey(v, currentEpoch) \notin minted
  \/ /\ Defect = "OffBoundaryIssuance"
     /\ ~atBoundary
     /\ phase[v] = "Active"
     /\ ~halted[v]
     /\ MintKey(v, currentEpoch) \notin minted
  \/ /\ Defect = "HaltedValidatorIssuance"
     /\ atBoundary
     /\ halted[v]
     /\ MintKey(v, currentEpoch) \notin minted
  \/ /\ Defect = "DuplicateEpochIssuance"
     /\ atBoundary
     /\ phase[v] = "Active"
     /\ ~halted[v]
     /\ MintKey(v, currentEpoch) \in minted

IssueEpoch(v) ==
  /\ v \in Validators
  /\ SafeEpochEligible(v) \/ UnsafeEpochEligible(v)
  /\ custody' = [custody EXCEPT ![v] = @ + EpochAmount]
  /\ authorizedIssued' = [authorizedIssued EXCEPT ![v] = @ + EpochAmount]
  /\ minted' = minted \cup {MintKey(v, currentEpoch)}
  /\ playEpochCredit' =
       [playEpochCredit EXCEPT ![v][currentEpoch] = @ + EpochAmount]
  /\ replayEpochCredit' =
       [replayEpochCredit EXCEPT ![v][currentEpoch] = @ + EpochAmount]
  /\ offBoundaryCredit' = (offBoundaryCredit \/ ~atBoundary)
  /\ inactiveCredit' = (inactiveCredit \/ phase[v] /= "Active")
  /\ haltedCredit' = (haltedCredit \/ halted[v])
  /\ generation' =
       IF Defect = "GenerationDuringEpoch"
       THEN [generation EXCEPT ![v] = @ + 1]
       ELSE generation
  /\ generationViolation' =
       (generationViolation \/ Defect = "GenerationDuringEpoch")
  /\ UNCHANGED <<currentEpoch, atBoundary, phase, halted, stake,
                  playInitialCredit, replayInitialCredit>>

FinishEpoch ==
  /\ atBoundary
  /\ \A v \in Validators :
       (phase[v] = "Active" /\ ~halted[v]) =>
         MintKey(v, currentEpoch) \in minted
  /\ atBoundary' = FALSE
  /\ UNCHANGED <<currentEpoch, phase, halted, generation, custody, stake,
                  authorizedIssued, minted, playInitialCredit,
                  replayInitialCredit, playEpochCredit, replayEpochCredit,
                  offBoundaryCredit, inactiveCredit, haltedCredit,
                  generationViolation>>

RequestWithdrawal(v) ==
  /\ v \in Validators
  /\ phase[v] \in {"Bonded", "Active"}
  /\ phase' = [phase EXCEPT ![v] = "Withdrawing"]
  /\ UNCHANGED <<currentEpoch, atBoundary, halted, generation, custody, stake,
                  authorizedIssued, minted, playInitialCredit,
                  replayInitialCredit, playEpochCredit, replayEpochCredit,
                  offBoundaryCredit, inactiveCredit, haltedCredit,
                  generationViolation>>

CompleteWithdrawal(v) ==
  /\ v \in Validators
  /\ phase[v] = "Withdrawing"
  /\ phase' = [phase EXCEPT ![v] = "Absent"]
  /\ custody' = [custody EXCEPT ![v] = @ + stake[v]]
  /\ stake' = [stake EXCEPT ![v] = 0]
  /\ UNCHANGED <<currentEpoch, atBoundary, halted, generation,
                  authorizedIssued, minted, playInitialCredit,
                  replayInitialCredit, playEpochCredit, replayEpochCredit,
                  offBoundaryCredit, inactiveCredit, haltedCredit,
                  generationViolation>>

Slash(v) ==
  /\ v \in Validators
  /\ phase[v] \in {"Bonded", "Active", "Withdrawing"}
  /\ phase' = [phase EXCEPT ![v] = "Quarantined"]
  /\ halted' = [halted EXCEPT ![v] = TRUE]
  /\ UNCHANGED <<currentEpoch, atBoundary, generation, custody, stake,
                  authorizedIssued, minted, playInitialCredit,
                  replayInitialCredit, playEpochCredit, replayEpochCredit,
                  offBoundaryCredit, inactiveCredit, haltedCredit,
                  generationViolation>>

Redeem(v) ==
  /\ v \in Validators
  /\ phase[v] = "Quarantined"
  /\ phase' = [phase EXCEPT ![v] = "Active"]
  /\ halted' = [halted EXCEPT ![v] = FALSE]
  /\ UNCHANGED <<currentEpoch, atBoundary, generation, custody, stake,
                  authorizedIssued, minted, playInitialCredit,
                  replayInitialCredit, playEpochCredit, replayEpochCredit,
                  offBoundaryCredit, inactiveCredit, haltedCredit,
                  generationViolation>>

Next ==
  \/ \E v \in Validators : FreshBond(v)
  \/ BeginEpoch
  \/ \E v \in Validators : Activate(v)
  \/ \E v \in Validators : IssueEpoch(v)
  \/ FinishEpoch
  \/ \E v \in Validators : RequestWithdrawal(v)
  \/ \E v \in Validators : CompleteWithdrawal(v)
  \/ \E v \in Validators : Slash(v)
  \/ \E v \in Validators : Redeem(v)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ currentEpoch \in Epochs
  /\ atBoundary \in BOOLEAN
  /\ phase \in [Validators -> Phases]
  /\ halted \in [Validators -> BOOLEAN]
  /\ generation \in [Validators -> -1..2]
  /\ custody \in [Validators -> 0..12]
  /\ stake \in [Validators -> 0..BondAmount]
  /\ authorizedIssued \in [Validators -> 0..8]
  /\ minted \subseteq MintKeyType
  /\ playInitialCredit \in [Validators -> 0..8]
  /\ replayInitialCredit \in [Validators -> 0..8]
  /\ playEpochCredit \in [Validators -> [Epochs -> 0..3]]
  /\ replayEpochCredit \in [Validators -> [Epochs -> 0..3]]
  /\ offBoundaryCredit \in BOOLEAN
  /\ inactiveCredit \in BOOLEAN
  /\ haltedCredit \in BOOLEAN
  /\ generationViolation \in BOOLEAN

InitialIssuanceOccursOnlyAtGenesis ==
  /\ playInitialCredit = GenesisGrant
  /\ replayInitialCredit = GenesisGrant

EpochIssuanceRequiresBoundary == ~offBoundaryCredit

EpochIssuanceRequiresActiveMembership == ~inactiveCredit

HaltedValidatorsReceiveNoIssuance == ~haltedCredit

AtMostOneCreditPerValidatorEpoch ==
  \A v \in Validators, epoch \in Epochs :
    /\ playEpochCredit[v][epoch] <= EpochAmount
    /\ replayEpochCredit[v][epoch] <= EpochAmount

EveryEpochCreditHasReceipt ==
  \A v \in Validators, epoch \in Epochs :
    (playEpochCredit[v][epoch] = EpochAmount) <=>
      MintKey(v, epoch) \in minted

GenerationChangesOnlyOnSuccessfulBond == ~generationViolation

CustodyConservedModuloAuthorizedIssuance ==
  \A v \in Validators :
    custody[v] + stake[v] = BaseFunds[v] + authorizedIssued[v]

PlayReplayIssuanceAgree ==
  /\ playInitialCredit = replayInitialCredit
  /\ playEpochCredit = replayEpochCredit

=============================================================================
