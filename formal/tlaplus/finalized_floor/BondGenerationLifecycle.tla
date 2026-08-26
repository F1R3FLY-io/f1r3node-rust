-------------------------- MODULE BondGenerationLifecycle --------------------------
EXTENDS Naturals, Integers, FiniteSets, TLC

CONSTANTS
  \* @type: Set(Int);
  Validators,
  \* @type: Int;
  InitialWallet,
  \* @type: Int;
  BondAmount,
  \* @type: Int;
  MaxGeneration,
  \* @type: Bool;
  IncrementGenerationOutsideBond,
  \* @type: Bool;
  AllowRebondWithLiveRecord,
  \* @type: Bool;
  SlashOnlyBonded,
  \* @type: Bool;
  MutateOnStaleSlash,
  \* @type: Bool;
  AllowRebondAfterBurn,
  \* @type: Bool;
  RestoreQuarantineAsBonded,
  \* @type: Bool;
  AllowFullGuiltyPenalty,
  \* @type: Bool;
  WrapGenerationAtLimit

ASSUME Validators /= {}
ASSUME Validators \in {{1}, {1, 2}}
ASSUME InitialWallet > 0
ASSUME BondAmount > 0
ASSUME InitialWallet >= BondAmount
ASSUME MaxGeneration >= 1

Unseen == "Unseen"
Bonded == "Bonded"
PendingWithdraw == "PendingWithdraw"
Withdrawing == "Withdrawing"
Quarantined == "Quarantined"
Withdrawn == "Withdrawn"
Burned == "Burned"
NoOrigin == "NoOrigin"

Phases == {Unseen, Bonded, PendingWithdraw, Withdrawing, Quarantined, Withdrawn, Burned}
LivePhases == {Bonded, PendingWithdraw, Withdrawing, Quarantined}
LockedPhases == {Bonded, PendingWithdraw, Withdrawing}
QuarantineOrigins == LockedPhases \cup {NoOrigin}
GenerationRange == -1..MaxGeneration
TargetGenerationRange == 0..MaxGeneration

\* @type: (Int -> Int) => Int;
SumValues(function) ==
  function[1] + IF 2 \in Validators THEN function[2] ELSE 0

VARIABLES
  \* @type: Int -> Str;
  phase,
  \* @type: Int -> Str;
  quarantineOrigin,
  \* @type: Int -> Int;
  generation,
  \* @type: Int -> Set(Int);
  liveGenerations,
  \* @type: Int -> Int;
  stake,
  \* @type: Int -> Int;
  wallet,
  \* @type: Int;
  cooperativeBalance,
  \* @type: Int;
  burnedBalance,
  \* @type: Int -> Int;
  successfulBonds,
  \* @type: Set(Int);
  mintingHalted,
  \* @type: Bool;
  lastSlashEligible,
  \* @type: Bool;
  lastSlashApplied,
  \* @type: Bool;
  staleSlashMutated,
  \* @type: Bool;
  rebondedAfterBurn,
  \* @type: Str;
  lastRestoredOrigin,
  \* @type: Str;
  lastRestoredPhase,
  \* @type: Bool;
  fullGuiltyApplied

vars ==
  <<phase, quarantineOrigin, generation, liveGenerations, stake, wallet,
    cooperativeBalance, burnedBalance, successfulBonds, mintingHalted,
    lastSlashEligible, lastSlashApplied, staleSlashMutated,
    rebondedAfterBurn, lastRestoredOrigin, lastRestoredPhase,
    fullGuiltyApplied>>

ResetSlashWitness ==
  /\ lastSlashEligible' = FALSE
  /\ lastSlashApplied' = FALSE
  /\ staleSlashMutated' = FALSE
  /\ lastRestoredOrigin' = NoOrigin
  /\ lastRestoredPhase' = NoOrigin
  /\ fullGuiltyApplied' = FALSE

Init ==
  /\ phase = [v \in Validators |-> Unseen]
  /\ quarantineOrigin = [v \in Validators |-> NoOrigin]
  /\ generation = [v \in Validators |-> -1]
  /\ liveGenerations = [v \in Validators |-> {}]
  /\ stake = [v \in Validators |-> 0]
  /\ wallet = [v \in Validators |-> InitialWallet]
  /\ cooperativeBalance = 0
  /\ burnedBalance = 0
  /\ successfulBonds = [v \in Validators |-> 0]
  /\ mintingHalted = {}
  /\ lastSlashEligible = FALSE
  /\ lastSlashApplied = FALSE
  /\ staleSlashMutated = FALSE
  /\ rebondedAfterBurn = FALSE
  /\ lastRestoredOrigin = NoOrigin
  /\ lastRestoredPhase = NoOrigin
  /\ fullGuiltyApplied = FALSE

