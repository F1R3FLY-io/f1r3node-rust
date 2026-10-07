-------------------- MODULE NativeReplayAuthentication --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Workers, Channels, SkipFootprint, SkipCommSource
VARIABLES actual, declared, sourceMatches, phase, held, effects, published
vars == <<actual, declared, sourceMatches, phase, held, effects, published>>
Footprints == (SUBSET Channels) \ {{}}

Init ==
  /\ actual \in [Workers -> Footprints]
  /\ declared \in [Workers -> Footprints]
  /\ sourceMatches \in [Workers -> BOOLEAN]
  /\ phase = [w \in Workers |-> "Ready"]
  /\ held = {}
  /\ effects = [c \in Channels |-> 0]
  /\ published = {}

Acquire(w) ==
  /\ phase[w] = "Ready"
  /\ \A other \in held : actual[w] \cap actual[other] = {}
  /\ held' = held \cup {w}
  /\ phase' = [phase EXCEPT ![w] = "Footprint"]
  /\ UNCHANGED <<actual, declared, sourceMatches, effects, published>>

CheckFootprint(w) ==
  /\ phase[w] = "Footprint"
  /\ actual[w] = declared[w] \/ SkipFootprint
  /\ phase' = [phase EXCEPT ![w] = "Introduction"]
  /\ UNCHANGED <<actual, declared, sourceMatches, held, effects, published>>

ObserveIntroduction(w) ==
  /\ phase[w] = "Introduction"
  /\ phase' = [phase EXCEPT ![w] = "Comm"]
  /\ UNCHANGED <<actual, declared, sourceMatches, held, effects, published>>

ObserveComm(w) ==
  /\ phase[w] = "Comm"
  /\ sourceMatches[w] \/ SkipCommSource
  /\ phase' = [phase EXCEPT ![w] = "Prepared"]
  /\ UNCHANGED <<actual, declared, sourceMatches, held, effects, published>>

Publish(w) ==
  /\ phase[w] = "Prepared"
  /\ effects' = [c \in Channels |-> IF c \in actual[w] THEN effects[c] + 1 ELSE effects[c]]
  /\ published' = published \cup {w}
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ held' = held \ {w}
  /\ UNCHANGED <<actual, declared, sourceMatches>>

Cancel(w) ==
  /\ w \in held
  /\ phase' = [phase EXCEPT ![w] = "Cancelled"]
  /\ held' = held \ {w}
  /\ UNCHANGED <<actual, declared, sourceMatches, effects, published>>

Next == \E w \in Workers : Acquire(w) \/ CheckFootprint(w) \/ ObserveIntroduction(w)
        \/ ObserveComm(w) \/ Publish(w) \/ Cancel(w)
Spec == Init /\ [][Next]_vars

TypeOK == /\ phase \in [Workers -> {"Ready", "Footprint", "Introduction", "Comm", "Prepared", "Done", "Cancelled"}]
          /\ held \subseteq Workers /\ published \subseteq Workers
          /\ effects \in [Channels -> 0..Cardinality(Workers)]
AuthenticatedFootprint == \A w \in Workers :
  phase[w] \in {"Introduction", "Comm", "Prepared", "Done"} => actual[w] = declared[w]
AuthenticatedComm == \A w \in Workers : phase[w] \in {"Prepared", "Done"} => sourceMatches[w]
ExclusiveChannels == \A a, b \in held : a = b \/ actual[a] \cap actual[b] = {}
ExactEffects == \A c \in Channels : effects[c] = Cardinality({w \in published : c \in actual[w]})
PreparedOwnership == \A w \in Workers : phase[w] = "Prepared" => w \in held
IndependentAcquisition == \A w \in Workers :
  (phase[w] = "Ready" /\ (\A other \in held : actual[w] \cap actual[other] = {})) => ENABLED Acquire(w)
=============================================================================
