-------------------- MODULE FeeCohortEligibility --------------------
EXTENDS Naturals, FiniteSets, Apalache

CONSTANTS Atoms, IncludeUnrelated, IgnoreMultiplicity
ASSUME /\ Atoms # {} /\ IsFiniteSet(Atoms)
       /\ IncludeUnrelated \in BOOLEAN
       /\ IgnoreMultiplicity \in BOOLEAN

Candidates == [Atoms -> 0..2]
Nonempty(authority) == \E atom \in Atoms : authority[atom] > 0
Contained(candidate, selected) ==
    \A atom \in Atoms : candidate[atom] <= selected[atom]
SupportContained(candidate, selected) ==
    \A atom \in Atoms : candidate[atom] > 0 => selected[atom] > 0

VARIABLES selected, presentations, eligible, done
vars == <<selected, presentations, eligible, done>>
Init ==
    /\ selected \in [Atoms -> 0..1]
    /\ Nonempty(selected)
    /\ presentations = Candidates
    /\ eligible = {}
    /\ done = FALSE
Filter ==
    /\ ~done
    /\ eligible' = {candidate \in presentations :
         Nonempty(candidate) /\
         (IncludeUnrelated \/
           IF IgnoreMultiplicity
           THEN SupportContained(candidate, selected)
           ELSE Contained(candidate, selected))}
    /\ done' = TRUE
    /\ UNCHANGED <<selected, presentations>>
Spec == Init /\ [][Filter]_vars

OnlyAuthenticatedAtoms ==
    \A candidate \in eligible : SupportContained(candidate, selected)
AuthorityMultiplicityIsPreserved ==
    \A candidate \in eligible : Contained(candidate, selected)
EmptyAuthorityCannotPay == \A candidate \in eligible : Nonempty(candidate)
EveryAuthorizedPresentationIsEligible ==
    done => \A candidate \in presentations :
      Nonempty(candidate) /\ Contained(candidate, selected) => candidate \in eligible

=============================================================================