FreshBond(v) ==
  /\ \/ generation[v] < MaxGeneration
     \/ /\ WrapGenerationAtLimit
        /\ generation[v] = MaxGeneration
  /\ wallet[v] >= BondAmount
  /\ \/ /\ phase[v] \in {Unseen, Withdrawn}
         /\ liveGenerations[v] = {}
      \/ /\ AllowRebondWithLiveRecord
         /\ phase[v] \in LivePhases
      \/ /\ AllowRebondAfterBurn
         /\ phase[v] = Burned
         /\ liveGenerations[v] = {}
  /\ LET nextGeneration ==
           IF WrapGenerationAtLimit /\ generation[v] = MaxGeneration
           THEN 0
           ELSE generation[v] + 1
     IN
       /\ generation' = [generation EXCEPT ![v] = nextGeneration]
       /\ liveGenerations' =
            [liveGenerations EXCEPT ![v] = @ \cup {nextGeneration}]
  /\ phase' = [phase EXCEPT ![v] = Bonded]
  /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
  /\ stake' = [stake EXCEPT ![v] = @ + BondAmount]
  /\ wallet' = [wallet EXCEPT ![v] = @ - BondAmount]
  /\ successfulBonds' = [successfulBonds EXCEPT ![v] = @ + 1]
  /\ rebondedAfterBurn' = (rebondedAfterBurn \/ (phase[v] = Burned))
  /\ UNCHANGED <<cooperativeBalance, burnedBalance, mintingHalted>>
  /\ ResetSlashWitness

FailedBond(v) ==
  /\ phase[v] \in Phases
  /\ UNCHANGED <<phase, quarantineOrigin, generation, liveGenerations, stake, wallet,
                  cooperativeBalance, burnedBalance, successfulBonds,
                  mintingHalted, rebondedAfterBurn>>
  /\ ResetSlashWitness

RequestWithdraw(v) ==
  /\ phase[v] = Bonded
  /\ phase' = [phase EXCEPT ![v] = PendingWithdraw]
  /\ generation' =
       IF IncrementGenerationOutsideBond
       THEN [generation EXCEPT ![v] = @ + 1]
       ELSE generation
  /\ UNCHANGED <<quarantineOrigin, liveGenerations, stake, wallet, cooperativeBalance,
                  burnedBalance, successfulBonds, mintingHalted,
                  rebondedAfterBurn>>
  /\ ResetSlashWitness

BeginWithdraw(v) ==
  /\ phase[v] = PendingWithdraw
  /\ phase' = [phase EXCEPT ![v] = Withdrawing]
  /\ UNCHANGED <<quarantineOrigin, generation, liveGenerations, stake, wallet,
                  cooperativeBalance, burnedBalance, successfulBonds,
                  mintingHalted, rebondedAfterBurn>>
  /\ ResetSlashWitness

PayoutFailure(v) ==
  /\ phase[v] = Withdrawing
  /\ UNCHANGED <<phase, quarantineOrigin, generation, liveGenerations, stake, wallet,
                  cooperativeBalance, burnedBalance, successfulBonds,
                  mintingHalted, rebondedAfterBurn>>
  /\ ResetSlashWitness

PayoutSuccess(v) ==
  /\ phase[v] = Withdrawing
  /\ phase' = [phase EXCEPT ![v] = Withdrawn]
  /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
  /\ liveGenerations' = [liveGenerations EXCEPT ![v] = @ \ {generation[v]}]
  /\ wallet' = [wallet EXCEPT ![v] = @ + stake[v]]
  /\ stake' = [stake EXCEPT ![v] = 0]
  /\ UNCHANGED <<generation, cooperativeBalance, burnedBalance,
                  successfulBonds, mintingHalted, rebondedAfterBurn>>
  /\ ResetSlashWitness

