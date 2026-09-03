-------------------- MODULE StateBoundFrontierExpansion --------------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS AuthorityCount, Backing, Fee, TraceDemand, FreezeStaticCap,
          CreditUnbackedFrontier, LeakSpeculativeEffects, ReplayUsesInitialCap

ASSUME /\ AuthorityCount \in Nat \ {0, 1}
       /\ Backing \in [1..AuthorityCount -> Nat \ {0}]
       /\ Fee \in Nat
       /\ Fee < Backing[1]
       /\ TraceDemand \in Nat \ {0}
       /\ FreezeStaticCap \in BOOLEAN
       /\ CreditUnbackedFrontier \in BOOLEAN
       /\ LeakSpeculativeEffects \in BOOLEAN
       /\ ReplayUsesInitialCap \in BOOLEAN

RECURSIVE PrefixSupply(_)
PrefixSupply(count) ==
  IF count = 0 THEN 0 ELSE PrefixSupply(count - 1) + Backing[count]

Capacity(count) == PrefixSupply(count) - Fee
TotalCapacity == Capacity(AuthorityCount)

VARIABLES phase, known, cap, previousCap, retries, speculativeEffects,
          accepted, rejected, replayCap

vars == <<phase, known, cap, previousCap, retries, speculativeEffects,
          accepted, rejected, replayCap>>

Init ==
  /\ phase = "Attempt"
  /\ known = 1
  /\ cap = Capacity(1)
  /\ previousCap = 0
  /\ retries = 0
  /\ speculativeEffects = 0
  /\ accepted = FALSE
  /\ rejected = FALSE
  /\ replayCap = 0

Accept ==
  /\ TraceDemand <= cap
  /\ accepted' = TRUE
  /\ rejected' = FALSE
  /\ phase' = "Replay"
  /\ UNCHANGED <<known, cap, previousCap, retries,
                  speculativeEffects, replayCap>>

Reject ==
  /\ accepted' = FALSE
  /\ rejected' = TRUE
  /\ phase' = "Done"
  /\ replayCap' = 0
  /\ UNCHANGED <<known, cap, previousCap, retries, speculativeEffects>>

Expand ==
  /\ known < AuthorityCount
  /\ previousCap' = cap
  /\ known' = known + 1
  /\ cap' = IF CreditUnbackedFrontier
             THEN Capacity(known') + 1
             ELSE Capacity(known')
  /\ retries' = retries + 1
  /\ speculativeEffects' = IF LeakSpeculativeEffects
                            THEN speculativeEffects + 1
                            ELSE 0
  /\ UNCHANGED <<phase, accepted, rejected, replayCap>>

Attempt ==
  /\ phase = "Attempt"
  /\ IF TraceDemand <= cap
        THEN Accept
        ELSE IF FreezeStaticCap \/ known = AuthorityCount
             THEN Reject
             ELSE Expand

Replay ==
  /\ phase = "Replay"
  /\ replayCap' = IF ReplayUsesInitialCap THEN Capacity(1) ELSE cap
  /\ phase' = "Done"
  /\ UNCHANGED <<known, cap, previousCap, retries,
                  speculativeEffects, accepted, rejected>>

Next == Attempt \/ Replay

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Attempt)
        /\ WF_vars(Replay)

TypeOK ==
  /\ phase \in {"Attempt", "Replay", "Done"}
  /\ known \in 1..AuthorityCount
  /\ cap \in Nat
  /\ previousCap \in Nat
  /\ retries \in Nat
  /\ speculativeEffects \in Nat
  /\ accepted \in BOOLEAN
  /\ rejected \in BOOLEAN
  /\ replayCap \in Nat

CapacityUsesOnlyAuthenticatedBacking == cap = Capacity(known)

ExpansionIsStrictAndBounded ==
  /\ retries <= AuthorityCount - 1
  /\ (retries > 0 => cap > previousCap)
  /\ cap <= TotalCapacity

SpeculativeAttemptsAreEffectFree == speculativeEffects = 0

AcceptanceRequiresCompleteTrace == accepted => TraceDemand <= cap

CompleteBackedTraceIsAccepted ==
  phase = "Done" /\ TraceDemand <= TotalCapacity => accepted

ReplayUsesTheExpandedBound == phase = "Done" /\ accepted => replayCap = cap

EventuallyDone == <>(phase = "Done")

=============================================================================
