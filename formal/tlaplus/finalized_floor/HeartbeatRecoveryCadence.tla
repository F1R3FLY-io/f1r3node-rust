------------------------ MODULE HeartbeatRecoveryCadence ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Set(Int);
  Validators,
  \* @type: Int;
  MaxElapsed,
  \* @type: Int;
  StallTimeout,
  \* @type: Int;
  RecoveryInterval,
  \* @type: Bool;
  UseSeparatedCadence

NoRound == MaxElapsed + 1
Rounds == 0..MaxElapsed
RoundRefs == Rounds \union {NoRound}

ASSUME /\ Validators # {}
       /\ MaxElapsed > StallTimeout
       /\ StallTimeout > 0
       /\ RecoveryInterval > 0
       /\ UseSeparatedCadence \in BOOLEAN

ContractRound(duration) ==
  IF duration < StallTimeout
  THEN NoRound
  ELSE (duration - StallTimeout) \div RecoveryInterval

CollapsedRound(duration) ==
  IF duration < StallTimeout
  THEN NoRound
  ELSE (duration \div StallTimeout) - 1

SelectedRound(duration) ==
  IF UseSeparatedCadence
  THEN ContractRound(duration)
  ELSE CollapsedRound(duration)

VARIABLES
  \* @type: Int -> Int;
  elapsed,
  \* @type: Int -> Int;
  nextRecoveryRound,
  \* @type: Set(<<Int, Int, Int>>);
  attempts

vars == <<elapsed, nextRecoveryRound, attempts>>

Init ==
  /\ elapsed = [validator \in Validators |-> 0]
  /\ nextRecoveryRound = [validator \in Validators |-> 0]
  /\ attempts = {}

AdvanceClock(validator, nextElapsed) ==
  /\ elapsed[validator] < nextElapsed
  /\ nextElapsed <= MaxElapsed
  /\ elapsed' = [elapsed EXCEPT ![validator] = nextElapsed]
  /\ UNCHANGED <<nextRecoveryRound, attempts>>

AttemptRecovery(validator) ==
  LET highestAvailable == SelectedRound(elapsed[validator])
      round == nextRecoveryRound[validator]
  IN /\ highestAvailable # NoRound
     /\ round <= highestAvailable
     /\ nextRecoveryRound' =
          [nextRecoveryRound EXCEPT ![validator] = @ + 1]
     /\ attempts' =
          attempts \union {<<validator, round, elapsed[validator]>>}
     /\ UNCHANGED elapsed

Next ==
  \/ \E validator \in Validators :
       \E nextElapsed \in 0..MaxElapsed :
         AdvanceClock(validator, nextElapsed)
  \/ \E validator \in Validators : AttemptRecovery(validator)

Spec ==
  /\ Init
  /\ [][Next]_vars

TypeOK ==
  /\ elapsed \in [Validators -> 0..MaxElapsed]
  /\ nextRecoveryRound \in [Validators -> RoundRefs]
  /\ \A attempt \in attempts :
       /\ attempt[1] \in Validators
       /\ attempt[2] \in Rounds
       /\ attempt[3] \in 0..MaxElapsed

Inv_CadenceMatchesContract ==
  \A validator \in Validators :
    SelectedRound(elapsed[validator]) = ContractRound(elapsed[validator])

Inv_NoPrematureAttempt ==
  \A attempt \in attempts : attempt[3] >= StallTimeout

Inv_AttemptUsesContractRound ==
  \A attempt \in attempts :
    /\ ContractRound(attempt[3]) # NoRound
    /\ attempt[2] <= ContractRound(attempt[3])

Inv_OneAttemptPerLocalRound ==
  \A validator \in Validators :
    \A round \in Rounds :
      Cardinality(
        {attempt \in attempts :
          attempt[1] = validator /\ attempt[2] = round}
      ) <= 1

Inv_CompletedRoundsFormPrefix ==
  \A validator \in Validators :
    \A round \in Rounds :
      (round < nextRecoveryRound[validator])
      <=> \E attempt \in attempts :
            /\ attempt[1] = validator
            /\ attempt[2] = round
=============================================================================
