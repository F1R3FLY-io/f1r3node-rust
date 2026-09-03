---------------- MODULE AppliedStateValidationPrecedence ----------------
EXTENDS FiniteSets, Naturals, Sequences

CONSTANT
  \* @type: Bool;
  ResolveClaimsFirst

ASSUME ResolveClaimsFirst \in BOOLEAN

\* @type: Set(Int);
Effects == 1..3
\* @type: Set(Str);
Users == {"user-a", "user-b"}
\* @type: Set(Str);
Results == {"Pending", "Invalid", "Deferred", "Accepted"}

\* @type: Set(Seq(Int));
Vectors == {
  <<>>,
  <<1>>,
  <<2>>,
  <<3>>,
  <<1, 2>>,
  <<1, 3>>,
  <<2, 3>>,
  <<2, 1>>,
  <<1, 1>>,
  <<1, 2, 3>>
}

\* @type: Set(Seq(Int));
CanonicalVectors == {
  <<>>,
  <<1>>,
  <<2>>,
  <<3>>,
  <<1, 2>>,
  <<1, 3>>,
  <<2, 3>>,
  <<1, 2, 3>>
}

\* @type: Int -> Str;
EffectUser == [effect \in Effects |->
  CASE effect = 1 -> "user-a"
    [] effect = 2 -> "user-b"
    [] OTHER -> "system"]

\* @type: (Seq(Int)) => Set(Int);
EffectSet(vector) ==
  {vector[index] : index \in DOMAIN vector}

\* @type: (Seq(Int)) => Bool;
IsCanonical(vector) ==
  /\ Cardinality(EffectSet(vector)) = Len(vector)
  /\ \A left \in DOMAIN vector, right \in DOMAIN vector :
       left < right => vector[left] < vector[right]

\* @type: (Seq(Int)) => Set(Str);
UserProjection(vector) ==
  {EffectUser[vector[index]] :
    index \in {candidate \in DOMAIN vector :
      EffectUser[vector[candidate]] \in Users}}

VARIABLES
  \* @type: Seq(Int);
  claimed,
  \* @type: Seq(Int);
  computed,
  \* @type: Set(Int);
  held,
  \* @type: Set(Str);
  declaredScope,
  \* @type: Str;
  result,
  \* @type: Set(Int);
  lookupTargets

vars == <<claimed, computed, held, declaredScope, result, lookupTargets>>

\* @type: (Seq(Int)) => Set(Int);
Missing(vector) == EffectSet(vector) \ held

ExactDecision ==
  IF Missing(computed) # {}
  THEN "Deferred"
  ELSE IF declaredScope = UserProjection(computed)
       THEN "Accepted"
       ELSE "Invalid"

SafeValidation ==
  IF ~IsCanonical(claimed) \/ claimed # computed
  THEN
    /\ result' = "Invalid"
    /\ lookupTargets' = {}
  ELSE
    /\ result' = ExactDecision
    /\ lookupTargets' = EffectSet(computed)

ClaimsFirstValidation ==
  /\ lookupTargets' = EffectSet(claimed)
  /\ result' =
       IF Missing(claimed) # {}
       THEN "Deferred"
       ELSE IF ~IsCanonical(claimed) \/ claimed # computed
            THEN "Invalid"
            ELSE ExactDecision

Init ==
  /\ claimed \in Vectors
  /\ computed \in CanonicalVectors
  /\ held \in SUBSET Effects
  /\ declaredScope \in SUBSET Users
  /\ result = "Pending"
  /\ lookupTargets = {}

Validate ==
  /\ result = "Pending"
  /\ IF ResolveClaimsFirst
     THEN ClaimsFirstValidation
     ELSE SafeValidation
  /\ UNCHANGED <<claimed, computed, held, declaredScope>>

Idle == UNCHANGED vars

Next == Validate \/ Idle

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ claimed \in Vectors
  /\ computed \in CanonicalVectors
  /\ held \in SUBSET Effects
  /\ declaredScope \in SUBSET Users
  /\ result \in Results
  /\ lookupTargets \in SUBSET Effects

Inv_UnequalVectorIsInvalidWithoutLookup ==
  result # "Pending" /\ (~IsCanonical(claimed) \/ claimed # computed)
    => result = "Invalid" /\ lookupTargets = {}

Inv_ExactMissingDependencyDefers ==
  result # "Pending"
    /\ IsCanonical(claimed)
    /\ claimed = computed
    /\ Missing(computed) # {}
    => result = "Deferred" /\ lookupTargets = EffectSet(computed)

Inv_AcceptedImpliesExactVectorAndProjection ==
  result = "Accepted"
    => /\ IsCanonical(claimed)
       /\ claimed = computed
       /\ Missing(computed) = {}
       /\ declaredScope = UserProjection(computed)
       /\ lookupTargets = EffectSet(computed)

Inv_AbsentExtraDoesNotAmplifyDependencies ==
  result # "Pending" /\ (EffectSet(claimed) \ EffectSet(computed)) # {}
    => result = "Invalid" /\ lookupTargets = {}

Inv_HeldInheritedNonAppliedEffectIsInvalid ==
  result # "Pending"
    /\ 3 \in held
    /\ 3 \in EffectSet(claimed)
    /\ 3 \notin EffectSet(computed)
    => result = "Invalid" /\ lookupTargets = {}

=============================================================================
