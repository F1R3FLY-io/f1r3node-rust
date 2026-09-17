------------------------ MODULE AccountableFinality ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
  \* @type: Set(Int);
  Validators,
  \* @type: Set(Int);
  Candidates,
  \* @type: Int;
  StakeOne,
  \* @type: Int;
  StakeTwo,
  \* @type: Int;
  StakeThree,
  \* @type: Set(Int);
  Faulty,
  \* @type: Int;
  ThresholdNum,
  \* @type: Int;
  ThresholdDen,
  \* @type: Bool;
  EnforceAccountability

Stake == [validator \in Validators |->
  CASE validator = 1 -> StakeOne
    [] validator = 2 -> StakeTwo
    [] OTHER -> StakeThree]

ASSUME /\ Validators = {1, 2, 3}
       /\ Candidates = {1, 2}
       /\ StakeOne \in Nat
       /\ StakeTwo \in Nat
       /\ StakeThree \in Nat
       /\ Faulty \subseteq Validators
       /\ ThresholdNum > 0
       /\ ThresholdDen > 0
       /\ ThresholdNum <= ThresholdDen
       /\ EnforceAccountability \in BOOLEAN

Incompatible ==
  {pair \in Candidates \X Candidates : pair[1] # pair[2]}

SetWeight(validators) ==
  (IF 1 \in validators THEN Stake[1] ELSE 0) +
  (IF 2 \in validators THEN Stake[2] ELSE 0) +
  (IF 3 \in validators THEN Stake[3] ELSE 0)

TotalStake == SetWeight(Validators)
FaultyStake == SetWeight(Faulty)

VARIABLE
  \* @type: Set(<<Int, Int>>);
  support

\* @type: <<Set(<<Int, Int>>)>>;
vars == <<support>>

Init == support = {}

Supports(validator, candidate) == <<validator, candidate>> \in support

CanSupport(validator, candidate) ==
  /\ validator \in Validators
  /\ candidate \in Candidates
  /\ ~Supports(validator, candidate)
  /\ \/ ~EnforceAccountability
     \/ validator \in Faulty
     \/ \A prior \in Candidates :
          Supports(validator, prior) =>
            <<candidate, prior>> \notin Incompatible

AddSupport(validator, candidate) ==
  /\ CanSupport(validator, candidate)
  /\ support' = support \union {<<validator, candidate>>}

Next ==
  \/ \E validator \in Validators, candidate \in Candidates :
       AddSupport(validator, candidate)
  \/ UNCHANGED vars

Supporters(candidate) ==
  {validator \in Validators : Supports(validator, candidate)}

FloorCertified(candidate) ==
  2 * SetWeight(Supporters(candidate)) * ThresholdDen
    >= TotalStake * (ThresholdDen + ThresholdNum)

LfbCertified(candidate) ==
  2 * SetWeight(Supporters(candidate)) * ThresholdDen
    > TotalStake * (ThresholdDen + ThresholdNum)

ConflictingFloorCertificates ==
  \E left \in Candidates, right \in Candidates :
    /\ <<left, right>> \in Incompatible
    /\ FloorCertified(left)
    /\ FloorCertified(right)

ConflictingLfbCertificates ==
  \E left \in Candidates, right \in Candidates :
    /\ <<left, right>> \in Incompatible
    /\ LfbCertified(left)
    /\ LfbCertified(right)

Inv_Accountability ==
  \A validator \in Validators \ Faulty,
      left \in Candidates,
      right \in Candidates :
    <<left, right>> \in Incompatible =>
      ~(Supports(validator, left) /\ Supports(validator, right))

Inv_FloorConflictRequiresFaultBudget ==
  ConflictingFloorCertificates =>
    FaultyStake * ThresholdDen >= TotalStake * ThresholdNum

Inv_LfbConflictRequiresFaultBudget ==
  ConflictingLfbCertificates =>
    FaultyStake * ThresholdDen > TotalStake * ThresholdNum

Inv_NoConflictingFloorBelowBudget ==
  FaultyStake * ThresholdDen < TotalStake * ThresholdNum =>
    ~ConflictingFloorCertificates

Inv_NoConflictingLfbAtBudget ==
  FaultyStake * ThresholdDen <= TotalStake * ThresholdNum =>
    ~ConflictingLfbCertificates

Inv_NoConflictingFloor == ~ConflictingFloorCertificates

Spec == Init /\ [][Next]_vars

=============================================================================