Slash(v, targetGeneration) ==
  /\ targetGeneration \in TargetGenerationRange
  /\ UNCHANGED <<lastRestoredOrigin, lastRestoredPhase, fullGuiltyApplied>>
  /\ LET generationMatches == targetGeneration = generation[v] IN
     LET locked == phase[v] \in LockedPhases IN
     LET eligible == generationMatches /\ locked IN
       /\ lastSlashEligible' = eligible
       /\ IF eligible
          THEN IF SlashOnlyBonded /\ phase[v] /= Bonded
               THEN /\ UNCHANGED <<phase, quarantineOrigin, liveGenerations, stake, wallet,
                                     cooperativeBalance, burnedBalance,
                                     successfulBonds, mintingHalted,
                                     rebondedAfterBurn>>
                    /\ generation' = generation
                    /\ lastSlashApplied' = FALSE
                    /\ staleSlashMutated' = FALSE
               ELSE /\ phase' = [phase EXCEPT ![v] = Quarantined]
                    /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = phase[v]]
                    /\ mintingHalted' = mintingHalted \cup {v}
                    /\ lastSlashApplied' = TRUE
                    /\ staleSlashMutated' = FALSE
                    /\ UNCHANGED <<generation, liveGenerations, stake, wallet,
                                    cooperativeBalance, burnedBalance,
                                    successfulBonds, rebondedAfterBurn>>
          ELSE IF generationMatches /\ phase[v] = Quarantined
               THEN /\ UNCHANGED <<phase, quarantineOrigin, generation, liveGenerations, stake,
                                     wallet, cooperativeBalance, burnedBalance,
                                     successfulBonds, mintingHalted,
                                     rebondedAfterBurn>>
                    /\ lastSlashApplied' = TRUE
                    /\ staleSlashMutated' = FALSE
               ELSE IF MutateOnStaleSlash /\ targetGeneration /= generation[v]
                    THEN /\ phase' = [phase EXCEPT ![v] = Quarantined]
                         /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
                         /\ mintingHalted' = mintingHalted \cup {v}
                         /\ lastSlashApplied' = TRUE
                         /\ staleSlashMutated' = TRUE
                         /\ UNCHANGED <<generation, liveGenerations, stake,
                                         wallet, cooperativeBalance,
                                         burnedBalance, successfulBonds,
                                         rebondedAfterBurn>>
                    ELSE /\ UNCHANGED <<phase, quarantineOrigin, generation, liveGenerations,
                                         stake, wallet, cooperativeBalance,
                                         burnedBalance, successfulBonds,
                                         mintingHalted, rebondedAfterBurn>>
                         /\ lastSlashApplied' = FALSE
                         /\ staleSlashMutated' = FALSE

Vindicate(v) ==
  /\ phase[v] = Quarantined
  /\ quarantineOrigin[v] \in LockedPhases
  /\ LET restoredPhase ==
           IF RestoreQuarantineAsBonded THEN Bonded ELSE quarantineOrigin[v]
     IN /\ phase' = [phase EXCEPT ![v] = restoredPhase]
        /\ lastRestoredOrigin' = quarantineOrigin[v]
        /\ lastRestoredPhase' = restoredPhase
  /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
  /\ mintingHalted' = mintingHalted \ {v}
  /\ lastSlashEligible' = FALSE
  /\ lastSlashApplied' = FALSE
  /\ staleSlashMutated' = FALSE
  /\ fullGuiltyApplied' = FALSE
  /\ UNCHANGED <<generation, liveGenerations, stake, wallet,
                  cooperativeBalance, burnedBalance, successfulBonds,
                  rebondedAfterBurn>>

Guilty(v, penalty) ==
  /\ phase[v] = Quarantined
  /\ quarantineOrigin[v] \in LockedPhases
  /\ penalty \in
       (IF AllowFullGuiltyPenalty THEN 0..stake[v] ELSE 0..(stake[v] - 1))
  /\ LET remainder == stake[v] - penalty
         restoredPhase ==
           IF RestoreQuarantineAsBonded THEN Bonded ELSE quarantineOrigin[v]
     IN /\ phase' = [phase EXCEPT ![v] = restoredPhase]
       /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
       /\ stake' = [stake EXCEPT ![v] = remainder]
       /\ liveGenerations' = liveGenerations
       /\ lastRestoredOrigin' = quarantineOrigin[v]
       /\ lastRestoredPhase' = restoredPhase
       /\ fullGuiltyApplied' = (penalty = stake[v])
  /\ cooperativeBalance' = cooperativeBalance + penalty
  /\ mintingHalted' = mintingHalted \ {v}
  /\ lastSlashEligible' = FALSE
  /\ lastSlashApplied' = FALSE
  /\ staleSlashMutated' = FALSE
  /\ UNCHANGED <<generation, wallet, burnedBalance, successfulBonds,
                  rebondedAfterBurn>>

