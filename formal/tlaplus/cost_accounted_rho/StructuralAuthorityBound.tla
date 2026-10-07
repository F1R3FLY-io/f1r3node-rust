------------------------- MODULE StructuralAuthorityBound -------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS Introductions, Regions, OuterRegion, InnerRegion,
          OuterOnly, InnerOnly, BothRegions, ReuseIntroductions

ASSUME /\ IsFiniteSet(Introductions)
       /\ IsFiniteSet(Regions)
       /\ OuterRegion \in Regions
       /\ InnerRegion \in Regions
       /\ OuterOnly \subseteq Introductions
       /\ InnerOnly \subseteq Introductions
       /\ BothRegions \subseteq Introductions
       /\ OuterOnly \cup InnerOnly \cup BothRegions = Introductions
       /\ OuterOnly \cap InnerOnly = {}
       /\ OuterOnly \cap BothRegions = {}
       /\ InnerOnly \cap BothRegions = {}
       /\ ReuseIntroductions \in BOOLEAN

ScopeOf(intro) ==
  IF intro \in BothRegions
  THEN {OuterRegion, InnerRegion}
  ELSE IF intro \in OuterOnly THEN {OuterRegion} ELSE {InnerRegion}

VARIABLES remaining, realized, forced

vars == <<remaining, realized, forced>>

Init ==
  /\ remaining = Introductions
  /\ realized = [region \in Regions |-> 0]
  /\ forced = 0

Force(participants) ==
  /\ participants \in SUBSET remaining
  /\ participants # {}
  /\ LET authorities == UNION {ScopeOf(intro) : intro \in participants}
     IN realized' = [region \in Regions |->
          realized[region] + IF region \in authorities THEN 1 ELSE 0]
  /\ remaining' =
       IF ReuseIntroductions
       THEN remaining
       ELSE remaining \ participants
  /\ forced' = forced + 1

Next == \E participants \in SUBSET remaining : Force(participants)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ remaining \subseteq Introductions
  /\ realized \in [Regions -> Nat]
  /\ forced \in Nat

StructuralDemand(region) ==
  Cardinality({intro \in Introductions : region \in ScopeOf(intro)})

RealizedNeverExceedsStructuralDemand ==
  \A region \in Regions : realized[region] <= StructuralDemand(region)

=============================================================================
