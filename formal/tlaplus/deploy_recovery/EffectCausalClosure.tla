---------------------- MODULE EffectCausalClosure ----------------------
EXTENDS FiniteSets

CONSTANTS
    \* @type: Bool;
    UseBlockLineage,
    \* @type: Bool;
    UseTransitiveClosure

ASSUME /\ UseBlockLineage \in BOOLEAN
       /\ UseTransitiveClosure \in BOOLEAN

BaseClose == "base-close"
StaleClose == "stale-close"
CausalChild == "causal-child"
TransitiveChild == "transitive-child"
MergeChild == "merge-child"
UserEffect == "user-effect"

Effects ==
    {BaseClose,
     StaleClose,
     CausalChild,
     TransitiveChild,
     MergeChild,
     UserEffect}

Candidates == Effects \ {BaseClose, StaleClose}
SeedRejected == {StaleClose}
IndependentEffects == {MergeChild, UserEffect}
CausalRejectClosure == {StaleClose, CausalChild, TransitiveChild}

Dependencies(effect) ==
    CASE effect = CausalChild -> {StaleClose}
      [] effect = TransitiveChild -> {CausalChild}
      [] effect = MergeChild -> {BaseClose}
      [] OTHER -> {}

BlockDescendantsOfRejected ==
    {CausalChild, TransitiveChild, MergeChild, UserEffect}

VARIABLES
    \* @type: Set(Str);
    pending,
    \* @type: Set(Str);
    accepted,
    \* @type: Set(Str);
    rejected

\* @type: <<Set(Str), Set(Str), Set(Str)>>;
vars == <<pending, accepted, rejected>>

Init ==
    /\ pending = Candidates
    /\ accepted = {BaseClose}
    /\ rejected = SeedRejected

Ready(effect) == Dependencies(effect) \intersect pending = {}

ShouldReject(effect) ==
    IF UseBlockLineage
    THEN effect \in BlockDescendantsOfRejected
    ELSE IF UseTransitiveClosure
         THEN Dependencies(effect) \intersect rejected /= {}
         ELSE Dependencies(effect) \intersect SeedRejected /= {}

Classify(effect) ==
    /\ effect \in pending
    /\ Ready(effect)
    /\ pending' = pending \ {effect}
    /\ IF ShouldReject(effect)
       THEN /\ rejected' = rejected \union {effect}
            /\ accepted' = accepted
       ELSE /\ accepted' = accepted \union {effect}
            /\ rejected' = rejected

Next == \E effect \in pending : Classify(effect)

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ pending \subseteq Candidates
    /\ accepted \subseteq Effects
    /\ rejected \subseteq Effects

Inv_DisjointDisposition == accepted \intersect rejected = {}

Inv_NoAcceptedDependsOnRejected ==
    \A effect \in accepted : Dependencies(effect) \intersect rejected = {}

Inv_IndependentEffectsSurvive ==
    pending = {} => IndependentEffects \subseteq accepted

Inv_CausalClosureComplete ==
    pending = {} => CausalRejectClosure \subseteq rejected

AllEffectsClassified == <> (pending = {})

=======================================================================