Burn(v) ==
  /\ phase[v] = Quarantined
  /\ phase' = [phase EXCEPT ![v] = Burned]
  /\ quarantineOrigin' = [quarantineOrigin EXCEPT ![v] = NoOrigin]
  /\ liveGenerations' = [liveGenerations EXCEPT ![v] = @ \ {generation[v]}]
  /\ burnedBalance' = burnedBalance + stake[v]
  /\ stake' = [stake EXCEPT ![v] = 0]
  /\ UNCHANGED <<generation, wallet, cooperativeBalance, successfulBonds,
                  mintingHalted, rebondedAfterBurn>>
  /\ ResetSlashWitness

Next ==
  \/ \E v \in Validators : FreshBond(v)
  \/ \E v \in Validators : FailedBond(v)
  \/ \E v \in Validators : RequestWithdraw(v)
  \/ \E v \in Validators : BeginWithdraw(v)
  \/ \E v \in Validators : PayoutFailure(v)
  \/ \E v \in Validators : PayoutSuccess(v)
  \/ \E v \in Validators, targetGeneration \in TargetGenerationRange :
       Slash(v, targetGeneration)
  \/ \E v \in Validators : Vindicate(v)
  \/ \E v \in Validators, penalty \in 0..InitialWallet : Guilty(v, penalty)
  \/ \E v \in Validators : Burn(v)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Validators -> Phases]
  /\ quarantineOrigin \in [Validators -> QuarantineOrigins]
  /\ generation \in [Validators -> GenerationRange]
  /\ liveGenerations \in [Validators -> SUBSET (0..MaxGeneration)]
  /\ stake \in [Validators -> 0..InitialWallet]
  /\ wallet \in [Validators -> 0..InitialWallet]
  /\ cooperativeBalance \in 0..(InitialWallet * Cardinality(Validators))
  /\ burnedBalance \in 0..(InitialWallet * Cardinality(Validators))
  /\ successfulBonds \in [Validators -> 0..(MaxGeneration + 1)]
  /\ mintingHalted \subseteq Validators
  /\ lastSlashEligible \in BOOLEAN
  /\ lastSlashApplied \in BOOLEAN
  /\ staleSlashMutated \in BOOLEAN
  /\ rebondedAfterBurn \in BOOLEAN
  /\ lastRestoredOrigin \in QuarantineOrigins
  /\ lastRestoredPhase \in QuarantineOrigins
  /\ fullGuiltyApplied \in BOOLEAN

Inv_GenerationEqualsSuccessfulBondCount ==
  \A v \in Validators : generation[v] = successfulBonds[v] - 1

Inv_AtMostOneLiveGenerationPerKey ==
  \A v \in Validators : Cardinality(liveGenerations[v]) <= 1

Inv_LivePhaseHasCurrentGeneration ==
  \A v \in Validators :
    (phase[v] \in LivePhases) => liveGenerations[v] = {generation[v]}

Inv_ResolvedPhaseHasNoLiveGeneration ==
  \A v \in Validators :
    (phase[v] \in {Unseen, Withdrawn, Burned}) => liveGenerations[v] = {}

Inv_StakeTracksLiveGeneration ==
  \A v \in Validators : (stake[v] > 0) <=> (liveGenerations[v] /= {})

Inv_LockedStakeConserved ==
  SumValues(wallet) + SumValues(stake) +
  cooperativeBalance + burnedBalance = InitialWallet * Cardinality(Validators)

Inv_CurrentLockedSlashApplies == lastSlashEligible => lastSlashApplied

Inv_StaleSlashIsNoninterfering == ~staleSlashMutated

Inv_BurnedGenerationCannotRebond == ~rebondedAfterBurn

Inv_QuarantineOriginRestoresExactPhase ==
  \A v \in Validators :
    /\ (phase[v] = Quarantined) <=> (quarantineOrigin[v] \in LockedPhases)
    /\ (phase[v] /= Quarantined) => quarantineOrigin[v] = NoOrigin

Inv_RedemptionRestoresExactPreSlashPhase ==
  lastRestoredOrigin = NoOrigin \/ lastRestoredOrigin = lastRestoredPhase

Inv_GuiltyPenaltyIsStrictlyPartial == ~fullGuiltyApplied

=============================================================================
